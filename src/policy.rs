use crate::{
    filter::FileFilter,
    formats::{CompressionOptions, ExtractionOptions},
    Result,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymlinkPolicy {
    Skip,
    FollowWithinRoot,
    FollowAllowEscape,
}

impl SymlinkPolicy {
    pub fn follows_targets(self) -> bool {
        !matches!(self, Self::Skip)
    }

    pub fn allows_escape(self) -> bool {
        matches!(self, Self::FollowAllowEscape)
    }
}

#[derive(Debug, Clone)]
pub struct FilterPolicy {
    pub use_default_excludes: bool,
    pub exclude_sensitive: bool,
    pub patterns: Vec<String>,
}

impl FilterPolicy {
    pub fn new(use_default_excludes: bool, exclude_sensitive: bool, patterns: Vec<String>) -> Self {
        Self {
            use_default_excludes,
            exclude_sensitive,
            patterns,
        }
    }
}

impl Default for FilterPolicy {
    fn default() -> Self {
        Self {
            use_default_excludes: true,
            exclude_sensitive: false,
            patterns: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompressPolicy {
    pub options: CompressionOptions,
    pub filters: FilterPolicy,
}

#[derive(Debug, Clone)]
pub struct ExtractPolicy {
    pub options: ExtractionOptions,
}

#[derive(Debug, Clone)]
pub struct CompressPolicyInputs {
    pub level: i32,
    pub threads: u32,
    pub password: Option<String>,
    pub preserve_ownership: bool,
    pub preserve_xattrs: bool,
    pub strip_timestamps: bool,
    pub follow_symlinks: bool,
    pub allow_symlink_escape: bool,
    pub exclude_sensitive: bool,
    pub use_default_excludes: bool,
    pub exclude_patterns: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ExtractPolicyInputs {
    pub overwrite: bool,
    pub strip_components: usize,
    pub password: Option<String>,
    pub preserve_ownership: bool,
    pub preserve_xattrs: bool,
    pub strip_timestamps: bool,
}

impl CompressPolicy {
    pub fn resolve(inputs: CompressPolicyInputs) -> Result<Self> {
        let symlink_policy =
            resolve_symlink_policy(inputs.follow_symlinks, inputs.allow_symlink_escape)?;
        let options = CompressionOptions {
            level: inputs.level,
            threads: inputs.threads,
            password: inputs.password,
            preserve_ownership: inputs.preserve_ownership,
            preserve_xattrs: inputs.preserve_xattrs,
            preserve_timestamps: !inputs.strip_timestamps,
            symlink_policy,
            ..Default::default()
        };
        let filters = FilterPolicy::new(
            inputs.use_default_excludes,
            inputs.exclude_sensitive,
            inputs.exclude_patterns,
        );
        Ok(Self { options, filters })
    }
}

impl ExtractPolicy {
    pub fn resolve(inputs: ExtractPolicyInputs) -> Self {
        let options = ExtractionOptions {
            overwrite: inputs.overwrite,
            strip_components: inputs.strip_components,
            password: inputs.password,
            preserve_ownership: inputs.preserve_ownership,
            preserve_xattrs: inputs.preserve_xattrs,
            preserve_timestamps: !inputs.strip_timestamps,
            ..Default::default()
        };
        Self { options }
    }
}

pub fn resolve_symlink_policy(
    follow_symlinks: bool,
    allow_symlink_escape: bool,
) -> Result<SymlinkPolicy> {
    match (follow_symlinks, allow_symlink_escape) {
        (false, false) => Ok(SymlinkPolicy::Skip),
        (true, false) => Ok(SymlinkPolicy::FollowWithinRoot),
        (true, true) => Ok(SymlinkPolicy::FollowAllowEscape),
        (false, true) => Err(anyhow::anyhow!(
            "--allow-symlink-escape requires --follow-symlinks"
        )),
    }
}

pub fn build_filter(policy: &FilterPolicy) -> Result<FileFilter> {
    FileFilter::new(policy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compress_policy_defaults() {
        let policy = CompressPolicy::resolve(CompressPolicyInputs {
            level: 19,
            threads: 0,
            password: None,
            preserve_ownership: false,
            preserve_xattrs: false,
            strip_timestamps: false,
            follow_symlinks: false,
            allow_symlink_escape: false,
            exclude_sensitive: false,
            use_default_excludes: true,
            exclude_patterns: Vec::new(),
        })
        .expect("compress policy should resolve");

        assert!(policy.options.preserve_permissions);
        assert!(policy.options.preserve_timestamps);
        assert!(!policy.options.preserve_ownership);
        assert!(!policy.options.preserve_xattrs);
        assert_eq!(policy.options.symlink_policy, SymlinkPolicy::Skip);
        assert!(policy.filters.use_default_excludes);
        assert!(!policy.filters.exclude_sensitive);
    }

    #[test]
    fn test_extract_policy_defaults() {
        let policy = ExtractPolicy::resolve(ExtractPolicyInputs {
            overwrite: false,
            strip_components: 0,
            password: None,
            preserve_ownership: false,
            preserve_xattrs: false,
            strip_timestamps: false,
        });

        assert!(policy.options.preserve_permissions);
        assert!(policy.options.preserve_timestamps);
        assert!(!policy.options.preserve_ownership);
        assert!(!policy.options.preserve_xattrs);
    }

    #[test]
    fn test_symlink_policy_resolution() {
        assert_eq!(
            resolve_symlink_policy(false, false).unwrap(),
            SymlinkPolicy::Skip
        );
        assert_eq!(
            resolve_symlink_policy(true, false).unwrap(),
            SymlinkPolicy::FollowWithinRoot
        );
        assert_eq!(
            resolve_symlink_policy(true, true).unwrap(),
            SymlinkPolicy::FollowAllowEscape
        );
        assert!(resolve_symlink_policy(false, true).is_err());
    }
}
