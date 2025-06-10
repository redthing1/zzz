//! compression format abstraction

use std::path::Path;
use crate::Result;

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
        Self { input_size, output_size, compression_ratio }
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
    pub level: i32,                    // 1-22, default 19
    pub threads: u32,                  // 0 = auto-detect CPU cores
    pub normalize_permissions: bool,    // security: normalize ownership
    pub strip_xattrs: bool,            // security: remove extended attributes  
    pub deterministic: bool,           // sort files for reproducible archives
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
#[derive(Debug, Clone)]
pub struct ExtractionOptions {
    pub overwrite: bool,
    pub strip_components: usize,
}

impl Default for ExtractionOptions {
    fn default() -> Self {
        Self {
            overwrite: false,
            strip_components: 0,
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
        progress: Option<&crate::progress::Progress>
    ) -> Result<CompressionStats>;
    
    fn extract(
        archive_path: &Path,
        output_dir: &Path,
        options: &ExtractionOptions
    ) -> Result<()>;
    
    fn list(archive_path: &Path) -> Result<Vec<ArchiveEntry>>;
    
    fn extension() -> &'static str;
}