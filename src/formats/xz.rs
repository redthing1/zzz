//! XZ format support (tar.xz/txz)

use crate::{
    filter::FileFilter,
    formats::{
        ArchiveEntry, CompressionFormat, CompressionOptions, CompressionStats, ExtractionOptions,
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

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use tar::{Archive, Builder, EntryType};
use walkdir::WalkDir;
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
        let input_size = if input_path.is_file() {
            std::fs::metadata(input_path)
                .with_context(|| {
                    format!("Failed to read metadata for input {}", input_path.display())
                })?
                .len()
        } else {
            utils::calculate_directory_size(input_path, filter)?
        };

        // Map compression level (1-22) to xz level (0-9)
        let xz_level = (((options.level as f32 / 22.0) * 9.0) as u32).clamp(0, 9);

        if let Some(progress) = progress {
            progress.set_length(input_size);
        }

        if input_path.is_file() {
            if is_raw_xz(output_path) {
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
                // Single file compression as tarball
                let output_file = File::create(output_path).with_context(|| {
                    format!("Failed to create output file {}", output_path.display())
                })?;
                let buf_writer = BufWriter::new(output_file);
                let encoder = XzEncoder::new(buf_writer, xz_level);
                let mut tar_builder = Builder::new(encoder);

                // Configure tar builder for security
                tar_builder.mode(tar::HeaderMode::Deterministic);

                let file = File::open(input_path).with_context(|| {
                    format!("Failed to open input file {}", input_path.display())
                })?;
                let mut header = tar::Header::new_gnu();
                header.set_size(
                    std::fs::metadata(input_path)
                        .with_context(|| {
                            format!(
                                "Failed to read metadata for input file {}",
                                input_path.display()
                            )
                        })?
                        .len(),
                );
                header.set_mode(0o644);
                header.set_cksum();

                let filename =
                    input_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "Could not determine filename from input path: {}",
                                input_path.display()
                            )
                        })?;
                tar_builder.append_data(&mut header, filename, file)?;

                let encoder = tar_builder.into_inner()?;
                encoder.finish()?;
            }
        } else {
            // Directory compression
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
            let mut tar_builder = Builder::new(encoder);

            // Configure tar builder for security
            tar_builder.mode(tar::HeaderMode::Deterministic);

            let base_path = input_path.parent().unwrap_or(input_path);
            let mut entries: Vec<_> = WalkDir::new(input_path)
                .follow_links(false)
                .into_iter()
                .filter_entry(|entry| filter.should_include_path(input_path, entry.path()))
                .filter_map(|e| e.ok())
                .collect();

            // Sort for deterministic archives
            if options.deterministic {
                entries.sort_by(|a, b| a.path().cmp(b.path()));
            }

            let mut processed_size = 0u64;

            for entry in entries {
                let path = entry.path();
                let relative_path = path.strip_prefix(base_path)?;

                if path.is_file() {
                    let file = File::open(path).with_context(|| {
                        format!("Failed to open file for archiving {}", path.display())
                    })?;
                    let metadata = entry.metadata()?;
                    let mut header = tar::Header::new_gnu();

                    header.set_size(metadata.len());
                    header.set_mode(if options.normalize_permissions {
                        0o644
                    } else {
                        #[cfg(unix)]
                        {
                            metadata.permissions().mode()
                        }
                        #[cfg(not(unix))]
                        {
                            0o644
                        }
                    });
                    header.set_mtime(
                        metadata
                            .modified()?
                            .duration_since(std::time::UNIX_EPOCH)?
                            .as_secs(),
                    );
                    header.set_cksum();

                    tar_builder.append_data(&mut header, relative_path, file)?;
                    processed_size += metadata.len();

                    if let Some(progress) = progress {
                        progress.set_position(processed_size);
                    }
                } else if path.is_dir() {
                    let metadata = entry.metadata()?;
                    let mut header = tar::Header::new_gnu();

                    header.set_size(0);
                    header.set_mode(if options.normalize_permissions {
                        0o755
                    } else {
                        #[cfg(unix)]
                        {
                            metadata.permissions().mode()
                        }
                        #[cfg(not(unix))]
                        {
                            0o755
                        }
                    });
                    header.set_entry_type(EntryType::Directory);
                    header.set_mtime(
                        metadata
                            .modified()?
                            .duration_since(std::time::UNIX_EPOCH)?
                            .as_secs(),
                    );
                    header.set_cksum();

                    tar_builder.append_data(&mut header, relative_path, std::io::empty())?;
                }
            }

            let encoder = tar_builder.into_inner()?;
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
            let relative_path = crate::utils::sanitize_archive_entry_path(
                Path::new(&output_name),
                options.strip_components,
            )?;
            let Some(relative_path) = relative_path else {
                return Ok(());
            };
            let target_path = output_dir.join(&relative_path);

            crate::utils::ensure_no_symlink_ancestors(output_dir, &target_path)?;

            if target_path.exists() && !options.overwrite {
                return Err(anyhow::anyhow!(
                    "output file '{}' already exists. Use --overwrite to replace.",
                    target_path.display()
                ));
            }

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
        let mut archive = Archive::new(decoder);

        std::fs::create_dir_all(output_dir)?;

        let mut entry_count = 0u64;
        for entry in archive.entries()? {
            let mut entry = entry?;
            let path = entry.path()?;
            let relative_path =
                crate::utils::sanitize_archive_entry_path(&path, options.strip_components)?;
            let Some(relative_path) = relative_path else {
                continue;
            };
            let target_path = output_dir.join(&relative_path);

            crate::utils::ensure_no_symlink_ancestors(output_dir, &target_path)?;

            // Check for overwrites
            if target_path.exists() && !options.overwrite {
                return Err(anyhow::anyhow!(
                    "output file '{}' already exists. Use --overwrite to replace.",
                    target_path.display()
                ));
            }

            // Show verbose output for individual files
            if let Some(progress) = progress {
                if progress.is_verbose() {
                    if entry.header().entry_type().is_dir() {
                        println!("  creating: {}", path.display());
                    } else {
                        println!("  extracting: {}", path.display());
                    }
                }
            }

            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            entry.unpack(&target_path)?;

            // Update progress
            entry_count += 1;
            if let Some(progress) = progress {
                progress.set_position(entry_count);
            }
        }

        Ok(())
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
        let mut archive = Archive::new(decoder);

        let mut entries = Vec::new();

        for entry in archive.entries()? {
            let entry = entry?;
            let path = entry.path()?.to_string_lossy().to_string();
            let size = entry.header().size()?;
            let is_file = entry.header().entry_type().is_file();

            entries.push(ArchiveEntry {
                path,
                size,
                is_file,
            });
        }

        Ok(entries)
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
