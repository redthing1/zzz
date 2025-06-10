//! zstd compression format implementation

use crate::filter::FileFilter;
use crate::formats::{
    ArchiveEntry, CompressionFormat, CompressionOptions, CompressionStats, ExtractionOptions,
};
use crate::progress::Progress;
use crate::Result;
use anyhow::Context;
use std::fs::File;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use tar::Builder;
use walkdir::WalkDir;

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
            metadata.permissions().mode()
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
            metadata.permissions().mode()
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

    /// Add a single file to the tar archive
    fn add_file_to_archive(
        tar_builder: &mut Builder<zstd::Encoder<'_, File>>,
        file_path: &Path,
        archive_path: &Path,
        options: &CompressionOptions,
        progress: Option<&Progress>,
        bytes_processed: &mut u64,
    ) -> Result<()> {
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
        *bytes_processed += metadata.len();
        if let Some(progress) = progress {
            progress.update(*bytes_processed);
        }

        Ok(())
    }

    /// Add a directory to the tar archive
    fn add_directory_to_archive(
        tar_builder: &mut Builder<zstd::Encoder<'_, File>>,
        dir_path: &Path,
        archive_path: &Path,
        options: &CompressionOptions,
    ) -> Result<()> {
        let metadata = dir_path.metadata()?;
        let mut header = Self::create_dir_header(&metadata, options)?;

        // ensure directory path ends with /
        let mut dir_path_str = archive_path.to_string_lossy().to_string();
        if !dir_path_str.ends_with('/') {
            dir_path_str.push('/');
        }

        tar_builder.append_data(&mut header, dir_path_str.as_str(), io::empty())?;
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
        let output_file = File::create(output_path)
            .with_context(|| format!("failed to create output file: {}", output_path.display()))?;

        // set up zstd encoder with compression level
        let encoder = zstd::Encoder::new(output_file, options.level).with_context(|| {
            format!("failed to create zstd encoder with level {}", options.level)
        })?;

        // track bytes processed for progress
        let mut bytes_processed = 0u64;

        // determine base path for relative paths in archive
        let base_path = if input_path.is_file() {
            input_path.parent().unwrap_or(Path::new("."))
        } else {
            input_path.parent().unwrap_or(Path::new("."))
        };

        // collect all files to add (with filtering)
        let files_to_add = Self::collect_files_to_add(input_path, filter)?;

        // create tar builder
        let mut tar_builder = Builder::new(encoder);

        // add files to tar archive
        for file_path in files_to_add {
            // calculate relative path for archive
            let archive_path = file_path.strip_prefix(base_path).unwrap_or_else(|_| {
                file_path
                    .file_name()
                    .map(std::path::Path::new)
                    .unwrap_or(&file_path)
            });

            if file_path.is_file() {
                Self::add_file_to_archive(
                    &mut tar_builder,
                    &file_path,
                    archive_path,
                    options,
                    progress,
                    &mut bytes_processed,
                )?;
            } else if file_path.is_dir() {
                Self::add_directory_to_archive(
                    &mut tar_builder,
                    &file_path,
                    archive_path,
                    options,
                )?;
            }
        }

        // finish tar archive and get encoder back
        let encoder = tar_builder.into_inner()?;

        // finish compression and get output file
        let output_file = encoder.finish()?;
        let output_size = output_file.metadata()?.len();

        // finalize progress
        if let Some(progress) = progress {
            progress.update(input_size);
        }

        Ok(CompressionStats::new(input_size, output_size))
    }

    fn extract(archive_path: &Path, output_dir: &Path, options: &ExtractionOptions) -> Result<()> {
        // open archive file
        let archive_file = File::open(archive_path)
            .with_context(|| format!("failed to open archive file: {}", archive_path.display()))?;

        // create zstd decoder
        let decoder = zstd::Decoder::new(archive_file).with_context(|| {
            format!(
                "failed to create zstd decoder for: {}",
                archive_path.display()
            )
        })?;

        // create tar archive reader
        let mut archive = tar::Archive::new(decoder);

        // extract with safety checks
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
        }

        Ok(())
    }

    fn list(archive_path: &Path) -> Result<Vec<ArchiveEntry>> {
        // open archive file
        let archive_file = File::open(archive_path)
            .with_context(|| format!("failed to open archive file: {}", archive_path.display()))?;

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
}
