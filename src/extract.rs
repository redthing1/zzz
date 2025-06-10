//! extraction orchestration

use crate::formats::zstd::ZstdFormat;
use crate::formats::{CompressionFormat, ExtractionOptions};
use crate::Result;
use std::path::Path;

/// extract a .zst archive to a directory
pub fn extract(
    archive_path: &Path,
    output_dir: &Path,
    options: ExtractionOptions,
    verbose: bool,
) -> Result<()> {
    if verbose {
        println!(
            "extracting {} to {}",
            archive_path.display(),
            output_dir.display()
        );
    }

    // ensure output directory exists
    if !output_dir.exists() {
        std::fs::create_dir_all(output_dir)?;
    }

    // use zstd format
    ZstdFormat::extract(archive_path, output_dir, &options)?;

    if verbose {
        println!("extraction completed");
    }

    Ok(())
}
