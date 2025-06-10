//! archive listing functionality

use std::path::Path;
use crate::formats::CompressionFormat;
use crate::formats::zstd::ZstdFormat;
use crate::Result;

/// list contents of a .zst archive
pub fn list(archive_path: &Path, verbose: bool) -> Result<()> {
    if verbose {
        println!("listing contents of {}", archive_path.display());
    }
    
    let entries = ZstdFormat::list(archive_path)?;
    
    for entry in entries {
        if verbose {
            // detailed listing with sizes
            let size_str = if entry.is_file {
                crate::utils::format_bytes(entry.size)
            } else {
                "dir".to_string()
            };
            println!("{:>10} {}", size_str, entry.path);
        } else {
            // simple listing
            println!("{}", entry.path);
        }
    }
    
    Ok(())
}