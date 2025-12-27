//! XZ format support (tar.xz/txz)

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
use std::{
    fs::File,
    io::{BufReader, BufWriter, Read},
    path::Path,
};

use xz2::{read::XzDecoder, write::XzEncoder};

pub struct XzFormat;

fn file_name_lower(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn is_tarxz(path: &Path) -> bool {
    let name = file_name_lower(path);
    name.ends_with(".tar.xz") || name.ends_with(".txz")
}

fn is_raw_xz(path: &Path) -> bool {
    let name = file_name_lower(path);
    name.ends_with(".xz") && !is_tarxz(path)
}

fn raw_output_name(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy();
    let lower = name.to_lowercase();
    if lower.ends_with(".xz") {
        let new_len = name.len().saturating_sub(3);
        return Some(name[..new_len].to_string());
    }
    None
}

impl CompressionFormat for XzFormat {
    fn compress(
        input_path: &Path,
        output_path: &Path,
        options: &CompressionOptions,
        filter: &FileFilter,
        progress: Option<&Progress>,
    ) -> Result<CompressionStats> {
        let input_size = utils::calculate_directory_size(input_path, filter)?;

        // Map compression level (1-22) to xz level (0-9)
        let xz_level = (((options.level as f32 / 22.0) * 9.0) as u32).clamp(0, 9);

        if let Some(progress) = progress {
            progress.set_length(input_size);
        }

        if input_path.is_file() {
            if is_raw_xz(output_path) {
                let filename = input_path.file_name();
                if let Some(filename) = filename {
                    if !filter.should_include_relative(Path::new(filename)) {
                        let output_file = File::create(output_path).with_context(|| {
                            format!("Failed to create output file {}", output_path.display())
                        })?;
                        let buf_writer = BufWriter::new(output_file);
                        let encoder = XzEncoder::new(buf_writer, xz_level);
                        encoder.finish()?;
                        let output_size = std::fs::metadata(output_path)?.len();
                        return Ok(CompressionStats::new(input_size, output_size));
                    }
                }

                let output_file = File::create(output_path).with_context(|| {
                    format!("Failed to create output file {}", output_path.display())
                })?;
                let buf_writer = BufWriter::new(output_file);
                let mut encoder = XzEncoder::new(buf_writer, xz_level);

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
                let encoder = XzEncoder::new(buf_writer, xz_level);
                let encoder = tarball::build_tarball(
                    encoder,
                    input_path,
                    options,
                    filter,
                    progress,
                    tarball::BuildOptions {
                        normalize_ownership: options.normalize_permissions,
                        apply_filter_to_single_file: true,
                        directory_slash: false,
                        set_mtime_for_single_file: true,
                    },
                )?;
                encoder.finish()?;
            }
        } else {
            if is_raw_xz(output_path) {
                return Err(anyhow::anyhow!(
                    "Directory input requires a .txz or .tar.xz output"
                ));
            }

            let output_file = File::create(output_path).with_context(|| {
                format!("Failed to create output file {}", output_path.display())
            })?;
            let buf_writer = BufWriter::new(output_file);
            let encoder = XzEncoder::new(buf_writer, xz_level);
            let encoder = tarball::build_tarball(
                encoder,
                input_path,
                options,
                filter,
                progress,
                tarball::BuildOptions {
                    normalize_ownership: options.normalize_permissions,
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
        if is_raw_xz(archive_path) {
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
            let mut decoder = XzDecoder::new(file);
            let mut output_file = File::create(&target_path)?;
            std::io::copy(&mut decoder, &mut output_file)?;
            return Ok(());
        }

        let file = File::open(archive_path)
            .with_context(|| format!("Failed to open archive file {}", archive_path.display()))?;
        let buf_reader = BufReader::new(file);
        let decoder = XzDecoder::new(buf_reader);

        tarball::extract_tarball(decoder, output_dir, options, progress)
    }

    fn list(archive_path: &Path) -> Result<Vec<ArchiveEntry>> {
        if is_raw_xz(archive_path) {
            let output_name = raw_output_name(archive_path)
                .ok_or_else(|| anyhow::anyhow!("Failed to determine output filename"))?;
            let file = File::open(archive_path)?;
            let mut decoder = XzDecoder::new(file);
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
        let decoder = XzDecoder::new(buf_reader);

        tarball::list_tarball(decoder)
    }

    fn extension() -> &'static str {
        "txz"
    }

    fn test_integrity(archive_path: &Path) -> Result<()> {
        // Similar to Gzip, for .txz (tar.xz), list contents.
        // For a raw .xz stream, try to decompress fully.
        use std::fs::File;
        use std::io::Read;
        use tar::Archive;
        use xz2::read::XzDecoder;

        let file = File::open(archive_path)?;
        if is_tarxz(archive_path) {
            let xz_decoder = XzDecoder::new(file);
            let mut archive = Archive::new(xz_decoder);
            for entry in archive.entries()? {
                let _entry = entry?;
            }
        } else {
            // Single .xz file
            let mut xz_decoder = XzDecoder::new(file);
            let mut buffer = Vec::new();
            xz_decoder.read_to_end(&mut buffer)?;
        }
        Ok(())
    }
}
