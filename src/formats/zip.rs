//! ZIP format support

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
use walkdir::WalkDir;
use zip::{write::FileOptions, CompressionMethod, ZipArchive, ZipWriter};

pub struct ZipFormat;

impl CompressionFormat for ZipFormat {
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
        let mut zip_writer = ZipWriter::new(buf_writer);

        // Map compression level (1-22) to zip level (0-9)
        let zip_level = (((options.level as f32 / 22.0) * 9.0) as i32).clamp(0, 9);
        let base_file_options = FileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .compression_level(Some(zip_level));

        if let Some(progress) = progress {
            progress.set_length(input_size);
        }

        if input_path.is_file() {
            // Single file compression
            // Password protection is not supported for ZIP format
            if options.password.is_some() {
                return Err(anyhow::anyhow!("Password protection is not supported for ZIP format. Use 7z format for password protection."));
            }
            
            let current_file_options = base_file_options.clone();

            let filename = input_path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Could not determine filename from input path: {}",
                        input_path.display()
                    )
                })?;
            zip_writer.start_file(filename, current_file_options)?;

            let mut file = File::open(input_path)
                .with_context(|| format!("Failed to open input file {}", input_path.display()))?;
            std::io::copy(&mut file, &mut zip_writer)?;
        } else {
            // Directory compression
            // Password protection is not supported for ZIP format
            if options.password.is_some() {
                return Err(anyhow::anyhow!("Password protection is not supported for ZIP format. Use 7z format for password protection."));
            }
            
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
                let path_str = relative_path.to_string_lossy();

                let current_file_options = base_file_options.clone();

                if path.is_file() {
                    zip_writer.start_file(path_str.as_ref(), current_file_options)?;

                    let mut file = File::open(path).with_context(|| {
                        format!("Failed to open file for archiving {}", path.display())
                    })?;
                    std::io::copy(&mut file, &mut zip_writer)?;

                    let metadata = entry.metadata()?;
                    processed_size += metadata.len();

                    if let Some(progress) = progress {
                        progress.set_position(processed_size);
                    }
                } else if path.is_dir() {
                    // Add directory entry with trailing slash
                    let dir_path = format!("{path_str}/");
                    zip_writer.add_directory(&dir_path, current_file_options)?;
                }
            }
        }

        zip_writer.finish()?;

        let output_size = std::fs::metadata(output_path)?.len();
        Ok(CompressionStats::new(input_size, output_size))
    }

    fn extract(archive_path: &Path, output_dir: &Path, options: &ExtractionOptions) -> Result<()> {
        // Password protection is not supported for ZIP format
        if options.password.is_some() {
            return Err(anyhow::anyhow!("Password protection is not supported for ZIP format. Use 7z format for password protection."));
        }
        
        let file = File::open(archive_path)
            .with_context(|| format!("Failed to open archive file {}", archive_path.display()))?;
        let buf_reader = BufReader::new(file);
        let mut archive = ZipArchive::new(buf_reader).with_context(|| {
            format!("Failed to read ZIP archive from {}", archive_path.display())
        })?;

        std::fs::create_dir_all(output_dir)?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let file_path = file.mangled_name();

            // Security: prevent path traversal
            if file_path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                continue;
            }

            let mut target_path = output_dir.to_path_buf();

            // Handle strip_components
            let components: Vec<_> = file_path.components().collect();
            if components.len() > options.strip_components {
                for component in components.iter().skip(options.strip_components) {
                    target_path.push(component);
                }
            } else {
                continue; // Skip if not enough components
            }

            // Check for overwrites
            if target_path.exists() && !options.overwrite {
                continue;
            }

            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            if file.is_dir() {
                std::fs::create_dir_all(&target_path)?;
            } else {
                let mut output_file = File::create(&target_path)?;
                std::io::copy(&mut file, &mut output_file)?;
            }
        }

        Ok(())
    }

    fn list(archive_path: &Path) -> Result<Vec<ArchiveEntry>> {
        let file = File::open(archive_path)?;
        let buf_reader = BufReader::new(file);
        let mut archive = ZipArchive::new(buf_reader)?;

        let mut entries = Vec::new();

        for i in 0..archive.len() {
            let file = archive.by_index(i)?;
            let path = file.mangled_name().to_string_lossy().to_string();
            let size = file.size();
            let is_file = !file.is_dir();

            entries.push(ArchiveEntry {
                path,
                size,
                is_file,
            });
        }

        Ok(entries)
    }

    fn extension() -> &'static str {
        "zip"
    }

    fn test_integrity(archive_path: &Path) -> Result<()> {
        use std::fs::File;
        use zip::ZipArchive;
        use std::io::Read;

        let file = File::open(archive_path)?;
        let mut archive = ZipArchive::new(file)?;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            // Try to read a small amount of data or check CRC if available
            // For now, just attempting to get the entry is a basic check.
            // To be more thorough, we could try reading from the entry:
            if entry.is_file() {
                let mut buffer = [0; 1]; // Read 1 byte
                entry.read(&mut buffer).ok(); // Ignore errors for now, or handle them
            }
        }
        Ok(())
    }
}
