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

fn zip_last_modified(metadata: &std::fs::Metadata, strip_timestamps: bool) -> zip::DateTime {
    if strip_timestamps {
        return zip::DateTime::default();
    }

    let Ok(modified) = metadata.modified() else {
        return zip::DateTime::default();
    };
    let dt = OffsetDateTime::from(modified);
    zip::DateTime::try_from(dt).unwrap_or_default()
}

impl CompressionFormat for ZipFormat {
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
            progress.set_length(input_size);
        }

        if input_path.is_file() {
            // Single file compression
            // Password protection is not supported for ZIP format
            if options.password.is_some() {
                return Err(anyhow::anyhow!("Password protection is not supported for ZIP format. Use 7z format for password protection."));
            }

            let metadata = std::fs::metadata(input_path).with_context(|| {
                format!(
                    "Failed to read metadata for input file {}",
                    input_path.display()
                )
            })?;
            let zip_time = zip_last_modified(&metadata, options.strip_timestamps);
            let permissions = if options.normalize_permissions {
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
            };
            let current_file_options = base_file_options
                .last_modified_time(zip_time)
                .unix_permissions(permissions);

            let filename_os = input_path.file_name().ok_or_else(|| {
                anyhow::anyhow!(
                    "Could not determine filename from input path: {}",
                    input_path.display()
                )
            })?;
            if !filter.should_include_relative(Path::new(filename_os)) {
                zip_writer.finish()?;
                let output_size = std::fs::metadata(output_path)?.len();
                return Ok(CompressionStats::new(input_size, output_size));
            }

            let filename = filename_os.to_str().ok_or_else(|| {
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
            let canonical_root = if options.follow_symlinks && !options.allow_symlink_escape {
                Some(std::fs::canonicalize(input_path).with_context(|| {
                    format!("Failed to resolve input root '{}'", input_path.display())
                })?)
            } else {
                None
            };
            let mut entries: Vec<_> = filter
                .walk_entries_with_follow(input_path, options.follow_symlinks)
                .map(|entry| {
                    let entry = entry?;
                    if entry.path_is_symlink() {
                        if !options.follow_symlinks {
                            return Err(anyhow::anyhow!(
                                "symlink '{}' is not supported for archiving (use --follow-symlinks to include targets)",
                                entry.path().display()
                            ));
                        }
                        if let Some(root) = &canonical_root {
                            utils::ensure_symlink_within_root(root, entry.path())?;
                        }
                    }
                    Ok(entry)
                })
                .collect::<std::result::Result<Vec<_>, _>>()?;

            // Sort for deterministic archives
            if options.deterministic {
                entries.sort_by(|a, b| a.path().cmp(b.path()));
            }

            let mut processed_size = 0u64;

            for entry in entries {
                let path = entry.path();
                let relative_path = path.strip_prefix(base_path)?;
                let path_str = utils::normalize_archive_path(relative_path);

                let metadata = entry.metadata()?;
                let zip_time = zip_last_modified(&metadata, options.strip_timestamps);
                let permissions = if options.normalize_permissions {
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
                };
                let current_file_options = base_file_options
                    .last_modified_time(zip_time)
                    .unix_permissions(permissions);

                if path.is_file() {
                    zip_writer.start_file(path_str.as_str(), current_file_options)?;

                    let mut file = File::open(path).with_context(|| {
                        format!("Failed to open file for archiving {}", path.display())
                    })?;
                    std::io::copy(&mut file, &mut zip_writer)?;

                    processed_size += metadata.len();

                    if let Some(progress) = progress {
                        progress.set_position(processed_size);
                    }
                } else if path.is_dir() {
                    let permissions = if options.normalize_permissions {
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
                    };
                    let current_file_options = base_file_options
                        .last_modified_time(zip_time)
                        .unix_permissions(permissions);

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
            let entry_mtime = if options.strip_timestamps || file.is_dir() {
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
