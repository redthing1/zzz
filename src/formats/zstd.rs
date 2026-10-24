//! zstd compression format implementation

use crate::archive_plan::ArchivePlan;
use crate::encryption::{
    DecryptingReader, EncryptingWriter, EncryptionHeader, DEFAULT_ENCRYPTION_CHUNK_SIZE,
};
use crate::formats::{
    tarball, ArchiveEntry, CompressionFormat, CompressionOptions, CompressionStats,
    ExtractionOptions,
};
use crate::progress::{Progress, ProgressReader};
use crate::Result;
use anyhow::{anyhow, bail, Context};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

pub struct ZstdFormat;

fn resolved_thread_count(requested_threads: u32) -> u32 {
    if requested_threads == 0 {
        std::thread::available_parallelism()
            .map(|parallelism| parallelism.get() as u32)
            .unwrap_or(1)
    } else {
        requested_threads
    }
}

fn configure_threads<W: Write>(
    encoder: &mut zstd::Encoder<'_, W>,
    requested_threads: u32,
) -> Result<()> {
    let thread_count = resolved_thread_count(requested_threads);

    // Preserve `-j1` as a single-threaded request. zstd's `NbWorkers=1`
    // still offloads compression onto a background worker thread.
    if thread_count <= 1 {
        return Ok(());
    }

    encoder.multithread(thread_count).with_context(|| {
        format!("failed to enable multithreaded zstd compression with {thread_count} workers")
    })?;

    Ok(())
}

fn compress_tarball<W: Write>(
    writer: W,
    plan: &ArchivePlan,
    zstd_level: i32,
    options: &CompressionOptions,
    progress: Option<&Progress>,
    build_options: tarball::BuildOptions,
) -> Result<()> {
    let mut zstd_encoder =
        zstd::Encoder::new(writer, zstd_level).context("Failed to create ZSTD encoder")?;
    configure_threads(&mut zstd_encoder, options.threads)?;

    let zstd_encoder =
        tarball::build_tarball(zstd_encoder, plan, options, progress, build_options)?;

    drop(zstd_encoder.finish()?);

    Ok(())
}

impl CompressionFormat for ZstdFormat {
    fn compress_plan(
        plan: &ArchivePlan,
        output_path: &Path,
        options: &CompressionOptions,
        progress: Option<&Progress>,
    ) -> Result<CompressionStats> {
        // Derive the key before creating or truncating the output file.
        let encryption = options
            .password
            .as_deref()
            .map(|password| {
                let header = EncryptionHeader::new(options.kdf)?;
                let key = header
                    .derive_key(password)
                    .context("Failed to derive encryption key for ZSTD compression")?;
                Ok::<_, anyhow::Error>((header, key))
            })
            .transpose()?;

        // create output file
        let mut underlying_file = File::create(output_path)
            .with_context(|| format!("failed to create output file: {}", output_path.display()))?;
        let zstd_level = if options.level == 0 { 3 } else { options.level };
        let build_options = tarball::BuildOptions {
            directory_slash: true,
        };

        // Handle password-based encryption
        if let Some((header, derived_key)) = encryption {
            header
                .write_to(&mut underlying_file)
                .context("Failed to write encryption header")?;

            let encrypting_writer =
                EncryptingWriter::new(underlying_file, &derived_key, DEFAULT_ENCRYPTION_CHUNK_SIZE)
                    .context("Failed to create EncryptingWriter for ZSTD")?;
            compress_tarball(
                encrypting_writer,
                plan,
                zstd_level,
                options,
                progress,
                build_options,
            )?;
        } else {
            compress_tarball(
                underlying_file,
                plan,
                zstd_level,
                options,
                progress,
                build_options,
            )?;
        }

        let output_size = std::fs::metadata(output_path)?.len();

        // finalize progress
        if let Some(progress) = progress {
            progress.update(plan.total_size);
        }

        Ok(CompressionStats::new(plan.total_size, output_size))
    }

    fn extract(
        archive_path: &Path,
        output_dir: &Path,
        options: &ExtractionOptions,
        progress: Option<&crate::progress::Progress>,
    ) -> Result<()> {
        // open archive file
        let mut archive_file = File::open(archive_path)
            .with_context(|| format!("failed to open archive file: {}", archive_path.display()))?;
        let archive_size = std::fs::metadata(archive_path)
            .with_context(|| {
                format!(
                    "failed to read archive metadata: {}",
                    archive_path.display()
                )
            })?
            .len();

        let header = EncryptionHeader::read_from(&mut archive_file)?;

        // Determine if this is an encrypted archive and set up the input stream
        let (input_stream, bytes_offset): (Box<dyn Read>, u64) = if let Some(header) = header {
            // This is an encrypted archive
            let password = options.password.as_deref().ok_or_else(|| {
                anyhow!(
                    "Encrypted archive '{}' requires a password.",
                    archive_path.display()
                )
            })?;

            if password.is_empty() {
                bail!(
                    "Password cannot be empty for encrypted archive '{}'.",
                    archive_path.display()
                );
            }

            // Derive the decryption key
            let derived_key = header
                .derive_key(password)
                .context("Failed to derive decryption key")?;

            // Create decrypting reader
            let decrypting_reader =
                DecryptingReader::new(ProgressReader::new(archive_file, progress), &derived_key)
                    .context("Failed to create DecryptingReader for ZSTD")?;

            (Box::new(decrypting_reader), header.size() as u64)
        } else {
            // This is a standard (unencrypted) archive
            if options.password.is_some()
                && !options.password.as_deref().unwrap_or_default().is_empty()
            {
                eprintln!(
                    "warning: Password provided, but archive '{}' does not appear to be in the expected encrypted format. Attempting standard extraction.",
                    archive_path.display()
                );
            }

            (Box::new(ProgressReader::new(archive_file, progress)), 0)
        };

        if let Some(progress) = progress {
            progress.set_length(archive_size.saturating_sub(bytes_offset));
        }

        // create zstd decoder with the appropriate input stream
        let decoder = zstd::Decoder::new(input_stream).with_context(|| {
            format!(
                "failed to create zstd decoder for: {}",
                archive_path.display()
            )
        })?;

        tarball::extract_tarball(decoder, output_dir, options, progress)
    }

    fn list(archive_path: &Path) -> Result<Vec<ArchiveEntry>> {
        // open archive file
        let mut archive_file = File::open(archive_path)
            .with_context(|| format!("failed to open archive file: {}", archive_path.display()))?;

        // If this is an encrypted archive, we can't list it without a password
        if EncryptionHeader::read_from(&mut archive_file)?.is_some() {
            return Err(anyhow!(
                "Cannot list encrypted ZSTD archive '{}' - password required. Use the extract command with --password to access contents.",
                archive_path.display()
            ));
        }

        // create zstd decoder
        let decoder = zstd::Decoder::new(archive_file).with_context(|| {
            format!(
                "failed to create zstd decoder for: {}",
                archive_path.display()
            )
        })?;

        tarball::list_tarball(decoder)
    }

    fn extension() -> &'static str {
        "zst"
    }

    fn test_integrity(archive_path: &Path) -> Result<()> {
        // open archive file
        let mut archive_file = File::open(archive_path)
            .with_context(|| format!("failed to open archive file: {}", archive_path.display()))?;

        // Encrypted contents require a password; only the header is checked here.
        if EncryptionHeader::read_from(&mut archive_file)?.is_some() {
            return Ok(());
        }

        // Check if this is a tar.zst or single .zst file
        if archive_path
            .extension()
            .is_some_and(|ext| ext == "tzst" || ext == "tar.zst")
            || archive_path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(".tar.zst"))
        {
            // This is a tar.zst archive - test by reading all tar entries
            let zstd_decoder = zstd::stream::read::Decoder::new(archive_file)?;
            let mut archive = tar::Archive::new(zstd_decoder);
            for entry in archive.entries()? {
                let _entry = entry?; // This will fail if data is corrupted
            }
        } else {
            // Single .zst file - test by decompressing fully
            let mut zstd_decoder = zstd::stream::read::Decoder::new(archive_file)?;
            let mut buffer = Vec::new();
            zstd_decoder.read_to_end(&mut buffer)?;
        }

        Ok(())
    }
}
