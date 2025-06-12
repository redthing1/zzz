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
    io::{BufReader, BufWriter},
    path::Path,
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use tar::{Archive, Builder, EntryType};
use walkdir::WalkDir;
use xz2::{read::XzDecoder, write::XzEncoder};

pub struct XzFormat;

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

        let output_file = File::create(output_path)
            .with_context(|| format!("Failed to create output file {}", output_path.display()))?;
        let buf_writer = BufWriter::new(output_file);

        // Map compression level (1-22) to xz level (0-9)
        let xz_level = (((options.level as f32 / 22.0) * 9.0) as u32).clamp(0, 9);
        let encoder = XzEncoder::new(buf_writer, xz_level);
        let mut tar_builder = Builder::new(encoder);

        // Configure tar builder for security
        tar_builder.mode(tar::HeaderMode::Deterministic);

        if let Some(progress) = progress {
            progress.set_length(input_size);
        }

        if input_path.is_file() {
            // Single file compression
            let file = File::open(input_path)
                .with_context(|| format!("Failed to open input file {}", input_path.display()))?;
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

            let filename = input_path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Could not determine filename from input path: {}",
                        input_path.display()
                    )
                })?;
            tar_builder.append_data(&mut header, filename, file)?;
        } else {
            // Directory compression
            let base_path = input_path.parent().unwrap_or(input_path);
            let mut entries: Vec<_> = WalkDir::new(input_path)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|entry| filter.should_include(entry.path()))
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
        }

        let encoder = tar_builder.into_inner()?;
        encoder.finish()?;

        let output_size = std::fs::metadata(output_path)?.len();
        Ok(CompressionStats::new(input_size, output_size))
    }

    fn extract(archive_path: &Path, output_dir: &Path, options: &ExtractionOptions) -> Result<()> {
        let file = File::open(archive_path)
            .with_context(|| format!("Failed to open archive file {}", archive_path.display()))?;
        let buf_reader = BufReader::new(file);
        let decoder = XzDecoder::new(buf_reader);
        let mut archive = Archive::new(decoder);

        std::fs::create_dir_all(output_dir)?;

        for entry in archive.entries()? {
            let mut entry = entry?;
            let path = entry.path()?;

            // Security: prevent path traversal
            if path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                continue;
            }

            let mut target_path = output_dir.to_path_buf();

            // Handle strip_components
            let components: Vec<_> = path.components().collect();
            if components.len() > options.strip_components {
                for component in components.iter().skip(options.strip_components) {
                    target_path.push(component);
                }
            } else {
                continue; // Skip if not enough components
            }

            // Check for overwrites
            if target_path.exists() && !options.overwrite {
                return Err(anyhow::anyhow!(
                    "output file '{}' already exists. Use --overwrite to replace.",
                    target_path.display()
                ));
            }

            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            entry.unpack(&target_path)?;
        }

        Ok(())
    }

    fn list(archive_path: &Path) -> Result<Vec<ArchiveEntry>> {
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
        use xz2::read::XzDecoder;
        use tar::Archive;

        let file = File::open(archive_path)?;
        if archive_path.extension().map_or(false, |ext| ext == "txz") ||
           archive_path.file_name().map_or(false, |name| name.to_string_lossy().ends_with(".tar.xz")) {
            let xz_decoder = XzDecoder::new(file);
            let mut archive = Archive::new(xz_decoder);
            for entry in archive.entries()? {
                let _entry = entry?;
            }
        } else { // Single .xz file
            let mut xz_decoder = XzDecoder::new(file);
            let mut buffer = Vec::new();
            xz_decoder.read_to_end(&mut buffer)?;
        }
        Ok(())
    }
}
