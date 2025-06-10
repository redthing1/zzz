//! zzz - A simple, fast compression tool for .zst archives
//!
//! This library provides functionality for creating and extracting zstd-compressed
//! tar archives with smart file filtering and security features.

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
