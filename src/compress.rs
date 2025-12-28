//! compression orchestration

use crate::filter::FileFilter;
use crate::formats::{
    gz::GzipFormat, rar::RarFormat, sevenz::SevenZFormat, xz::XzFormat, zip::ZipFormat,
    zstd::ZstdFormat, CompressionFormat, CompressionOptions, CompressionStats, Format,
};
use crate::progress::Progress;
use crate::Result;
use std::path::Path;

/// compress a file or directory using specified or auto-detected format
pub fn compress(
    input_path: &Path,
    output_path: &Path,
    options: CompressionOptions,
    filter: FileFilter,
    show_progress: bool,
    verbose: bool,
    format_override: Option<Format>,
) -> Result<CompressionStats> {
    if verbose {
        println!(
            "compressing {} to {}",
            input_path.display(),
            output_path.display()
        );
    }

    // Use format override or detect from output path
    let format = format_override
        .map(Ok)
        .unwrap_or_else(|| detect_output_format(output_path))?;

    if verbose {
        println!("using {} format", format.name());
    }

    // calculate total size for progress tracking
    let total_size = crate::utils::calculate_directory_size(
        input_path,
        &filter,
        options.follow_symlinks,
        options.allow_symlink_escape,
    )?;
    let progress = Progress::new(show_progress, total_size, verbose);

    // dispatch to appropriate format implementation
    let stats = match format {
        Format::Zstd => {
            ZstdFormat::compress(input_path, output_path, &options, &filter, Some(&progress))?
        }
        Format::Gzip => {
            GzipFormat::compress(input_path, output_path, &options, &filter, Some(&progress))?
        }
        Format::Xz => {
            XzFormat::compress(input_path, output_path, &options, &filter, Some(&progress))?
        }
        Format::Zip => {
            ZipFormat::compress(input_path, output_path, &options, &filter, Some(&progress))?
        }
        Format::SevenZ => {
            SevenZFormat::compress(input_path, output_path, &options, &filter, Some(&progress))?
        }
        Format::Rar => {
            RarFormat::compress(input_path, output_path, &options, &filter, Some(&progress))?
        }
    };

    progress.finish();

    if verbose {
        println!(
            "compressed {} ({}) -> {} ({}) ratio {:.2}",
            input_path.display(),
            crate::utils::format_bytes(stats.input_size),
            output_path.display(),
            crate::utils::format_bytes(stats.output_size),
            stats.compression_ratio
        );
    }

    Ok(stats)
}

/// Detect compression format from output file extension
fn detect_output_format(output_path: &Path) -> Result<Format> {
    Format::from_extension(output_path).ok_or_else(|| {
        anyhow::anyhow!(
            "cannot determine compression format from extension: {}",
            output_path.display()
        )
    })
}
