//! Shared tarball helpers for tar-based formats.

use crate::{
    archive_plan::{ArchivePlan, PlannedEntryKind},
    formats::{ArchiveEntry, CompressionOptions, ExtractionOptions},
    progress::Progress,
    utils, Result,
};
use anyhow::Context;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};
use tar::{Archive, Builder, EntryType, HeaderMode};

const NORMALIZED_FILE_MODE: u32 = 0o644;
const NORMALIZED_DIR_MODE: u32 = 0o755;
const PAX_XATTR_PREFIX: &str = "SCHILY.xattr.";

#[derive(Debug, Clone, Copy)]
pub struct BuildOptions {
    pub directory_slash: bool,
}

#[cfg(unix)]
fn append_xattrs<W: Write>(builder: &mut Builder<W>, path: &Path) -> Result<()> {
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    let xattrs = xattr::list(path)
        .with_context(|| format!("Failed to list xattrs for {}", path.display()))?;

    for name in xattrs {
        let name_str = name.to_str().ok_or_else(|| {
            anyhow::anyhow!(
                "Non-UTF-8 xattr name on {} (omit --preserve-xattrs to skip)",
                path.display()
            )
        })?;
        let value = xattr::get(path, &name).with_context(|| {
            format!("Failed to read xattr '{}' for {}", name_str, path.display())
        })?;
        let Some(value) = value else {
            continue;
        };
        let key = format!("{PAX_XATTR_PREFIX}{name_str}");
        entries.push((key, value));
    }

    if entries.is_empty() {
        return Ok(());
    }

    builder
        .append_pax_extensions(
            entries
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_slice())),
        )
        .with_context(|| format!("Failed to write xattrs for {}", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn append_xattrs<W: Write>(_builder: &mut Builder<W>, _path: &Path) -> Result<()> {
    Ok(())
}

fn apply_header_normalization(
    header: &mut tar::Header,
    metadata: &std::fs::Metadata,
    preserve_ownership: bool,
    set_mtime: bool,
) -> Result<()> {
    if preserve_ownership {
        #[cfg(unix)]
        {
            header.set_uid(metadata.uid() as u64);
            header.set_gid(metadata.gid() as u64);
        }
    } else {
        header.set_uid(0);
        header.set_gid(0);
        header.set_username("")?;
        header.set_groupname("")?;
    }

    if set_mtime {
        if let Ok(mtime) = metadata.modified() {
            if let Ok(duration) = mtime.duration_since(std::time::UNIX_EPOCH) {
                header.set_mtime(duration.as_secs());
            }
        }
    }

    Ok(())
}

fn create_file_header(
    metadata: &std::fs::Metadata,
    options: &CompressionOptions,
    set_mtime: bool,
) -> Result<tar::Header> {
    let mut header = tar::Header::new_gnu();
    header.set_size(metadata.len());
    header.set_mode(if options.preserve_permissions {
        #[cfg(unix)]
        {
            metadata.permissions().mode()
        }
        #[cfg(not(unix))]
        {
            NORMALIZED_FILE_MODE
        }
    } else {
        NORMALIZED_FILE_MODE
    });

    apply_header_normalization(&mut header, metadata, options.preserve_ownership, set_mtime)?;
    header.set_cksum();
    Ok(header)
}

fn create_dir_header(
    metadata: &std::fs::Metadata,
    options: &CompressionOptions,
    set_mtime: bool,
) -> Result<tar::Header> {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(EntryType::Directory);
    header.set_size(0);
    header.set_mode(if options.preserve_permissions {
        #[cfg(unix)]
        {
            metadata.permissions().mode()
        }
        #[cfg(not(unix))]
        {
            NORMALIZED_DIR_MODE
        }
    } else {
        NORMALIZED_DIR_MODE
    });

    apply_header_normalization(&mut header, metadata, options.preserve_ownership, set_mtime)?;
    header.set_cksum();
    Ok(header)
}

pub fn build_tarball<W: Write>(
    writer: W,
    plan: &ArchivePlan,
    options: &CompressionOptions,
    progress: Option<&Progress>,
    build_options: BuildOptions,
) -> Result<W> {
    let mut tar_builder = Builder::new(writer);
    tar_builder.mode(HeaderMode::Deterministic);

    let mut bytes_processed = 0u64;

    for entry in &plan.entries {
        let archive_path_str = utils::normalize_archive_path(&entry.archive_path);
        match entry.kind {
            PlannedEntryKind::File => {
                if options.preserve_xattrs {
                    append_xattrs(&mut tar_builder, &entry.disk_path)?;
                }

                let file = File::open(&entry.disk_path).with_context(|| {
                    format!(
                        "Failed to open file for archiving {}",
                        entry.disk_path.display()
                    )
                })?;
                let metadata = file.metadata()?;
                let mut header =
                    create_file_header(&metadata, options, options.preserve_timestamps)?;
                tar_builder.append_data(&mut header, archive_path_str.as_str(), file)?;

                bytes_processed += metadata.len();
                if let Some(progress) = progress {
                    progress.update(bytes_processed);
                }
            }
            PlannedEntryKind::Directory => {
                if options.preserve_xattrs {
                    append_xattrs(&mut tar_builder, &entry.disk_path)?;
                }

                let metadata = entry.disk_path.metadata()?;
                let mut header =
                    create_dir_header(&metadata, options, options.preserve_timestamps)?;
                if build_options.directory_slash {
                    let mut dir_path = archive_path_str;
                    if !dir_path.ends_with('/') {
                        dir_path.push('/');
                    }
                    tar_builder.append_data(&mut header, dir_path.as_str(), std::io::empty())?;
                } else {
                    tar_builder.append_data(
                        &mut header,
                        archive_path_str.as_str(),
                        std::io::empty(),
                    )?;
                }
            }
        }
    }

    Ok(tar_builder.into_inner()?)
}

pub fn extract_tarball<R: Read>(
    reader: R,
    output_dir: &Path,
    options: &ExtractionOptions,
    progress: Option<&Progress>,
) -> Result<()> {
    let mut archive = Archive::new(reader);
    archive.set_preserve_mtime(options.preserve_timestamps);
    archive.set_unpack_xattrs(options.preserve_xattrs);
    archive.set_preserve_permissions(options.preserve_permissions);
    archive.set_preserve_ownerships(options.preserve_ownership);
    std::fs::create_dir_all(output_dir)?;

    let mut entry_count = 0u64;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
        let target_path = match utils::prepare_extract_target(
            output_dir,
            &path,
            options.strip_components,
            options.overwrite,
            entry.header().entry_type().is_dir(),
        )? {
            utils::ExtractTarget::Target(target_path) => target_path,
            utils::ExtractTarget::SkipStrip => continue,
            utils::ExtractTarget::SkipExisting(target_path) => {
                return Err(anyhow::anyhow!(
                    "output file '{}' already exists. Use --overwrite to replace.",
                    target_path.display()
                ));
            }
        };

        if let Some(progress) = progress {
            if progress.is_verbose() {
                if entry.header().entry_type().is_dir() {
                    println!("  creating: {}", path.display());
                } else {
                    println!("  extracting: {}", path.display());
                }
            }
        }

        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        entry.unpack(&target_path)?;

        entry_count += 1;
        if let Some(progress) = progress {
            if progress.is_items() {
                progress.set_position(entry_count);
            }
        }
    }

    Ok(())
}

pub fn list_tarball<R: Read>(reader: R) -> Result<Vec<ArchiveEntry>> {
    let mut archive = Archive::new(reader);
    let mut entries = Vec::new();

    for entry in archive.entries()? {
        let entry = entry?;
        let path = entry.path()?.to_string_lossy().to_string();
        let size = entry.header().size()?;
        let is_file = entry.header().entry_type().is_file();

        entries.push(ArchiveEntry {
            path,
            size,
            is_file,
        });
    }

    Ok(entries)
}
