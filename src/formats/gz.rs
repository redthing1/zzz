//! Gzip format support (tar.gz/tgz)

use crate::{
    filter::FileFilter,
    formats::{
        tarball, ArchiveEntry, CompressionFormat, CompressionOptions, CompressionStats,
        ExtractionOptions,
    },
    progress::Progress,
    utils, Result,
};
use anyhow::Context;
use flate2::{read::GzDecoder, write::GzEncoder, Compression, GzBuilder};
use std::{
    fs::File,
    io::{BufReader, BufWriter, Read},
    path::Path,
};

pub struct GzipFormat;

fn file_name_lower(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn is_targz(path: &Path) -> bool {
    let name = file_name_lower(path);
    name.ends_with(".tar.gz") || name.ends_with(".tgz")
}

fn is_raw_gz(path: &Path) -> bool {
    let name = file_name_lower(path);
    name.ends_with(".gz") && !is_targz(path)
}

fn raw_output_name(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy();
    let lower = name.to_lowercase();
    if lower.ends_with(".gz") {
        let new_len = name.len().saturating_sub(3);
        return Some(name[..new_len].to_string());
    }
    None
}

fn gzip_mtime(path: &Path, options: &CompressionOptions) -> u32 {
    if options.strip_timestamps {
        return 0;
    }

    let Ok(metadata) = std::fs::metadata(path) else {
        return 0;
    };
    let Ok(modified) = metadata.modified() else {
        return 0;
    };
    let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) else {
        return 0;
    };

    duration.as_secs().min(u64::from(u32::MAX)) as u32
}

impl CompressionFormat for GzipFormat {
    fn compress(
        input_path: &Path,
        output_path: &Path,
        options: &CompressionOptions,
        filter: &FileFilter,
        progress: Option<&Progress>,
    ) -> Result<CompressionStats> {
        let input_size = utils::calculate_directory_size(
            input_path,
            filter,
            options.follow_symlinks,
            options.allow_symlink_escape,
        )?;

        // Map compression level (1-22) to gzip level (0-9)
        let gzip_level = (((options.level as f32 / 22.0) * 9.0) as u32).clamp(0, 9);

        if let Some(progress) = progress {
            progress.set_length(input_size);
        }

        // Password protection is not supported for Gzip format
        if options.password.is_some() {
            return Err(anyhow::anyhow!(
                "Password protection is not supported for Gzip format. Use 7z format for password protection."
            ));
        }

        if input_path.is_file() {
            if is_raw_gz(output_path) {
                let filename = input_path.file_name();
                if let Some(filename) = filename {
                    if !filter.should_include_relative(Path::new(filename)) {
                        let output_file = File::create(output_path).with_context(|| {
                            format!("Failed to create output file {}", output_path.display())
                        })?;
                        let buf_writer = BufWriter::new(output_file);
                        let encoder = GzBuilder::new()
                            .mtime(0)
                            .write(buf_writer, Compression::new(gzip_level));
                        encoder.finish()?;
                        let output_size = std::fs::metadata(output_path)?.len();
                        return Ok(CompressionStats::new(input_size, output_size));
                    }
                }

                let mtime = gzip_mtime(input_path, options);
                let output_file = File::create(output_path).with_context(|| {
                    format!("Failed to create output file {}", output_path.display())
                })?;
                let buf_writer = BufWriter::new(output_file);
                let mut encoder = GzBuilder::new()
                    .mtime(mtime)
                    .write(buf_writer, Compression::new(gzip_level));

                let mut input_file = File::open(input_path).with_context(|| {
                    format!("Failed to open input file {}", input_path.display())
                })?;
                std::io::copy(&mut input_file, &mut encoder)?;
                encoder.finish()?;
            } else {
                let output_file = File::create(output_path).with_context(|| {
                    format!("Failed to create output file {}", output_path.display())
                })?;
                let buf_writer = BufWriter::new(output_file);
                let encoder = GzEncoder::new(buf_writer, Compression::new(gzip_level));
                let encoder = tarball::build_tarball(
                    encoder,
                    input_path,
                    options,
                    filter,
                    progress,
                    tarball::BuildOptions {
                        normalize_ownership: options.normalize_ownership,
                        apply_filter_to_single_file: true,
                        directory_slash: false,
                        set_mtime_for_single_file: true,
                    },
                )?;
                encoder.finish()?;
            }
        } else {
            if is_raw_gz(output_path) {
                return Err(anyhow::anyhow!(
                    "Directory input requires a .tgz or .tar.gz output"
                ));
            }

            let output_file = File::create(output_path).with_context(|| {
                format!("Failed to create output file {}", output_path.display())
            })?;
            let buf_writer = BufWriter::new(output_file);
            let encoder = GzEncoder::new(buf_writer, Compression::new(gzip_level));
            let encoder = tarball::build_tarball(
                encoder,
                input_path,
                options,
                filter,
                progress,
                tarball::BuildOptions {
                    normalize_ownership: options.normalize_ownership,
                    apply_filter_to_single_file: true,
                    directory_slash: false,
                    set_mtime_for_single_file: true,
                },
            )?;
            encoder.finish()?;
        }

        let output_size = std::fs::metadata(output_path)?.len();
        Ok(CompressionStats::new(input_size, output_size))
    }

    fn extract(
        archive_path: &Path,
        output_dir: &Path,
        options: &ExtractionOptions,
        progress: Option<&crate::progress::Progress>,
    ) -> Result<()> {
        if is_raw_gz(archive_path) {
            let output_name = raw_output_name(archive_path)
                .ok_or_else(|| anyhow::anyhow!("Failed to determine output filename"))?;
            let target_path = match crate::utils::prepare_extract_target(
                output_dir,
                Path::new(&output_name),
                options.strip_components,
                options.overwrite,
                false,
            )? {
                crate::utils::ExtractTarget::Target(target_path) => target_path,
                crate::utils::ExtractTarget::SkipStrip => return Ok(()),
                crate::utils::ExtractTarget::SkipExisting(target_path) => {
                    return Err(anyhow::anyhow!(
                        "output file '{}' already exists. Use --overwrite to replace.",
                        target_path.display()
                    ));
                }
            };

            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            let file = File::open(archive_path).with_context(|| {
                format!("Failed to open archive file {}", archive_path.display())
            })?;
            let mut decoder = GzDecoder::new(file);
            let mtime = if options.strip_timestamps {
                None
            } else {
                decoder
                    .header()
                    .and_then(|header| header.mtime_as_datetime())
            };
            let mut output_file = File::create(&target_path)?;
            std::io::copy(&mut decoder, &mut output_file)?;
            drop(output_file);
            if let Some(mtime) = mtime {
                utils::apply_mtime(&target_path, mtime)?;
            }
            return Ok(());
        }

        let file = File::open(archive_path)
            .with_context(|| format!("Failed to open archive file {}", archive_path.display()))?;
        let buf_reader = BufReader::new(file);
        let decoder = GzDecoder::new(buf_reader);

        tarball::extract_tarball(decoder, output_dir, options, progress)
    }

    fn list(archive_path: &Path) -> Result<Vec<ArchiveEntry>> {
        if is_raw_gz(archive_path) {
            let output_name = raw_output_name(archive_path)
                .ok_or_else(|| anyhow::anyhow!("Failed to determine output filename"))?;
            let file = File::open(archive_path)?;
            let mut decoder = GzDecoder::new(file);
            let mut size = 0u64;
            let mut buffer = [0u8; 8192];
            loop {
                let read = decoder.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                size += read as u64;
            }
            return Ok(vec![ArchiveEntry {
                path: output_name,
                size,
                is_file: true,
            }]);
        }

        let file = File::open(archive_path).with_context(|| {
            format!(
                "Failed to open archive for listing {}",
                archive_path.display()
            )
        })?;
        let buf_reader = BufReader::new(file);
        let decoder = GzDecoder::new(buf_reader);

        tarball::list_tarball(decoder)
    }

    fn extension() -> &'static str {
        "tgz"
    }

    fn test_integrity(archive_path: &Path) -> Result<()> {
        // For Gzip, try to read the whole stream to check for corruption.
        // If it's a .tgz (tar.gz), list contents using the tar crate.
        use flate2::read::GzDecoder;
        use std::fs::File;
        use std::io::Read;
        use tar::Archive;

        let file = File::open(archive_path)?;
        if is_targz(archive_path) {
            let gz_decoder = GzDecoder::new(file);
            let mut archive = Archive::new(gz_decoder);
            for entry in archive.entries()? {
                let _entry = entry?; // Access entry to check for errors
            }
        } else {
            // Single .gz file
            let mut gz_decoder = GzDecoder::new(file);
            let mut buffer = Vec::new();
            gz_decoder.read_to_end(&mut buffer)?;
        }
        Ok(())
    }
}
