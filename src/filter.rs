//! file filtering system with comprehensive garbage file exclusion

use crate::Result;
use glob::Pattern;
use once_cell::sync::Lazy;
use std::path::Path;
use walkdir::WalkDir;

/// comprehensive list of garbage files to exclude by default
pub const GARBAGE_FILES: &[&str] = &[
    // macOS system files
    "__MACOSX",
    ".AppleDouble",
    ".DS_Store",
    "._*", // resource forks
    ".LSOverride",
    ".Spotlight-V100",         // spotlight index
    ".Trashes",                // trash
    ".fseventsd",              // filesystem events
    ".VolumeIcon.icns",        // volume icons
    ".DocumentRevisions-V100", // document revisions
    ".TemporaryItems",         // temporary items
    "Icon\r",                  // finder icon
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
    "lost+found",
    // Development artifacts
    "__pycache__",
    "*.pyc",
    "*.pyo",
    "*.pyd",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    ".hypothesis",
    ".coverage",
    ".nox",
    ".tox",
    "__pypackages__",
    ".venv",
    "venv",
    "node_modules",
    ".npm",
    ".pnpm-store",
    ".yarn",
    ".yarn-cache",
    "bower_components",
    ".git",
    ".svn",
    ".hg",
    ".bzr",
    "target",
    "target/debug",
    "target/release", // Rust build dirs
    ".gradle",
    ".maven",
    ".vscode",
    ".idea", // IDE files (configurable)
    ".direnv",
    ".cache",
    ".parcel-cache",
    ".turbo",
    ".next",
    ".nuxt",
    ".svelte-kit",
    "CMakeFiles",
    "CMakeCache.txt",
    "cmake-build-*",
    "npm-debug.log",
    "yarn-error.log",
    "pnpm-debug.log",
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

/// common sensitive files and directories for redact mode
pub const SENSITIVE_FILES: &[&str] = &[
    ".env",
    ".env.*",
    ".npmrc",
    ".pypirc",
    ".netrc",
    ".aws",
    ".ssh",
    ".gnupg",
    ".kube",
    "id_rsa",
    "id_dsa",
    "id_ecdsa",
    "id_ed25519",
    "*.pem",
    "*.key",
    "*.p12",
    "*.pfx",
    "*.kdbx",
    "*.gpg",
    "*.pgp",
    "*.age",
    ".sops.yaml",
    ".sops.yml",
    ".sops.json",
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
    fn normalize_relative_path(path: &Path) -> String {
        let mut parts = Vec::new();
        for component in path.components() {
            if let std::path::Component::Normal(part) = component {
                parts.push(part.to_string_lossy());
            }
        }
        parts.join("/")
    }

    fn matches_patterns(patterns: &[Pattern], filename: &str, relative_path: &Path) -> bool {
        if !filename.is_empty() && patterns.iter().any(|pattern| pattern.matches(filename)) {
            return true;
        }

        let components: Vec<String> = relative_path
            .components()
            .filter_map(|component| match component {
                std::path::Component::Normal(part) => Some(part.to_string_lossy().to_string()),
                _ => None,
            })
            .collect();

        if components
            .iter()
            .any(|component| patterns.iter().any(|pattern| pattern.matches(component)))
        {
            return true;
        }

        for ancestor in relative_path.ancestors() {
            let ancestor_str = Self::normalize_relative_path(ancestor);
            if ancestor_str.is_empty() {
                continue;
            }
            if patterns
                .iter()
                .any(|pattern| pattern.matches(&ancestor_str))
            {
                return true;
            }
        }

        false
    }

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

    /// check if a relative path should be excluded from archiving
    pub fn should_exclude_relative(&self, relative_path: &Path) -> bool {
        if relative_path.as_os_str().is_empty() {
            return false;
        }

        let filename = relative_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        if Self::matches_patterns(&self.custom_excludes, filename, relative_path) {
            return true;
        }

        if self.use_defaults
            && Self::matches_patterns(&COMPILED_GARBAGE_PATTERNS, filename, relative_path)
        {
            return true;
        }

        false
    }

    /// check if a path should be excluded, based on its relative path to a root
    pub fn should_exclude_path(&self, root: &Path, path: &Path) -> bool {
        let relative_path = path.strip_prefix(root).unwrap_or(path);
        self.should_exclude_relative(relative_path)
    }

    /// check if a path should be included in archiving (inverse of should_exclude)
    pub fn should_include(&self, path: &Path) -> bool {
        !self.should_exclude(path)
    }

    /// check if a relative path should be included in archiving
    pub fn should_include_relative(&self, relative_path: &Path) -> bool {
        !self.should_exclude_relative(relative_path)
    }

    /// check if a path should be included, based on its relative path to a root
    pub fn should_include_path(&self, root: &Path, path: &Path) -> bool {
        !self.should_exclude_path(root, path)
    }

    /// walk a directory tree while applying filter pruning
    pub fn walk_entries<'a>(
        &'a self,
        root: &'a Path,
    ) -> impl Iterator<Item = walkdir::Result<walkdir::DirEntry>> + 'a {
        WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(move |entry| self.should_include_path(root, entry.path()))
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
        assert!(filter.should_exclude(Path::new("__MACOSX")));
        assert!(filter.should_exclude(Path::new(".AppleDouble")));
        assert!(filter.should_exclude(Path::new("lost+found")));
        assert!(filter.should_exclude(Path::new(".mypy_cache")));
        assert!(filter.should_exclude(Path::new(".ruff_cache")));
        assert!(filter.should_exclude(Path::new("npm-debug.log")));
        assert!(filter.should_exclude(Path::new("target")));
        assert!(filter.should_exclude(Path::new(".cache")));
        assert!(filter.should_exclude(Path::new(".venv")));
        assert!(filter.should_exclude(Path::new("__pypackages__")));
        assert!(filter.should_exclude(Path::new("CMakeCache.txt")));

        // Test pattern matching
        assert!(filter.should_exclude(Path::new("._resource_fork")));
        assert!(filter.should_exclude(Path::new("file.tmp")));
        assert!(filter.should_exclude(Path::new("backup~")));
        assert!(filter.should_exclude(Path::new(".#lockfile")));

        // Test normal files are not excluded
        assert!(!filter.should_exclude(Path::new("README.md")));
        assert!(!filter.should_exclude(Path::new("src/main.rs")));
        assert!(!filter.should_exclude(Path::new("Cargo.toml")));
        assert!(!filter.should_exclude(Path::new(".env")));

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
    fn test_sensitive_patterns_match_paths() -> Result<()> {
        let custom_patterns = SENSITIVE_FILES
            .iter()
            .map(|pattern| (*pattern).to_string())
            .collect::<Vec<_>>();
        let filter = FileFilter::new(true, &custom_patterns)?;

        assert!(filter.should_exclude_relative(Path::new(".env")));
        assert!(filter.should_exclude_relative(Path::new("config/.env.local")));
        assert!(filter.should_exclude_relative(Path::new(".ssh/id_rsa")));
        assert!(filter.should_exclude_relative(Path::new("config/.aws/credentials")));
        assert!(filter.should_exclude_relative(Path::new("keys/server.pem")));
        assert!(filter.should_exclude_relative(Path::new("keys/key.p12")));
        assert!(filter.should_exclude_relative(Path::new("vault/passwords.kdbx")));

        assert!(!filter.should_exclude_relative(Path::new("docs/notes.txt")));

        Ok(())
    }

    #[test]
    fn test_path_based_matching() -> Result<()> {
        let filter = FileFilter::new(true, &[])?;
        let root = Path::new("project");

        assert!(filter.should_exclude_path(root, Path::new("project/.git/config")));
        assert!(filter.should_exclude_path(root, Path::new("project/vendor/.git/config")));
        assert!(filter.should_exclude_path(root, Path::new("project/target/debug/app")));
        assert!(!filter.should_exclude_path(root, Path::new("project/src/main.rs")));

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
