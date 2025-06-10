//! zzz - A simple, fast compression tool for .zst archives
//!
//! This library provides functionality for creating and extracting zstd-compressed
//! tar archives with smart file filtering and security features.

pub mod cli;
pub mod compress;
pub mod extract;
pub mod list;
pub mod formats;
pub mod filter;
pub mod progress;
pub mod utils;
pub mod error;

// re-export main types for convenience
pub use error::Result;
pub use formats::CompressionFormat;