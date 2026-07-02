//! compression orchestration

use crate::archive_plan::{ArchivePlan, ArchiveSource, ArchiveSourceKind};
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
    input_paths: &[PathBuf],
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
            format_input_paths(input_paths),
            output_path.display()
        );
    }

    // Use format override or detect from output path
    let format = format_override
        .map(Ok)
        .unwrap_or_else(|| detect_output_format(output_path))?;

    let plan = ArchivePlan::from_paths(
        input_paths,
        &filter,
        options.symlink_policy,
        options.deterministic,
    )?;
    ensure_output_outside_sources(&plan.sources, output_path)?;

    if verbose {
        println!("using {} format", format.name());
    }

    if plan.skipped_symlinks > 0 {
        eprintln!(
            "warning: skipped {} symlink entries; use --follow-symlinks",
            plan.skipped_symlinks
        );
    }
    let progress = Progress::new(show_progress, plan.total_size, verbose);

    // dispatch to appropriate format implementation
    let stats = match format {
        Format::Zstd => ZstdFormat::compress_plan(&plan, output_path, &options, Some(&progress))?,
        Format::Gzip => GzipFormat::compress_plan(&plan, output_path, &options, Some(&progress))?,
        Format::Xz => XzFormat::compress_plan(&plan, output_path, &options, Some(&progress))?,
        Format::Zip => ZipFormat::compress_plan(&plan, output_path, &options, Some(&progress))?,
        Format::SevenZ => {
            SevenZFormat::compress_plan(&plan, output_path, &options, Some(&progress))?
        }
        Format::Rar => RarFormat::compress_plan(&plan, output_path, &options, Some(&progress))?,
    };

    progress.finish();

    if verbose {
        println!(
            "compressed {} ({}) -> {} ({}) ratio {:.2}",
            format_input_paths(input_paths),
            crate::utils::format_bytes(stats.input_size),
            output_path.display(),
            crate::utils::format_bytes(stats.output_size),
            stats.compression_ratio
        );
    }

    Ok(stats)
}

fn format_input_paths(input_paths: &[PathBuf]) -> String {
    if input_paths.len() == 1 {
        return input_paths[0].display().to_string();
    }

    format!("{} inputs", input_paths.len())
}

fn ensure_output_outside_sources(sources: &[ArchiveSource], output_path: &Path) -> Result<()> {
    let output_abs = resolve_absolute_path(output_path)?;
    let output_resolved = canonicalize_with_fallback(&output_abs);

    for source in sources {
        if source.kind == ArchiveSourceKind::File {
            if output_resolved == source.canonical_path {
                return Err(anyhow::anyhow!(
                    "output path '{}' resolves to input file '{}'",
                    output_path.display(),
                    source.input_path.display()
                ));
            }
            continue;
        }

        if output_resolved.starts_with(&source.canonical_path) {
            return Err(anyhow::anyhow!(
                "output path '{}' is inside input directory '{}'; choose an output path outside the input tree",
                output_path.display(),
                source.input_path.display()
            ));
        }
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
