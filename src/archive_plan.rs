//! Format-independent archive layout planning.
//!
//! Compression formats serialize an `ArchivePlan`; they do not decide how input
//! paths map into archive paths. Keeping that policy here makes multi-source
//! archives consistent across tar-based formats, ZIP, and 7-Zip.

use crate::{filter::FileFilter, policy::SymlinkPolicy, utils, Result};
use anyhow::Context;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveSourceKind {
    File,
    Directory,
}

#[derive(Debug, Clone)]
pub struct ArchiveSource {
    pub input_path: PathBuf,
    pub canonical_path: PathBuf,
    pub archive_root: PathBuf,
    pub kind: ArchiveSourceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlannedEntryKind {
    File,
    Directory,
}

#[derive(Debug, Clone)]
pub struct PlannedEntry {
    pub source_index: usize,
    pub disk_path: PathBuf,
    pub archive_path: PathBuf,
    pub kind: PlannedEntryKind,
    pub size: u64,
}

impl PlannedEntry {
    pub fn is_file(&self) -> bool {
        self.kind == PlannedEntryKind::File
    }

    pub fn is_dir(&self) -> bool {
        self.kind == PlannedEntryKind::Directory
    }
}

#[derive(Debug, Clone)]
pub struct ArchivePlan {
    pub sources: Vec<ArchiveSource>,
    pub entries: Vec<PlannedEntry>,
    pub total_size: u64,
    pub skipped_symlinks: usize,
}

impl ArchivePlan {
    pub fn from_paths(
        input_paths: &[PathBuf],
        filter: &FileFilter,
        symlink_policy: SymlinkPolicy,
        deterministic: bool,
    ) -> Result<Self> {
        if input_paths.is_empty() {
            return Err(anyhow::anyhow!("at least one input path is required"));
        }

        let mut plan = Self {
            sources: Vec::new(),
            entries: Vec::new(),
            total_size: 0,
            skipped_symlinks: 0,
        };
        let mut seen_archive_paths = HashMap::new();

        for input_path in input_paths {
            let source = plan_source(input_path, symlink_policy)?;
            let source_index = plan.sources.len();
            let (source_entries, skipped_symlinks) =
                plan_entries_for_source(source_index, &source, filter, symlink_policy)?;
            plan.skipped_symlinks += skipped_symlinks;

            for entry in source_entries {
                reject_archive_path_collision(
                    &mut seen_archive_paths,
                    &entry.archive_path,
                    &entry.disk_path,
                )?;
                plan.total_size += entry.size;
                plan.entries.push(entry);
            }

            plan.sources.push(source);
        }

        if deterministic {
            plan.entries.sort_by(|left, right| {
                left.archive_path
                    .cmp(&right.archive_path)
                    .then_with(|| left.disk_path.cmp(&right.disk_path))
            });
        }

        Ok(plan)
    }

    pub fn single_raw_file_entry(
        &self,
        format_name: &str,
        archive_extension: &str,
    ) -> Result<Option<&PlannedEntry>> {
        if self.sources.len() != 1 || self.sources[0].kind != ArchiveSourceKind::File {
            return Err(anyhow::anyhow!(
                "raw .{archive_extension} output supports exactly one file input; use {format_name} archive output for multiple files or directories"
            ));
        }

        match self.entries.as_slice() {
            [] => Ok(None),
            [entry] if entry.is_file() => Ok(Some(entry)),
            _ => Err(anyhow::anyhow!(
                "raw .{archive_extension} output supports exactly one file input; use {format_name} archive output for multiple files or directories"
            )),
        }
    }
}

fn plan_source(input_path: &Path, symlink_policy: SymlinkPolicy) -> Result<ArchiveSource> {
    let symlink_metadata = std::fs::symlink_metadata(input_path).with_context(|| {
        format!(
            "Failed to read metadata for input path '{}'",
            input_path.display()
        )
    })?;

    if symlink_metadata.file_type().is_symlink() && symlink_policy == SymlinkPolicy::Skip {
        return Err(anyhow::anyhow!(
            "input path '{}' is a symlink; use --follow-symlinks",
            input_path.display()
        ));
    }

    let canonical_path = std::fs::canonicalize(input_path)
        .with_context(|| format!("Failed to resolve input path '{}'", input_path.display()))?;
    let metadata = std::fs::metadata(input_path).with_context(|| {
        format!(
            "Failed to read target metadata for input path '{}'",
            input_path.display()
        )
    })?;
    let kind = if metadata.is_file() {
        ArchiveSourceKind::File
    } else if metadata.is_dir() {
        ArchiveSourceKind::Directory
    } else {
        return Err(anyhow::anyhow!(
            "unsupported input path type '{}'",
            input_path.display()
        ));
    };

    let archive_root = archive_root_for_input(input_path, &canonical_path)?;

    Ok(ArchiveSource {
        input_path: input_path.to_path_buf(),
        canonical_path,
        archive_root,
        kind,
    })
}

fn archive_root_for_input(input_path: &Path, canonical_path: &Path) -> Result<PathBuf> {
    let root_name = input_path
        .file_name()
        .or_else(|| canonical_path.file_name())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "could not determine archive root for input path '{}'",
                input_path.display()
            )
        })?;

    let mut archive_root = PathBuf::new();
    archive_root.push(root_name);
    Ok(archive_root)
}

fn plan_entries_for_source(
    source_index: usize,
    source: &ArchiveSource,
    filter: &FileFilter,
    symlink_policy: SymlinkPolicy,
) -> Result<(Vec<PlannedEntry>, usize)> {
    match source.kind {
        ArchiveSourceKind::File => Ok((plan_file_source(source_index, source, filter)?, 0)),
        ArchiveSourceKind::Directory => {
            plan_directory_source(source_index, source, filter, symlink_policy)
        }
    }
}

fn plan_file_source(
    source_index: usize,
    source: &ArchiveSource,
    filter: &FileFilter,
) -> Result<Vec<PlannedEntry>> {
    if !filter.should_include_relative(&source.archive_root) {
        return Ok(Vec::new());
    }

    let metadata = std::fs::metadata(&source.input_path).with_context(|| {
        format!(
            "Failed to read metadata for input file '{}'",
            source.input_path.display()
        )
    })?;

    Ok(vec![PlannedEntry {
        source_index,
        disk_path: source.input_path.clone(),
        archive_path: source.archive_root.clone(),
        kind: PlannedEntryKind::File,
        size: metadata.len(),
    }])
}

fn plan_directory_source(
    source_index: usize,
    source: &ArchiveSource,
    filter: &FileFilter,
    symlink_policy: SymlinkPolicy,
) -> Result<(Vec<PlannedEntry>, usize)> {
    let mut entries = Vec::new();
    let mut skipped_symlinks = 0;

    for entry in
        filter.walk_entries_with_follow(&source.input_path, symlink_policy.follows_targets())
    {
        let entry = entry?;
        let path = entry.path();

        if entry.path_is_symlink() {
            match symlink_policy {
                SymlinkPolicy::Skip => {
                    skipped_symlinks += 1;
                    continue;
                }
                SymlinkPolicy::FollowWithinRoot => {
                    utils::ensure_symlink_within_root(&source.canonical_path, path)?;
                }
                SymlinkPolicy::FollowAllowEscape => {}
            }
        }

        let metadata = entry
            .metadata()
            .with_context(|| format!("Failed to read metadata for '{}'", entry.path().display()))?;
        let kind = if metadata.is_file() {
            PlannedEntryKind::File
        } else if metadata.is_dir() {
            PlannedEntryKind::Directory
        } else {
            continue;
        };

        let relative = path.strip_prefix(&source.input_path).unwrap_or(path);
        let mut archive_path = source.archive_root.clone();
        if !relative.as_os_str().is_empty() {
            archive_path.push(relative);
        }

        entries.push(PlannedEntry {
            source_index,
            disk_path: path.to_path_buf(),
            archive_path,
            kind,
            size: if kind == PlannedEntryKind::File {
                metadata.len()
            } else {
                0
            },
        });
    }

    Ok((entries, skipped_symlinks))
}

fn reject_archive_path_collision(
    seen_archive_paths: &mut HashMap<PathBuf, PathBuf>,
    archive_path: &Path,
    disk_path: &Path,
) -> Result<()> {
    if let Some(previous_disk_path) = seen_archive_paths.get(archive_path) {
        return Err(anyhow::anyhow!(
            "archive path collision: both '{}' and '{}' would be stored as '{}'",
            previous_disk_path.display(),
            disk_path.display(),
            archive_path.display()
        ));
    }

    seen_archive_paths.insert(archive_path.to_path_buf(), disk_path.to_path_buf());
    Ok(())
}
