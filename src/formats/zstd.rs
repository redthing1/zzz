//! zstd compression format implementation

use crate::encryption::{
    self, DecryptingReader, EncryptingWriter, ARGON2_SALT_LEN, DEFAULT_ENCRYPTION_CHUNK_SIZE,
    ENCRYPTED_ZSTD_MAGIC,
};
use crate::filter::FileFilter;
use crate::formats::{
    ArchiveEntry, CompressionFormat, CompressionOptions, CompressionStats, ExtractionOptions,
};
use crate::progress::Progress;
use crate::Result;
use anyhow::{anyhow, bail, Context};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use tar::Builder;
use walkdir::WalkDir;
use zstd::stream::raw::CParameter;

// File permission constants for security normalization
const NORMALIZED_FILE_MODE: u32 = 0o644;
const NORMALIZED_DIR_MODE: u32 = 0o755;

pub struct ZstdFormat;

impl ZstdFormat {
    /// Create and configure a normalized tar header for files
    fn create_file_header(
        metadata: &std::fs::Metadata,
        options: &CompressionOptions,
    ) -> Result<tar::Header> {
        let mut header = tar::Header::new_gnu();
        header.set_size(metadata.len());
        header.set_mode(if options.normalize_permissions {
            NORMALIZED_FILE_MODE
        } else {
            #[cfg(unix)]
            {
                metadata.permissions().mode()
            }
            #[cfg(not(unix))]
            {
                NORMALIZED_FILE_MODE
            }
        });

        Self::apply_header_normalization(&mut header, metadata, options)?;
        header.set_cksum();
        Ok(header)
    }

    /// Create and configure a normalized tar header for directories
    fn create_dir_header(
        metadata: &std::fs::Metadata,
        options: &CompressionOptions,
    ) -> Result<tar::Header> {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Directory);
        header.set_size(0);
        header.set_mode(if options.normalize_permissions {
            NORMALIZED_DIR_MODE
        } else {
            #[cfg(unix)]
            {
                metadata.permissions().mode()
            }
            #[cfg(not(unix))]
            {
                NORMALIZED_DIR_MODE
            }
        });

        Self::apply_header_normalization(&mut header, metadata, options)?;
        header.set_cksum();
        Ok(header)
    }

    /// Apply common header normalization (ownership, timestamps)
    fn apply_header_normalization(
        header: &mut tar::Header,
        metadata: &std::fs::Metadata,
        options: &CompressionOptions,
    ) -> Result<()> {
        // Set normalized ownership if requested
        if options.normalize_permissions {
            header.set_uid(0);
            header.set_gid(0);
            header.set_username("")?;
            header.set_groupname("")?;
        }

        // Set modification time
        if let Ok(mtime) = metadata.modified() {
            if let Ok(duration) = mtime.duration_since(std::time::UNIX_EPOCH) {
                header.set_mtime(duration.as_secs());
            }
        }

        Ok(())
    }

    /// Collect files to be added to archive, applying filtering
    fn collect_files_to_add(
        input_path: &Path,
        filter: &FileFilter,
    ) -> Result<Vec<std::path::PathBuf>> {
        let mut files_to_add = Vec::new();

        if input_path.is_file() {
            // single file
            if !filter.should_exclude(input_path) {
                files_to_add.push(input_path.to_path_buf());
            }
        } else {
            // directory - walk and filter
            for entry in WalkDir::new(input_path)
                .follow_links(false)
                .sort_by(|a, b| a.file_name().cmp(b.file_name()))
            // deterministic ordering
            {
                let entry = entry?;
                let path = entry.path();

                // apply filtering
                if !filter.should_exclude(path) {
                    files_to_add.push(path.to_path_buf());
                }
            }
        }

        Ok(files_to_add)
    }

    /// Helper function to add files to tar archive (works with any Writer type)
    fn add_files_to_tar<W: Write>(
        tar_builder: &mut Builder<W>,
        _input_path: &Path,
        files_to_add: &[std::path::PathBuf],
        base_path: &Path,
        options: &CompressionOptions,
        progress: Option<&Progress>,
    ) -> Result<()> {
        let mut bytes_processed = 0u64;

        for file_path in files_to_add {
            // calculate relative path for archive
            let archive_path = file_path.strip_prefix(base_path).unwrap_or_else(|_| {
                file_path
                    .file_name()
                    .map(std::path::Path::new)
                    .unwrap_or(file_path)
            });

            if file_path.is_file() {
                let mut file = File::open(file_path).with_context(|| {
                    format!("failed to open file for archiving: {}", file_path.display())
                })?;
                let metadata = file.metadata().with_context(|| {
                    format!("failed to read metadata for file: {}", file_path.display())
                })?;

                // create normalized tar header
                let mut header = Self::create_file_header(&metadata, options)?;

                // add to archive
                tar_builder.append_data(&mut header, archive_path, &mut file)?;

                // update progress
                bytes_processed += metadata.len();
                if let Some(progress) = progress {
                    progress.update(bytes_processed);
                }
            } else if file_path.is_dir() {
                let metadata = file_path.metadata()?;
                let mut header = Self::create_dir_header(&metadata, options)?;

                // ensure directory path ends with /
                let mut dir_path_str = archive_path.to_string_lossy().to_string();
                if !dir_path_str.ends_with('/') {
                    dir_path_str.push('/');
                }

                tar_builder.append_data(&mut header, dir_path_str.as_str(), std::io::empty())?;
            }
        }

        Ok(())
    }
}

impl CompressionFormat for ZstdFormat {
    fn compress(
        input_path: &Path,
        output_path: &Path,
        options: &CompressionOptions,
        filter: &FileFilter,
        progress: Option<&Progress>,
    ) -> Result<CompressionStats> {
        // calculate input size for progress and stats
        let input_size = crate::utils::calculate_dir_size(input_path)?;

        // create output file
        let mut underlying_file = File::create(output_path)
            .with_context(|| format!("failed to create output file: {}", output_path.display()))?;

        let mut key_material: Option<Vec<u8>> = None;

        // Handle password-based encryption
        if let Some(password) = &options.password {
            if !password.is_empty() {
                let (derived_key, salt) = encryption::derive_key(password, None)
                    .context("Failed to derive encryption key for ZSTD compression")?;

                // Write magic header and salt
                underlying_file
                    .write_all(ENCRYPTED_ZSTD_MAGIC)
                    .context("Failed to write encryption magic header")?;
                underlying_file
                    .write_all(&salt)
                    .context("Failed to write encryption salt")?;

                key_material = Some(derived_key);
            }
        }

        // Set up compression parameters
        let zstd_level = if options.level == 0 { 3 } else { options.level };
        let thread_count = if options.threads == 0 {
            num_cpus::get() as u32
        } else {
            options.threads
        };

        // determine base path for relative paths in archive
        let base_path = input_path.parent().unwrap_or(Path::new("."));

        // collect all files to add (with filtering)
        let files_to_add = Self::collect_files_to_add(input_path, filter)?;

        // Handle encrypted vs unencrypted compression differently
        if let Some(key) = key_material {
            // Encrypted compression pipeline
            let encrypting_writer =
                EncryptingWriter::new(underlying_file, &key, DEFAULT_ENCRYPTION_CHUNK_SIZE)
                    .context("Failed to create EncryptingWriter for ZSTD")?;
            let mut zstd_encoder = zstd::Encoder::new(encrypting_writer, zstd_level)
                .context("Failed to create ZSTD encoder for encrypted stream")?;

            // Configure threading
            if thread_count > 1 {
                let _ = zstd_encoder.set_parameter(CParameter::NbWorkers(thread_count));
            }

            // Create tar builder and add files
            let mut tar_builder = Builder::new(zstd_encoder);
            Self::add_files_to_tar(
                &mut tar_builder,
                input_path,
                &files_to_add,
                base_path,
                options,
                progress,
            )?;

            // Finish the tar and get the encoder back
            let encoder = tar_builder.into_inner()?;
            let _inner = encoder.finish()?; // This finishes the zstd encoder, the encrypting writer handles the file
        } else {
            // Standard unencrypted compression pipeline
            let mut zstd_encoder = zstd::Encoder::new(underlying_file, zstd_level)
                .context("Failed to create ZSTD encoder for unencrypted stream")?;

            // Configure threading
            if thread_count > 1 {
                let _ = zstd_encoder.set_parameter(CParameter::NbWorkers(thread_count));
            }

            // Create tar builder and add files
            let mut tar_builder = Builder::new(zstd_encoder);
            Self::add_files_to_tar(
                &mut tar_builder,
                input_path,
                &files_to_add,
                base_path,
                options,
                progress,
            )?;

            // Finish the tar and get the encoder back
            let encoder = tar_builder.into_inner()?;
            let output_file = encoder.finish()?;
            let _ = output_file; // The file is already written
        }

        let output_size = std::fs::metadata(output_path)?.len();

        // finalize progress
        if let Some(progress) = progress {
            progress.update(input_size);
        }

        Ok(CompressionStats::new(input_size, output_size))
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

        // Check for encryption magic header
        let mut magic_buffer = [0u8; ENCRYPTED_ZSTD_MAGIC.len()];
        let bytes_read = archive_file
            .read(&mut magic_buffer)
            .context("Failed to read initial bytes from archive for encryption check")?;

        // Determine if this is an encrypted archive and set up the input stream
        let input_stream: Box<dyn Read> = if bytes_read == ENCRYPTED_ZSTD_MAGIC.len()
            && magic_buffer == *ENCRYPTED_ZSTD_MAGIC
        {
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

            // Read the salt
            let mut salt = vec![0u8; ARGON2_SALT_LEN];
            archive_file
                .read_exact(&mut salt)
                .context("Failed to read salt from encrypted archive")?;

            // Derive the decryption key
            let (derived_key, _used_salt) = encryption::derive_key(password, Some(&salt))
                .context("Failed to derive decryption key")?;

            // Create decrypting reader
            let decrypting_reader = DecryptingReader::new(archive_file, &derived_key)
                .context("Failed to create DecryptingReader for ZSTD")?;

            Box::new(decrypting_reader)
        } else {
            // This is a standard (unencrypted) archive
            archive_file
                .seek(SeekFrom::Start(0))
                .context("Failed to rewind archive file for standard processing")?;

            if options.password.is_some()
                && !options.password.as_deref().unwrap_or_default().is_empty()
            {
                eprintln!(
                    "warning: Password provided, but archive '{}' does not appear to be in the expected encrypted format. Attempting standard extraction.",
                    archive_path.display()
                );
            }

            Box::new(archive_file)
        };

        // create zstd decoder with the appropriate input stream
        let decoder = zstd::Decoder::new(input_stream).with_context(|| {
            format!(
                "failed to create zstd decoder for: {}",
                archive_path.display()
            )
        })?;

        // create tar archive reader
        let mut archive = tar::Archive::new(decoder);

        // extract with safety checks
        let mut entry_count = 0u64;
        for entry_result in archive.entries()? {
            let mut entry = entry_result?;
            let entry_path = entry.path()?;

            // security: prevent directory traversal attacks
            if entry_path
                .components()
                .any(|comp| comp == std::path::Component::ParentDir)
            {
                anyhow::bail!("archive contains unsafe path: {}", entry_path.display());
            }

            // calculate output path
            let output_path = output_dir.join(&entry_path);

            // check for overwrite
            if output_path.exists() && !options.overwrite {
                anyhow::bail!(
                    "file already exists: {} (use --overwrite to force)",
                    output_path.display()
                );
            }

            // ensure parent directory exists
            if let Some(parent) = output_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            // extract the entry
            entry.unpack(&output_path)?;

            // Update progress
            entry_count += 1;
            if let Some(progress) = progress {
                progress.set_position(entry_count);
            }
        }

        Ok(())
    }

    fn list(archive_path: &Path) -> Result<Vec<ArchiveEntry>> {
        // open archive file
        let mut archive_file = File::open(archive_path)
            .with_context(|| format!("failed to open archive file: {}", archive_path.display()))?;

        // Check for encryption magic header
        let mut magic_buffer = [0u8; ENCRYPTED_ZSTD_MAGIC.len()];
        let bytes_read = archive_file
            .read(&mut magic_buffer)
            .context("Failed to read initial bytes from archive for encryption check")?;

        // If this is an encrypted archive, we can't list it without a password
        if bytes_read == ENCRYPTED_ZSTD_MAGIC.len() && magic_buffer == *ENCRYPTED_ZSTD_MAGIC {
            return Err(anyhow!(
                "Cannot list encrypted ZSTD archive '{}' - password required. Use the extract command with --password to access contents.",
                archive_path.display()
            ));
        }

        // This is a standard archive, proceed normally
        archive_file
            .seek(SeekFrom::Start(0))
            .context("Failed to rewind archive file for standard processing")?;

        // create zstd decoder
        let decoder = zstd::Decoder::new(archive_file).with_context(|| {
            format!(
                "failed to create zstd decoder for: {}",
                archive_path.display()
            )
        })?;

        // create tar archive reader
        let mut archive = tar::Archive::new(decoder);

        let mut entries = Vec::new();

        // read entries without extracting
        for entry_result in archive.entries()? {
            let entry = entry_result?;
            let entry_path = entry.path()?;
            let header = entry.header();

            entries.push(ArchiveEntry {
                path: entry_path.to_string_lossy().to_string(),
                size: header.size()?,
                is_file: header.entry_type() == tar::EntryType::Regular,
            });
        }

        Ok(entries)
    }

    fn extension() -> &'static str {
        "zst"
    }

    fn test_integrity(archive_path: &Path) -> Result<()> {
        // open archive file
        let mut archive_file = File::open(archive_path)
            .with_context(|| format!("failed to open archive file: {}", archive_path.display()))?;

        // Check for encryption magic header
        let mut magic_buffer = [0u8; ENCRYPTED_ZSTD_MAGIC.len()];
        let bytes_read = archive_file
            .read(&mut magic_buffer)
            .context("Failed to read initial bytes from archive for encryption check")?;

        // If this is an encrypted archive, we can only verify the header format
        if bytes_read == ENCRYPTED_ZSTD_MAGIC.len() && magic_buffer == *ENCRYPTED_ZSTD_MAGIC {
            // For encrypted archives, we can check if the salt is readable
            let mut salt = vec![0u8; ARGON2_SALT_LEN];
            archive_file
                .read_exact(&mut salt)
                .context("Failed to read salt from encrypted archive")?;

            // If we got here, the header format is valid
            // Full integrity testing would require a password, but basic format is OK
            return Ok(());
        }

        // This is a standard archive, proceed with full integrity testing
        archive_file
            .seek(SeekFrom::Start(0))
            .context("Failed to rewind archive file for standard processing")?;

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
