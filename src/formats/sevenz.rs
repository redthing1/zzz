//! 7-Zip format support

use crate::{
    filter::FileFilter,
    formats::{ArchiveEntry, CompressionFormat, CompressionOptions, CompressionStats, ExtractionOptions},
    progress::Progress,
    utils, Result,
};
use sevenz_rust::{Password, SevenZReader, SevenZWriter};
use std::{
    fs::File,
    path::Path,
};
use walkdir::WalkDir;

pub struct SevenZFormat;

impl CompressionFormat for SevenZFormat {
    fn compress(
        input_path: &Path,
        output_path: &Path,
        options: &CompressionOptions,
        filter: &FileFilter,
        progress: Option<&Progress>,
    ) -> Result<CompressionStats> {
        let input_size = if input_path.is_file() {
            std::fs::metadata(input_path)?.len()
        } else {
            utils::calculate_directory_size(input_path, filter)?
        };

        let mut sz = SevenZWriter::create(output_path)?;

        if let Some(progress) = progress {
            progress.set_length(input_size);
        }

        if input_path.is_file() {
            // Single file compression
            let filename = input_path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| anyhow::anyhow!("invalid filename"))?;
            sz.push_archive_entry(
                sevenz_rust::SevenZArchiveEntry::from_path(input_path, filename.to_string()),
                Some(File::open(input_path)?),
            )?;
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
                let path_str = relative_path.to_string_lossy().to_string();

                if path.is_file() {
                    sz.push_archive_entry(
                        sevenz_rust::SevenZArchiveEntry::from_path(path, path_str),
                        Some(File::open(path)?),
                    )?;

                    let metadata = entry.metadata()?;
                    processed_size += metadata.len();

                    if let Some(progress) = progress {
                        progress.set_position(processed_size);
                    }
                } else if path.is_dir() {
                    // Add directory entry
                    let mut entry = sevenz_rust::SevenZArchiveEntry::new();
                    entry.name = path_str;
                    entry.is_directory = true;
                    sz.push_archive_entry(entry, None::<std::io::Empty>)?;
                }
            }
        }

        sz.finish()?;

        let output_size = std::fs::metadata(output_path)?.len();
        Ok(CompressionStats::new(input_size, output_size))
    }

    fn extract(archive_path: &Path, output_dir: &Path, options: &ExtractionOptions) -> Result<()> {
        let mut sz = SevenZReader::open(archive_path, Password::empty())?;

        std::fs::create_dir_all(output_dir)?;

        sz.for_each_entries(|entry, reader| {
            let file_path = std::path::Path::new(&entry.name);

            // Security: prevent path traversal
            if file_path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
                return Ok(true);
            }

            let mut target_path = output_dir.to_path_buf();
            
            // Handle strip_components
            let components: Vec<_> = file_path.components().collect();
            if components.len() > options.strip_components {
                for component in components.iter().skip(options.strip_components) {
                    target_path.push(component);
                }
            } else {
                return Ok(true); // Skip if not enough components
            }

            // Check for overwrites
            if target_path.exists() && !options.overwrite {
                return Ok(true);
            }

            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            if entry.is_directory() {
                std::fs::create_dir_all(&target_path)?;
            } else {
                let mut output_file = File::create(&target_path)?;
                std::io::copy(reader, &mut output_file)?;
            }

            Ok(true)
        })?;

        Ok(())
    }

    fn list(archive_path: &Path) -> Result<Vec<ArchiveEntry>> {
        let sz = SevenZReader::open(archive_path, Password::empty())?;
        let archive = sz.archive();

        let mut entries = Vec::new();

        for file in &archive.files {
            let path = file.name.clone();
            let size = file.size;
            let is_file = !file.is_directory();

            entries.push(ArchiveEntry { path, size, is_file });
        }

        Ok(entries)
    }

    fn extension() -> &'static str {
        "7z"
    }
}