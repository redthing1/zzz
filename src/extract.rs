//! extraction orchestration

use crate::formats::{
    gz::GzipFormat, rar::RarFormat, sevenz::SevenZFormat, xz::XzFormat, zip::ZipFormat,
    zstd::ZstdFormat, CompressionFormat, ExtractionOptions, Format,
};
use crate::progress::Progress;
use crate::Result;
use std::path::Path;

/// extract an archive to a directory using auto-detected format
pub fn extract(
    archive_path: &Path,
    output_dir: &Path,
    options: ExtractionOptions,
    show_progress: bool,
    verbose: bool,
) -> Result<()> {
    if verbose {
        println!(
            "extracting {} to {}",
            archive_path.display(),
            output_dir.display()
        );
    }

    // detect format from archive
    let format = Format::detect(archive_path)?;

    if verbose {
        println!("detected {} format", format.name());
    }

    // ensure output directory exists
    if !output_dir.exists() {
        std::fs::create_dir_all(output_dir)?;
    }

    // create progress tracker (enabled when requested or verbose)
    let progress = match format {
        Format::Zstd | Format::Gzip | Format::Xz => {
            Progress::new(show_progress || verbose, 0, verbose)
        }
        Format::Zip | Format::SevenZ | Format::Rar => {
            Progress::new_items(show_progress || verbose, 0, verbose)
        }
    };

    // dispatch to appropriate format implementation
    match format {
        Format::Zstd => ZstdFormat::extract(archive_path, output_dir, &options, Some(&progress))?,
        Format::Gzip => GzipFormat::extract(archive_path, output_dir, &options, Some(&progress))?,
        Format::Xz => XzFormat::extract(archive_path, output_dir, &options, Some(&progress))?,
        Format::Zip => ZipFormat::extract(archive_path, output_dir, &options, Some(&progress))?,
        Format::SevenZ => {
            SevenZFormat::extract(archive_path, output_dir, &options, Some(&progress))?
        }
        Format::Rar => RarFormat::extract(archive_path, output_dir, &options, Some(&progress))?,
    }

    progress.finish_with_message("extraction completed");

    if verbose {
        println!("extraction completed");
    }

    Ok(())
}
