//! zzz - A simple, fast compression multitool
//!
//! This library provides functionality for creating and extracting archives in multiple
//! formats (zst, tgz, txz, zip, 7z) with smart file filtering and security features.

pub mod cli;
pub mod compress;
pub mod error;
pub mod extract;
pub mod filter;
pub mod formats;
pub mod list;
pub mod progress;
pub mod utils;

// re-export main types for convenience
pub use error::Result;
pub use formats::CompressionFormat;
