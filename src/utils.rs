//! utility functions for file size calculations and formatting

use crate::Result;
use std::path::Path;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.00 KiB");
        assert_eq!(format_bytes(1536), "1.50 KiB");
        assert_eq!(format_bytes(1024 * 1024), "1.00 MiB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.00 GiB");
        assert_eq!(format_bytes(1024_u64.pow(4)), "1.00 TiB");
        assert_eq!(format_bytes(1024_u64.pow(5)), "1024.00 TiB");
    }

    #[test]
    fn test_calculate_dir_size_single_file() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let file_path = temp_dir.path().join("test.txt");

        // Create a file with known content
        fs::write(&file_path, "Hello, World!")?;

        let size = calculate_dir_size(&file_path)?;
        assert_eq!(size, 13); // "Hello, World!" is 13 bytes

        Ok(())
    }

    #[test]
    fn test_calculate_dir_size_directory() -> Result<()> {
        let temp_dir = TempDir::new()?;

        // Create multiple files
        fs::write(temp_dir.path().join("file1.txt"), "12345")?; // 5 bytes
        fs::write(temp_dir.path().join("file2.txt"), "abcdef")?; // 6 bytes

        // Create subdirectory with file
        let sub_dir = temp_dir.path().join("subdir");
        fs::create_dir(&sub_dir)?;
        fs::write(sub_dir.join("file3.txt"), "xyz")?; // 3 bytes

        let total_size = calculate_dir_size(temp_dir.path())?;
        assert_eq!(total_size, 14); // 5 + 6 + 3 = 14 bytes

        Ok(())
    }

    #[test]
    fn test_calculate_dir_size_empty_directory() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let size = calculate_dir_size(temp_dir.path())?;
        assert_eq!(size, 0);
        Ok(())
    }
}
