//! RAR archive format support

#[cfg(feature = "rar")]
use crate::formats::{ArchiveEntry, CompressionFormat, CompressionStats, ExtractionOptions};
#[cfg(feature = "rar")]
use crate::Result;
#[cfg(feature = "rar")]
use std::path::Path;

#[cfg(feature = "rar")]
pub struct RarFormat;

#[cfg(feature = "rar")]
impl CompressionFormat for RarFormat {
    fn compress(
        _input_path: &Path,
        _output_path: &Path,
        _options: &crate::formats::CompressionOptions,
        _filter: &crate::filter::FileFilter,
        _progress: Option<&crate::progress::Progress>,
    ) -> Result<CompressionStats> {
        Err(anyhow::anyhow!("RAR compression is not supported"))
    }

    fn extract(
        archive_path: &Path,
        output_dir: &Path,
        options: &ExtractionOptions,
        progress: Option<&crate::progress::Progress>,
    ) -> Result<()> {
        use unrar::Archive;

        let archive_path_str = archive_path.to_str().ok_or_else(|| {
            anyhow::anyhow!(
                "RAR archive path is not valid UTF-8: {}",
                archive_path.display()
            )
        })?;
        let mut archive = Archive::new(archive_path_str).open_for_processing()?;

        let mut entry_count = 0u64;
        while let Some(header) = archive.read_header()? {
            let entry = header.entry();

            if entry.is_file() {
                let output_path = crate::utils::extract_entry_to_path(
                    output_dir,
                    entry.filename.as_path(),
                    options.strip_components,
                    options.overwrite,
                    false,
                )?;
                let Some(output_path) = output_path else {
                    archive = header.skip()?;
                    continue;
                };

                if let Some(parent) = output_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }

                archive = header.extract_to(output_path)?;
            } else {
                archive = header.skip()?;
            }

            // Update progress
            entry_count += 1;
            if let Some(progress) = progress {
                progress.set_position(entry_count);
            }
        }

        Ok(())
    }

    fn list(archive_path: &Path) -> Result<Vec<ArchiveEntry>> {
        use unrar::Archive;

        let mut entries = Vec::new();
        let archive_path_str = archive_path.to_str().ok_or_else(|| {
            anyhow::anyhow!(
                "RAR archive path is not valid UTF-8: {}",
                archive_path.display()
            )
        })?;
        let mut archive = Archive::new(archive_path_str).open_for_listing()?;

        while let Some(header) = archive.read_header()? {
            let entry = header.entry();
            entries.push(ArchiveEntry {
                path: entry.filename.to_string_lossy().to_string(),
                size: entry.unpacked_size,
                is_file: entry.is_file(),
            });
            archive = header.skip()?;
        }

        Ok(entries)
    }

    fn extension() -> &'static str {
        "rar"
    }

    fn test_integrity(archive_path: &Path) -> Result<()> {
        use unrar::Archive;

        let archive_path_str = archive_path.to_str().ok_or_else(|| {
            anyhow::anyhow!(
                "RAR archive path is not valid UTF-8: {}",
                archive_path.display()
            )
        })?;
        let mut archive = Archive::new(archive_path_str).open_for_processing()?;

        while let Some(header) = archive.read_header()? {
            archive = header.test()?;
        }

        Ok(())
    }
}

#[cfg(not(feature = "rar"))]
pub struct RarFormat;

#[cfg(not(feature = "rar"))]
impl crate::formats::CompressionFormat for RarFormat {
    fn compress(
        _input_path: &std::path::Path,
        _output_path: &std::path::Path,
        _options: &crate::formats::CompressionOptions,
        _filter: &crate::filter::FileFilter,
        _progress: Option<&crate::progress::Progress>,
    ) -> crate::Result<crate::formats::CompressionStats> {
        Err(rar_not_enabled_error())
    }

    fn extract(
        _archive_path: &std::path::Path,
        _output_dir: &std::path::Path,
        _options: &crate::formats::ExtractionOptions,
        _progress: Option<&crate::progress::Progress>,
    ) -> crate::Result<()> {
        Err(rar_not_enabled_error())
    }

    fn list(_archive_path: &std::path::Path) -> crate::Result<Vec<crate::formats::ArchiveEntry>> {
        Err(rar_not_enabled_error())
    }

    fn extension() -> &'static str {
        "rar"
    }

    fn test_integrity(_archive_path: &std::path::Path) -> crate::Result<()> {
        Err(rar_not_enabled_error())
    }
}

#[cfg(not(feature = "rar"))]
fn rar_not_enabled_error() -> anyhow::Error {
    anyhow::anyhow!("RAR support not enabled - compile with --features rar")
}
