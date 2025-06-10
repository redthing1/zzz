//! file filtering system with comprehensive garbage file exclusion

use std::path::Path;
use glob::Pattern;
use crate::Result;

/// comprehensive list of garbage files to exclude by default
pub const GARBAGE_FILES: &[&str] = &[
    // macOS system files
    ".DS_Store",
    "._*",                  // resource forks  
    ".Spotlight-V100",      // spotlight index
    ".Trashes",            // trash
    ".fseventsd",          // filesystem events
    ".VolumeIcon.icns",    // volume icons
    ".DocumentRevisions-V100", // document revisions
    ".TemporaryItems",     // temporary items
    
    // Windows system files
    "thumbs.db", "Thumbs.db",
    "desktop.ini", "Desktop.ini", 
    "ehthumbs.db", "ehthumbs_vista.db",
    "$RECYCLE.BIN", "System Volume Information",
    "hiberfil.sys", "pagefile.sys", "swapfile.sys",
    
    // Linux/Unix system files
    ".directory",          // KDE folder metadata
    ".trash", ".Trash-*",  // trash directories
    ".nfs*",              // NFS lock files
    
    // Development artifacts
    "__pycache__", "*.pyc", "*.pyo", "*.pyd",
    ".pytest_cache", ".coverage", ".tox",
    "node_modules", ".npm", ".yarn",
    ".git", ".svn", ".hg", ".bzr",
    "target/debug", "target/release", // Rust build dirs
    ".gradle", ".maven",
    ".vscode", ".idea",    // IDE files (configurable)
    
    // Temporary/backup files
    "*.tmp", "*.temp", "*.bak", "*.orig",
    "*~", ".#*", "#*#",    // editor backup files
    "*.swp", "*.swo",     // vim swap files
    ".*.sw?",             // vim swap file pattern
];

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
        Ok(Self { use_defaults, custom_excludes })
    }
    
    /// check if a path should be excluded from archiving
    pub fn should_exclude(&self, path: &Path) -> bool {
        // check custom patterns first
        for pattern in &self.custom_excludes {
            if pattern.matches_path(path) {
                return true;
            }
        }
        
        // check default garbage files if enabled
        if self.use_defaults {
            let filename = path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
                
            for garbage_pattern in GARBAGE_FILES {
                if let Ok(pattern) = Pattern::new(garbage_pattern) {
                    if pattern.matches(filename) {
                        return true;
                    }
                }
            }
        }
        
        false
    }
}