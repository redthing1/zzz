//! 7-Zip format support

use crate::{
    filter::FileFilter,
    formats::{
        ArchiveEntry, CompressionFormat, CompressionOptions, CompressionStats, ExtractionOptions,
    },
    progress::Progress,
    utils, Result,
};
use anyhow::Context;
use sevenz_rust::{Password, SevenZArchiveEntry, SevenZReader, SevenZWriter};
use std::{fs::File, path::Path};

pub struct SevenZFormat;

impl CompressionFormat for SevenZFormat {
    fn compress(
        input_path: &Path,
        output_path: &Path,
        options: &CompressionOptions,
        filter: &FileFilter,
        progress: Option<&Progress>,
    ) -> Result<CompressionStats> {
        let input_size = utils::calculate_directory_size(input_path, filter)?;

        let mut sz = SevenZWriter::create(output_path).with_context(|| {
            format!(
                "Failed to create 7-Zip writer for {}",
                output_path.display()
            )
        })?;

        // Set password encryption if provided
        if let Some(password) = &options.password {
            use sevenz_rust::{AesEncoderOptions, SevenZMethod};
            sz.set_content_methods(vec![
                AesEncoderOptions::new(Password::from(password.as_str())).into(),
                SevenZMethod::LZMA2.into(),
            ]);
        }

        if let Some(progress) = progress {
            progress.set_length(input_size);
        }

        if input_path.is_file() {
            // Single file compression
            let filename_os = input_path.file_name().ok_or_else(|| {
                anyhow::anyhow!(
                    "Could not determine filename from input path: {}",
                    input_path.display()
                )
            })?;
            if !filter.should_include_relative(Path::new(filename_os)) {
                sz.finish().with_context(|| {
                    format!("Failed to finalize 7-Zip archive {}", output_path.display())
                })?;
                let output_size = std::fs::metadata(output_path)
                    .with_context(|| {
                        format!(
                            "Failed to read metadata for output file {}",
                            output_path.display()
                        )
                    })?
                    .len();
                return Ok(CompressionStats::new(input_size, output_size));
            }

            let filename = filename_os.to_str().ok_or_else(|| {
                anyhow::anyhow!(
                    "Could not determine filename from input path: {}",
                    input_path.display()
                )
            })?;

            let entry = SevenZArchiveEntry::from_path(input_path, filename.to_string());
            sz.push_archive_entry(
                entry,
                Some(File::open(input_path).with_context(|| {
                    format!("Failed to open input file {}", input_path.display())
                })?),
            )?;
        } else {
            // Directory compression - preserve directory structure like our other formats
            let base_path = input_path.parent().unwrap_or(input_path);
            let mut entries: Vec<_> = filter
                .walk_entries(input_path)
                .filter_map(|entry| entry.ok())
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
                    let archive_entry = SevenZArchiveEntry::from_path(path, path_str);
                    sz.push_archive_entry(
                        archive_entry,
                        Some(File::open(path).with_context(|| {
                            format!("Failed to open file for archiving {}", path.display())
                        })?),
                    )?;

                    let metadata = entry.metadata().with_context(|| {
                        format!("Failed to read metadata for {}", path.display())
                    })?;
                    processed_size += metadata.len();

                    if let Some(progress) = progress {
                        progress.set_position(processed_size);
                    }
                } else if path.is_dir() {
                    // Add directory entry
                    let mut archive_entry = SevenZArchiveEntry::new();
                    archive_entry.name = path_str;
                    archive_entry.is_directory = true;
                    sz.push_archive_entry(archive_entry, None::<std::io::Empty>)?;
                }
            }
        }

        sz.finish().with_context(|| {
            format!("Failed to finalize 7-Zip archive {}", output_path.display())
        })?;

        let output_size = std::fs::metadata(output_path)
            .with_context(|| {
                format!(
                    "Failed to read metadata for output file {}",
                    output_path.display()
                )
            })?
            .len();
        Ok(CompressionStats::new(input_size, output_size))
    }

    fn extract(
        archive_path: &Path,
        output_dir: &Path,
        options: &ExtractionOptions,
        progress: Option<&crate::progress::Progress>,
    ) -> Result<()> {
        let password = options
            .password
            .as_ref()
            .map_or(Password::empty(), |p| Password::from(p.as_str()));
        let mut sz = SevenZReader::open(archive_path, password).map_err(|e| {
            // Check if this looks like a password-related error
            let error_msg = format!("{e}");
            if error_msg.contains("MaybeBadPassword")
                || (options.password.is_some()
                    && (error_msg.contains("password")
                        || error_msg.contains("decrypt")
                        || error_msg.contains("encrypted")))
            {
                anyhow::anyhow!("Failed to decrypt archive (invalid password)")
            } else if error_msg.contains("PasswordRequired")
                || (options.password.is_none()
                    && (error_msg.contains("password")
                        || error_msg.contains("AES")
                        || error_msg.contains("encrypted")))
            {
                anyhow::anyhow!("Archive is password protected but no password was provided")
            } else {
                anyhow::anyhow!(
                    "Failed to open 7-Zip archive {}: {}",
                    archive_path.display(),
                    e
                )
            }
        })?;

        std::fs::create_dir_all(output_dir).with_context(|| {
            format!("Failed to create output directory {}", output_dir.display())
        })?;

        // Get entry count for progress
        let entry_count = sz.archive().files.len();
        if let Some(progress) = progress {
            progress.set_length(entry_count as u64);
        }

        let mut processed_count = 0;
        sz.for_each_entries(|entry, reader| {
            let file_path = std::path::Path::new(&entry.name);
            let target_path = crate::utils::extract_entry_to_path(
                output_dir,
                file_path,
                options.strip_components,
                options.overwrite,
                entry.is_directory(),
            )
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
            let Some(target_path) = target_path else {
                return Ok(true);
            };

            // Show verbose output for individual files
            if let Some(progress) = progress {
                if progress.is_verbose() {
                    if entry.is_directory() {
                        println!("  creating: {}", file_path.display());
                    } else {
                        println!("  extracting: {}", file_path.display());
                    }
                }
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

            // Update progress
            processed_count += 1;
            if let Some(progress) = progress {
                progress.set_position(processed_count);
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

            entries.push(ArchiveEntry {
                path,
                size,
                is_file,
            });
        }

        Ok(entries)
    }

    fn extension() -> &'static str {
        "7z"
    }

    fn test_integrity(archive_path: &Path) -> Result<()> {
        // The sevenz-rust crate has a way to iterate and test entries.
        // For now, we'll just try to open and list entries.
        let mut sz = sevenz_rust::SevenZReader::open(archive_path, sevenz_rust::Password::empty())
            .map_err(|e| {
                // Provide consistent error messages for password-protected archives
                let error_msg = format!("{e}");
                if error_msg.contains("PasswordRequired")
                    || error_msg.contains("password")
                    || error_msg.contains("encrypted")
                {
                    anyhow::anyhow!(
                        "Failed to open 7-Zip archive {}: archive is password protected",
                        archive_path.display()
                    )
                } else {
                    anyhow::anyhow!(
                        "Failed to open 7-Zip archive {}: {}",
                        archive_path.display(),
                        e
                    )
                }
            })?;
        sz.for_each_entries(|_entry, _reader| Ok(true))?; // This iterates and reads entry headers
        Ok(())
    }
}
