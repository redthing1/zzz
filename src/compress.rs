//! compression orchestration

use std::path::Path;
use crate::formats::{CompressionFormat, CompressionOptions, CompressionStats};
use crate::formats::zstd::ZstdFormat;
use crate::filter::FileFilter;
use crate::progress::Progress;
use crate::Result;

/// compress a file or directory to .zst format
pub fn compress(
    input_path: &Path,
    output_path: &Path,
    options: CompressionOptions,
    filter: FileFilter,
    show_progress: bool,
    verbose: bool
) -> Result<CompressionStats> {
    if verbose {
        println!("compressing {} to {}", input_path.display(), output_path.display());
    }
    
    // calculate total size for progress tracking
    let total_size = crate::utils::calculate_dir_size(input_path)?;
    let progress = Progress::new(show_progress, total_size);
    
    // use zstd format
    let stats = ZstdFormat::compress(input_path, output_path, &options, &filter, Some(&progress))?;
    
    progress.finish();
    
    if verbose {
        println!("compressed {} ({}) -> {} ({}) in ratio {:.2}",
            input_path.display(),
            crate::utils::format_bytes(stats.input_size),
            output_path.display(),
            crate::utils::format_bytes(stats.output_size),
            stats.compression_ratio
        );
    }
    
    Ok(stats)
}