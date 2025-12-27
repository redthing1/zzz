//! Shared tarball helpers for tar-based formats.

use crate::{
    filter::FileFilter,
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
    path::{Path, PathBuf},
};
use tar::{Archive, Builder, EntryType, HeaderMode};

const NORMALIZED_FILE_MODE: u32 = 0o644;
const NORMALIZED_DIR_MODE: u32 = 0o755;
const PAX_XATTR_PREFIX: &str = "SCHILY.xattr.";

#[derive(Debug, Clone, Copy)]
pub struct BuildOptions {
    pub normalize_ownership: bool,
    pub apply_filter_to_single_file: bool,
    pub directory_slash: bool,
    pub set_mtime_for_single_file: bool,
}

#[cfg(unix)]
fn append_xattrs<W: Write>(builder: &mut Builder<W>, path: &Path) -> Result<()> {
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    let xattrs = xattr::list(path)
        .with_context(|| format!("Failed to list xattrs for {}", path.display()))?;

    for name in xattrs {
        let name_str = name.to_str().ok_or_else(|| {
            anyhow::anyhow!(
                "Non-UTF-8 xattr name on {} (set --keep-xattrs to false to skip)",
                path.display()
            )
        })?;
        let value = xattr::get(path, &name).with_context(|| {
            format!("Failed to read xattr '{}' for {}", name_str, path.display())
        })?;
        let Some(value) = value else {
            continue;
        };
        let key = format!("{}{}", PAX_XATTR_PREFIX, name_str);
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
    normalize_ownership: bool,
    set_mtime: bool,
) -> Result<()> {
    if normalize_ownership {
        header.set_uid(0);
        header.set_gid(0);
        header.set_username("")?;
        header.set_groupname("")?;
    } else {
        #[cfg(unix)]
        {
            header.set_uid(metadata.uid() as u64);
            header.set_gid(metadata.gid() as u64);
        }
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
    normalize_ownership: bool,
    set_mtime: bool,
) -> Result<tar::Header> {
    let mut header = tar::Header::new_gnu();
    header.set_size(metadata.len());
    header.set_mode(if options.normalize_permissions {
        NORMALIZED_FILE_MODE
    } else {
        #[cfg(unix)]
        {
            metadata.permissions().mode()
        }
        #[cfg(not(unix))]
        {
            NORMALIZED_FILE_MODE
        }
    });

    apply_header_normalization(&mut header, metadata, normalize_ownership, set_mtime)?;
    header.set_cksum();
    Ok(header)
}

fn create_dir_header(
    metadata: &std::fs::Metadata,
    options: &CompressionOptions,
    normalize_ownership: bool,
) -> Result<tar::Header> {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(EntryType::Directory);
    header.set_size(0);
    header.set_mode(if options.normalize_permissions {
        NORMALIZED_DIR_MODE
    } else {
        #[cfg(unix)]
        {
            metadata.permissions().mode()
        }
        #[cfg(not(unix))]
        {
            NORMALIZED_DIR_MODE
        }
    });

    apply_header_normalization(&mut header, metadata, normalize_ownership, true)?;
    header.set_cksum();
    Ok(header)
}

pub fn build_tarball<W: Write>(
    writer: W,
    input_path: &Path,
    options: &CompressionOptions,
    filter: &FileFilter,
    progress: Option<&Progress>,
    build_options: BuildOptions,
) -> Result<W> {
    let mut tar_builder = Builder::new(writer);
    tar_builder.mode(HeaderMode::Deterministic);

    let mut bytes_processed = 0u64;

    if input_path.is_file() {
        if build_options.apply_filter_to_single_file {
            if let Some(filename) = input_path.file_name() {
                if !filter.should_include_relative(Path::new(filename)) {
                    return Ok(tar_builder.into_inner()?);
                }
            }
        }

        if !options.strip_xattrs {
            append_xattrs(&mut tar_builder, input_path)?;
        }

        let file = File::open(input_path)
            .with_context(|| format!("Failed to open input file {}", input_path.display()))?;
        let metadata = file.metadata().with_context(|| {
            format!(
                "Failed to read metadata for input file {}",
                input_path.display()
            )
        })?;
        let mut header = create_file_header(
            &metadata,
            options,
            build_options.normalize_ownership,
            build_options.set_mtime_for_single_file,
        )?;

        let filename = input_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Could not determine filename from input path: {}",
                    input_path.display()
                )
            })?;
        tar_builder.append_data(&mut header, filename, file)?;

        bytes_processed += metadata.len();
        if let Some(progress) = progress {
            progress.update(bytes_processed);
        }

        return Ok(tar_builder.into_inner()?);
    }

    let root_name = input_path.file_name();
    let mut entries: Vec<_> = filter
        .walk_entries(input_path)
        .filter_map(|entry| entry.ok())
        .collect();

    if options.deterministic {
        entries.sort_by(|a, b| a.path().cmp(b.path()));
    }

    for entry in entries {
        let path = entry.path();
        let relative = path.strip_prefix(input_path).unwrap_or(path);
        let mut archive_path = PathBuf::new();
        if let Some(root) = root_name {
            archive_path.push(root);
        }
        if !relative.as_os_str().is_empty() {
            archive_path.push(relative);
        }

        if path.is_file() {
            if !options.strip_xattrs {
                append_xattrs(&mut tar_builder, path)?;
            }

            let file = File::open(path)
                .with_context(|| format!("Failed to open file for archiving {}", path.display()))?;
            let metadata = entry.metadata()?;
            let mut header =
                create_file_header(&metadata, options, build_options.normalize_ownership, true)?;
            tar_builder.append_data(&mut header, &archive_path, file)?;

            bytes_processed += metadata.len();
            if let Some(progress) = progress {
                progress.update(bytes_processed);
            }
        } else if path.is_dir() {
            if archive_path.as_os_str().is_empty() {
                continue;
            }

            if !options.strip_xattrs {
                append_xattrs(&mut tar_builder, path)?;
            }

            let metadata = entry.metadata()?;
            let mut header =
                create_dir_header(&metadata, options, build_options.normalize_ownership)?;
            if build_options.directory_slash {
                let mut dir_path = archive_path.to_string_lossy().to_string();
                if !dir_path.ends_with('/') {
                    dir_path.push('/');
                }
                tar_builder.append_data(&mut header, dir_path.as_str(), std::io::empty())?;
            } else {
                tar_builder.append_data(&mut header, &archive_path, std::io::empty())?;
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
    archive.set_unpack_xattrs(!options.strip_xattrs);
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
            progress.set_position(entry_count);
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
