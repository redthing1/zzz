//! utility functions for file size calculations and formatting

use crate::Result;
use std::path::Path;

/// sanitize an archive entry path and apply strip_components
pub fn sanitize_archive_entry_path(
    entry_path: &Path,
    strip_components: usize,
) -> Result<Option<std::path::PathBuf>> {
    use std::path::{Component, PathBuf};

    let mut components: Vec<std::ffi::OsString> = Vec::new();

    for component in entry_path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => {
                return Err(anyhow::anyhow!(
                    "unsafe archive path: {}",
                    entry_path.display()
                ));
            }
            Component::CurDir => {}
            Component::Normal(part) => components.push(part.to_os_string()),
        }
    }

    if strip_components >= components.len() {
        return Ok(None);
    }

    let mut sanitized = PathBuf::new();
    for component in components.into_iter().skip(strip_components) {
        sanitized.push(component);
    }

    Ok(Some(sanitized))
}

/// ensure no symlink exists in the target path's ancestor chain under root
pub fn ensure_no_symlink_ancestors(root: &Path, target: &Path) -> Result<()> {
    use std::io::ErrorKind;
    use std::path::PathBuf;

    let relative = target.strip_prefix(root).map_err(|_| {
        anyhow::anyhow!(
            "target path '{}' is outside extraction root '{}'",
            target.display(),
            root.display()
        )
    })?;

    let mut current = PathBuf::from(root);
    for component in relative.components() {
        current.push(component);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(anyhow::anyhow!(
                        "unsafe archive path: symlink ancestor '{}'",
                        current.display()
                    ));
                }
            }
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
    }

    Ok(())
}

/// prepare a safe output path for extracting an archive entry
pub fn extract_entry_to_path(
    output_dir: &Path,
    entry_path: &Path,
    strip_components: usize,
    overwrite: bool,
    entry_is_dir: bool,
) -> Result<Option<std::path::PathBuf>> {
    match prepare_extract_target(
        output_dir,
        entry_path,
        strip_components,
        overwrite,
        entry_is_dir,
    )? {
        ExtractTarget::Target(path) => Ok(Some(path)),
        ExtractTarget::SkipStrip => Ok(None),
        ExtractTarget::SkipExisting(path) => Err(anyhow::anyhow!(
            "output file '{}' already exists. Use --overwrite to replace.",
            path.display()
        )),
    }
}

/// extraction target resolution outcomes
pub enum ExtractTarget {
    SkipStrip,
    SkipExisting(std::path::PathBuf),
    Target(std::path::PathBuf),
}

/// prepare a safe extraction target path with skip reasons
pub fn prepare_extract_target(
    output_dir: &Path,
    entry_path: &Path,
    strip_components: usize,
    overwrite: bool,
    entry_is_dir: bool,
) -> Result<ExtractTarget> {
    let relative_path = sanitize_archive_entry_path(entry_path, strip_components)?;
    let Some(relative_path) = relative_path else {
        return Ok(ExtractTarget::SkipStrip);
    };
    let target_path = output_dir.join(relative_path);

    ensure_no_symlink_ancestors(output_dir, &target_path)?;

    if target_path.exists() && !overwrite {
        if entry_is_dir && target_path.is_dir() {
            return Ok(ExtractTarget::Target(target_path));
        }
        return Ok(ExtractTarget::SkipExisting(target_path));
    }

    Ok(ExtractTarget::Target(target_path))
}

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

/// calculate total size of a directory with file filtering
pub fn calculate_directory_size(path: &Path, filter: &crate::filter::FileFilter) -> Result<u64> {
    let mut total = 0;

    if path.is_file() {
        if let Some(filename) = path.file_name() {
            if !filter.should_include_relative(Path::new(filename)) {
                return Ok(0);
            }
        }
        return Ok(path.metadata()?.len());
    }

    for entry in filter.walk_entries(path) {
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

    print!("{message} [y/N]: ");
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
    fn test_prepare_extract_target_allows_existing_dir() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let output_dir = temp_dir.path().join("out");
        fs::create_dir(&output_dir)?;

        let existing_dir = output_dir.join("existing");
        fs::create_dir(&existing_dir)?;

        let result = prepare_extract_target(&output_dir, Path::new("existing"), 0, false, true)?;

        match result {
            ExtractTarget::Target(path) => assert_eq!(path, existing_dir),
            _ => anyhow::bail!("expected target for existing directory"),
        }

        Ok(())
    }

    #[test]
    fn test_prepare_extract_target_rejects_existing_file() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let output_dir = temp_dir.path().join("out");
        fs::create_dir(&output_dir)?;

        let existing_file = output_dir.join("file.txt");
        fs::write(&existing_file, "data")?;

        let result = prepare_extract_target(&output_dir, Path::new("file.txt"), 0, false, false)?;

        match result {
            ExtractTarget::SkipExisting(path) => assert_eq!(path, existing_file),
            _ => anyhow::bail!("expected skip existing for file"),
        }

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
