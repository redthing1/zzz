//! compression format abstraction

use crate::Result;
use std::path::Path;

pub mod gz;
pub mod sevenz;
pub mod xz;
pub mod zip;
pub mod zstd;

#[derive(Debug)]
pub struct CompressionStats {
    pub input_size: u64,
    pub output_size: u64,
    pub compression_ratio: f64,
}

impl CompressionStats {
    pub fn new(input_size: u64, output_size: u64) -> Self {
        let compression_ratio = if input_size > 0 {
            output_size as f64 / input_size as f64
        } else {
            0.0
        };
        Self {
            input_size,
            output_size,
            compression_ratio,
        }
    }
}

#[derive(Debug)]
pub struct ArchiveEntry {
    pub path: String,
    pub size: u64,
    pub is_file: bool,
}

/// compression options for creating archives
#[derive(Debug, Clone)]
pub struct CompressionOptions {
    pub level: i32,                  // 1-22, default 19
    pub threads: u32,                // 0 = auto-detect CPU cores
    pub normalize_permissions: bool, // security: normalize ownership
    pub strip_xattrs: bool,          // security: remove extended attributes
    pub deterministic: bool,         // sort files for reproducible archives
}

impl Default for CompressionOptions {
    fn default() -> Self {
        Self {
            level: 19,
            threads: 0, // auto-detect
            normalize_permissions: true,
            strip_xattrs: true,
            deterministic: true,
        }
    }
}

/// extraction options for extracting archives
#[derive(Debug, Clone, Default)]
pub struct ExtractionOptions {
    pub overwrite: bool,
    pub strip_components: usize,
}

/// Supported compression formats
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Zstd,
    Gzip,
    Xz,
    Zip,
    SevenZ,
}

impl Format {
    /// Detect format from file path, with magic number validation
    pub fn detect(path: &Path) -> Result<Self> {
        // Try magic number detection first (most reliable)
        if let Ok(format) = Self::from_magic(path) {
            return Ok(format);
        }

        // Fall back to extension-based detection
        if let Some(format) = Self::from_extension(path) {
            return Ok(format);
        }

        Err(anyhow::anyhow!("unsupported archive format"))
    }

    /// Detect format from file extension
    pub fn from_extension(path: &Path) -> Option<Self> {
        let filename = path.file_name()?.to_str()?.to_lowercase();

        if filename.ends_with(".zst") || filename.ends_with(".zstd") {
            Some(Format::Zstd)
        } else if filename.ends_with(".tgz")
            || filename.ends_with(".tar.gz")
            || filename.ends_with(".gz")
        {
            Some(Format::Gzip)
        } else if filename.ends_with(".txz")
            || filename.ends_with(".tar.xz")
            || filename.ends_with(".xz")
        {
            Some(Format::Xz)
        } else if filename.ends_with(".zip") {
            Some(Format::Zip)
        } else if filename.ends_with(".7z") {
            Some(Format::SevenZ)
        } else {
            None
        }
    }

    /// Detect format using magic number detection
    fn from_magic(path: &Path) -> Result<Self> {
        use std::fs::File;
        use std::io::Read;

        let mut file = File::open(path)?;
        let mut buffer = [0u8; 16];
        let bytes_read = file.read(&mut buffer)?;

        if bytes_read >= 4 {
            // Check magic numbers
            match &buffer[..4] {
                [0x28, 0xB5, 0x2F, 0xFD] => return Ok(Format::Zstd), // Zstandard
                [0x1F, 0x8B, _, _] => return Ok(Format::Gzip),       // Gzip
                [0xFD, 0x37, 0x7A, 0x58] => return Ok(Format::Xz),   // XZ
                [0x50, 0x4B, 0x03, 0x04] | [0x50, 0x4B, 0x05, 0x06] | [0x50, 0x4B, 0x07, 0x08] => {
                    return Ok(Format::Zip); // ZIP
                }
                _ => {}
            }
        }

        if bytes_read >= 6 && &buffer[..6] == b"7z\xBC\xAF\x27\x1C" {
            return Ok(Format::SevenZ); // 7-Zip
        }

        // Use tree_magic_mini as final fallback
        let mime_type = tree_magic_mini::from_filepath(path);
        match mime_type.unwrap_or("") {
            "application/zstd" => Ok(Format::Zstd),
            "application/gzip" | "application/x-gzip" => Ok(Format::Gzip),
            "application/x-xz" => Ok(Format::Xz),
            "application/zip" => Ok(Format::Zip),
            "application/x-7z-compressed" => Ok(Format::SevenZ),
            _ => Err(anyhow::anyhow!("unsupported archive format")),
        }
    }

    /// Get the default file extension for this format
    pub fn extension(&self) -> &'static str {
        match self {
            Format::Zstd => "zst",
            Format::Gzip => "tgz",
            Format::Xz => "txz",
            Format::Zip => "zip",
            Format::SevenZ => "7z",
        }
    }

    /// Get format name for display
    pub fn name(&self) -> &'static str {
        match self {
            Format::Zstd => "Zstandard",
            Format::Gzip => "Gzip",
            Format::Xz => "XZ",
            Format::Zip => "ZIP",
            Format::SevenZ => "7-Zip",
        }
    }
}

/// trait for compression formats
pub trait CompressionFormat {
    fn compress(
        input_path: &Path,
        output_path: &Path,
        options: &CompressionOptions,
        filter: &crate::filter::FileFilter,
        progress: Option<&crate::progress::Progress>,
    ) -> Result<CompressionStats>;

    fn extract(archive_path: &Path, output_dir: &Path, options: &ExtractionOptions) -> Result<()>;

    fn list(archive_path: &Path) -> Result<Vec<ArchiveEntry>>;

    fn extension() -> &'static str;
}
