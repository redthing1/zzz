//! file filtering system with comprehensive garbage file exclusion

use crate::Result;
use glob::Pattern;
use once_cell::sync::Lazy;
use std::path::Path;

/// comprehensive list of garbage files to exclude by default
pub const GARBAGE_FILES: &[&str] = &[
    // macOS system files
    ".DS_Store",
    "._*",                     // resource forks
    ".Spotlight-V100",         // spotlight index
    ".Trashes",                // trash
    ".fseventsd",              // filesystem events
    ".VolumeIcon.icns",        // volume icons
    ".DocumentRevisions-V100", // document revisions
    ".TemporaryItems",         // temporary items
    // Windows system files
    "thumbs.db",
    "Thumbs.db",
    "desktop.ini",
    "Desktop.ini",
    "ehthumbs.db",
    "ehthumbs_vista.db",
    "$RECYCLE.BIN",
    "System Volume Information",
    "hiberfil.sys",
    "pagefile.sys",
    "swapfile.sys",
    // Linux/Unix system files
    ".directory", // KDE folder metadata
    ".trash",
    ".Trash-*", // trash directories
    ".nfs*",    // NFS lock files
    // Development artifacts
    "__pycache__",
    "*.pyc",
    "*.pyo",
    "*.pyd",
    ".pytest_cache",
    ".coverage",
    ".tox",
    "node_modules",
    ".npm",
    ".yarn",
    ".git",
    ".svn",
    ".hg",
    ".bzr",
    "target/debug",
    "target/release", // Rust build dirs
    ".gradle",
    ".maven",
    ".vscode",
    ".idea", // IDE files (configurable)
    // Temporary/backup files
    "*.tmp",
    "*.temp",
    "*.bak",
    "*.orig",
    "*~",
    ".#*",
    "#*#", // editor backup files
    "*.swp",
    "*.swo",  // vim swap files
    ".*.sw?", // vim swap file pattern
];

static COMPILED_GARBAGE_PATTERNS: Lazy<Vec<Pattern>> = Lazy::new(|| {
    GARBAGE_FILES
        .iter()
        .filter_map(|s| Pattern::new(s).ok())
        .collect()
});

pub struct FileFilter {
    use_defaults: bool,
    custom_excludes: Vec<Pattern>,
}

impl FileFilter {
    /// create new file filter with optional custom patterns
    pub fn new(use_defaults: bool, custom_patterns: &[String]) -> Result<Self> {
        let mut custom_excludes = Vec::new();
        for pattern in custom_patterns {
            custom_excludes.push(Pattern::new(pattern)?);
        }
        Ok(Self {
            use_defaults,
            custom_excludes,
        })
    }

    /// check if a path should be excluded from archiving
    pub fn should_exclude(&self, path: &Path) -> bool {
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // check custom patterns first (against filename only for consistency)
        for pattern in &self.custom_excludes {
            if pattern.matches(filename) {
                return true;
            }
        }

        // check default garbage files if enabled
        if self.use_defaults {
            for pattern in &*COMPILED_GARBAGE_PATTERNS {
                if pattern.matches(filename) {
                    return true;
                }
            }
        }

        false
    }

    /// check if a path should be included in archiving (inverse of should_exclude)
    pub fn should_include(&self, path: &Path) -> bool {
        !self.should_exclude(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_file_filter_with_defaults() -> Result<()> {
        let filter = FileFilter::new(true, &[])?;

        // Test default garbage files
        assert!(filter.should_exclude(Path::new(".DS_Store")));
        assert!(filter.should_exclude(Path::new("thumbs.db")));
        assert!(filter.should_exclude(Path::new("__pycache__")));
        assert!(filter.should_exclude(Path::new("node_modules")));
        assert!(filter.should_exclude(Path::new(".git")));

        // Test pattern matching
        assert!(filter.should_exclude(Path::new("._resource_fork")));
        assert!(filter.should_exclude(Path::new("file.tmp")));
        assert!(filter.should_exclude(Path::new("backup~")));
        assert!(filter.should_exclude(Path::new(".#lockfile")));

        // Test normal files are not excluded
        assert!(!filter.should_exclude(Path::new("README.md")));
        assert!(!filter.should_exclude(Path::new("src/main.rs")));
        assert!(!filter.should_exclude(Path::new("Cargo.toml")));

        Ok(())
    }

    #[test]
    fn test_file_filter_without_defaults() -> Result<()> {
        let filter = FileFilter::new(false, &[])?;

        // Default garbage files should NOT be excluded when defaults are disabled
        assert!(!filter.should_exclude(Path::new(".DS_Store")));
        assert!(!filter.should_exclude(Path::new("thumbs.db")));
        assert!(!filter.should_exclude(Path::new("__pycache__")));

        Ok(())
    }

    #[test]
    fn test_custom_excludes() -> Result<()> {
        let custom_patterns = vec![
            "*.log".to_string(),
            "test_*".to_string(),
            "secret.txt".to_string(),
        ];
        let filter = FileFilter::new(true, &custom_patterns)?;

        // Test custom patterns
        assert!(filter.should_exclude(Path::new("debug.log")));
        assert!(filter.should_exclude(Path::new("test_file.txt")));
        assert!(filter.should_exclude(Path::new("secret.txt")));

        // Test files that should not be excluded
        assert!(!filter.should_exclude(Path::new("important.txt")));
        assert!(!filter.should_exclude(Path::new("production_config.yaml")));

        Ok(())
    }

    #[test]
    fn test_custom_excludes_override_defaults() -> Result<()> {
        let custom_patterns = vec!["*.rs".to_string()];
        let filter = FileFilter::new(true, &custom_patterns)?;

        // Custom patterns should work
        assert!(filter.should_exclude(Path::new("main.rs")));
        assert!(filter.should_exclude(Path::new("lib.rs")));

        // Default patterns should still work
        assert!(filter.should_exclude(Path::new(".DS_Store")));

        // Non-matching files should not be excluded
        assert!(!filter.should_exclude(Path::new("Cargo.toml")));

        Ok(())
    }

    #[test]
    fn test_invalid_pattern() {
        // Test that invalid glob patterns return an error
        let result = FileFilter::new(true, &["[".to_string()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_path_vs_filename_matching() -> Result<()> {
        let filter = FileFilter::new(true, &[])?;

        // Test that patterns match on filename, not full path
        assert!(filter.should_exclude(Path::new("some/path/.DS_Store")));
        assert!(filter.should_exclude(Path::new("nested/dir/thumbs.db")));

        // Test directory names in paths don't trigger false matches
        assert!(!filter.should_exclude(Path::new(".DS_Store_backup/file.txt")));

        Ok(())
    }
}
