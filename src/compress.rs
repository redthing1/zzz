//! compression orchestration

use crate::filter::FileFilter;
use crate::formats::{
    gz::GzipFormat, rar::RarFormat, sevenz::SevenZFormat, xz::XzFormat, zip::ZipFormat,
    zstd::ZstdFormat, CompressionFormat, CompressionOptions, CompressionStats, Format,
};
use crate::progress::Progress;
use crate::Result;
use anyhow::Context;
use std::path::{Path, PathBuf};

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

    ensure_output_outside_input(input_path, output_path)?;

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

fn ensure_output_outside_input(input_path: &Path, output_path: &Path) -> Result<()> {
    let input_abs = std::fs::canonicalize(input_path).with_context(|| {
        format!(
            "Failed to resolve input path '{}'",
            input_path.display()
        )
    })?;
    let output_abs = resolve_absolute_path(output_path)?;
    let output_resolved = canonicalize_with_fallback(&output_abs);

    if input_path.is_file() {
        if output_resolved == input_abs {
            return Err(anyhow::anyhow!(
                "output path '{}' resolves to input file '{}'",
                output_path.display(),
                input_path.display()
            ));
        }
        return Ok(());
    }

    if output_resolved.starts_with(&input_abs) {
        return Err(anyhow::anyhow!(
            "output path '{}' is inside input directory '{}'; choose an output path outside the input tree",
            output_path.display(),
            input_path.display()
        ));
    }

    Ok(())
}

fn resolve_absolute_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        let cwd = std::env::current_dir().context("Failed to resolve current directory")?;
        Ok(cwd.join(path))
    }
}

fn canonicalize_with_fallback(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| {
        if let Some(parent) = path.parent() {
            if let Ok(parent_canon) = std::fs::canonicalize(parent) {
                if let Some(name) = path.file_name() {
                    return parent_canon.join(name);
                }
                return parent_canon;
            }
        }
        path.to_path_buf()
    })
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
