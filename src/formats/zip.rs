//! ZIP format support

use crate::{
    archive_plan::{ArchivePlan, PlannedEntryKind},
    formats::{
        ArchiveEntry, CompressionFormat, CompressionOptions, CompressionStats, ExtractionOptions,
    },
    progress::Progress,
    utils, Result,
};
use anyhow::Context;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    fs::File,
    io::{BufReader, BufWriter},
    path::Path,
    time::SystemTime,
};
use time::OffsetDateTime;
use zip::{write::FileOptions, CompressionMethod, ZipArchive, ZipWriter};

pub struct ZipFormat;

fn zip_last_modified(metadata: &std::fs::Metadata, preserve_timestamps: bool) -> zip::DateTime {
    if !preserve_timestamps {
        return zip::DateTime::default();
    }

    let Ok(modified) = metadata.modified() else {
        return zip::DateTime::default();
    };
    let dt = OffsetDateTime::from(modified);
    zip::DateTime::try_from(dt).unwrap_or_default()
}

impl CompressionFormat for ZipFormat {
    fn compress_plan(
        plan: &ArchivePlan,
        output_path: &Path,
        options: &CompressionOptions,
        progress: Option<&Progress>,
    ) -> Result<CompressionStats> {
        let output_file = File::create(output_path)
            .with_context(|| format!("Failed to create output file {}", output_path.display()))?;
        let buf_writer = BufWriter::new(output_file);
        let mut zip_writer = ZipWriter::new(buf_writer);

        // Map compression level (1-22) to zip level (0-9)
        let zip_level = (((options.level as f32 / 22.0) * 9.0) as i64).clamp(0, 9);
        let base_file_options = FileOptions::<()>::default()
            .compression_method(CompressionMethod::Deflated)
            .compression_level(Some(zip_level));

        if let Some(progress) = progress {
            progress.set_length(plan.total_size);
        }

        // Password protection is not supported for ZIP format
        if options.password.is_some() {
            return Err(anyhow::anyhow!("Password protection is not supported for ZIP format. Use 7z format for password protection."));
        }

        let mut processed_size = 0u64;

        for entry in &plan.entries {
            let metadata = std::fs::metadata(&entry.disk_path).with_context(|| {
                format!("Failed to read metadata for {}", entry.disk_path.display())
            })?;
            let zip_time = zip_last_modified(&metadata, options.preserve_timestamps);
            let default_mode = if entry.is_dir() { 0o755 } else { 0o644 };
            let permissions = if options.preserve_permissions {
                #[cfg(unix)]
                {
                    metadata.permissions().mode()
                }
                #[cfg(not(unix))]
                {
                    default_mode
                }
            } else {
                default_mode
            };
            let current_file_options = base_file_options
                .last_modified_time(zip_time)
                .unix_permissions(permissions);
            let path_str = utils::normalize_archive_path(&entry.archive_path);

            match entry.kind {
                PlannedEntryKind::File => {
                    zip_writer.start_file(path_str.as_str(), current_file_options)?;

                    let mut file = File::open(&entry.disk_path).with_context(|| {
                        format!(
                            "Failed to open file for archiving {}",
                            entry.disk_path.display()
                        )
                    })?;
                    std::io::copy(&mut file, &mut zip_writer)?;

                    processed_size += metadata.len();

                    if let Some(progress) = progress {
                        progress.set_position(processed_size);
                    }
                }
                PlannedEntryKind::Directory => {
                    // Add directory entry with trailing slash
                    let dir_path = format!("{path_str}/");
                    zip_writer.add_directory(&dir_path, current_file_options)?;
                }
            }
        }

        zip_writer.finish()?;

        let output_size = std::fs::metadata(output_path)?.len();
        Ok(CompressionStats::new(plan.total_size, output_size))
    }

    fn extract(
        archive_path: &Path,
        output_dir: &Path,
        options: &ExtractionOptions,
        progress: Option<&crate::progress::Progress>,
    ) -> Result<()> {
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

        let total_files = archive.len();
        if let Some(progress) = progress {
            progress.set_length(total_files as u64);
        }

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let file_path = std::path::Path::new(file.name());
            let Some(target_path) = crate::utils::extract_entry_to_path(
                output_dir,
                file_path,
                options.strip_components,
                options.overwrite,
                file.is_dir(),
            )?
            else {
                continue;
            };
            let entry_mtime = if !options.preserve_timestamps || file.is_dir() {
                None
            } else {
                file.last_modified()
                    .and_then(|dt| OffsetDateTime::try_from(dt).ok())
                    .map(SystemTime::from)
            };
            let entry_mode = if options.preserve_permissions {
                file.unix_mode()
            } else {
                None
            };

            // Show verbose output for individual files
            if let Some(progress) = progress {
                if progress.is_verbose() {
                    if file.is_dir() {
                        println!("  creating: {}", file_path.display());
                    } else {
                        println!("  extracting: {}", file_path.display());
                    }
                }
            }

            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            if file.is_dir() {
                std::fs::create_dir_all(&target_path)?;
                if let Some(mode) = entry_mode {
                    utils::apply_permissions(&target_path, mode)?;
                }
            } else {
                let mut output_file = File::create(&target_path)?;
                std::io::copy(&mut file, &mut output_file)?;
                drop(output_file);

                if let Some(mode) = entry_mode {
                    utils::apply_permissions(&target_path, mode)?;
                }

                if let Some(mtime) = entry_mtime {
                    utils::apply_mtime(&target_path, mtime)?;
                }
            }

            // Update progress
            if let Some(progress) = progress {
                progress.set_position((i + 1) as u64);
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

        let file = File::open(archive_path)?;
        let mut archive = ZipArchive::new(file)?;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            if entry.is_file() {
                std::io::copy(&mut entry, &mut std::io::sink())?;
            }
        }
        Ok(())
    }
}
