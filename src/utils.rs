//! utility functions for file size calculations and formatting

use std::path::Path;
use crate::Result;

/// calculate total size of a directory recursively
pub fn calculate_dir_size(path: &Path) -> Result<u64> {
    let mut total = 0;
    
    if path.is_file() {
        return Ok(path.metadata()?.len());
    }
    
    for entry in walkdir::WalkDir::new(path) {
        let entry = entry?;
        if entry.file_type().is_file() {
            total += entry.metadata()?.len();
        }
    }
    
    Ok(total)
}

/// format bytes in human-readable format
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;
    
    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }
    
    if unit_index == 0 {
        format!("{} {}", bytes, UNITS[unit_index])
    } else {
        format!("{:.2} {}", size, UNITS[unit_index])
    }
}

/// prompt user for yes/no confirmation
pub fn prompt_yes_no(message: &str) -> bool {
    use std::io::{self, Write};
    
    print!("{} [y/N]: ", message);
    io::stdout().flush().unwrap();
    
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_ok() {
        let input = input.trim().to_lowercase();
        input == "y" || input == "yes"
    } else {
        false
    }
}