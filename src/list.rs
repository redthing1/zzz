//! archive listing functionality

use crate::formats::{
    gz::GzipFormat, rar::RarFormat, sevenz::SevenZFormat, xz::XzFormat, zip::ZipFormat,
    zstd::ZstdFormat, CompressionFormat, Format,
};
use crate::Result;
use std::path::Path;

/// list contents of an archive using auto-detected format
pub fn list(archive_path: &Path, verbose: bool) -> Result<()> {
    if verbose {
        println!("listing contents of {}", archive_path.display());
    }

    // detect format from archive
    let format = Format::detect(archive_path)?;

    if verbose {
        println!("detected {} format", format.name());
    }

    // dispatch to appropriate format implementation
    let entries = match format {
        Format::Zstd => ZstdFormat::list(archive_path)?,
        Format::Gzip => GzipFormat::list(archive_path)?,
        Format::Xz => XzFormat::list(archive_path)?,
        Format::Zip => ZipFormat::list(archive_path)?,
        Format::SevenZ => SevenZFormat::list(archive_path)?,
        Format::Rar => RarFormat::list(archive_path)?,
    };

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
