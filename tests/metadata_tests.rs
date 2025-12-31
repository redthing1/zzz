//! Metadata-focused tests for redaction and defaults.

use assert_cmd::cargo::cargo_bin_cmd;
use assert_cmd::Command;
use filetime::FileTime;
use std::{fs, path::Path};
use tempfile::TempDir;

type Result<T> = anyhow::Result<T>;

fn zzz_cmd() -> Command {
    cargo_bin_cmd!("zzz")
}

struct TarHeaderInfo {
    mtime: u64,
    has_xattr: bool,
    mode: u32,
    uid: u64,
    gid: u64,
    username: Option<String>,
    groupname: Option<String>,
}

fn read_zstd_tar_header_info(path: &Path) -> Result<TarHeaderInfo> {
    use std::fs::File;
    use tar::Archive;
    use zstd::stream::read::Decoder as ZstdDecoder;

    let file = File::open(path)?;
    let decoder = ZstdDecoder::new(file)?;
    let mut archive = Archive::new(decoder);
    let mut entries = archive.entries()?;
    let mut entry = entries
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing tar entries"))??;

    let mtime = entry.header().mtime()?;
    let mode = entry.header().mode()?;
    let uid = entry.header().uid()?;
    let gid = entry.header().gid()?;
    let username = entry
        .header()
        .username()
        .map_err(|e| anyhow::anyhow!("invalid tar username: {e}"))?
        .map(|name| name.to_string());
    let groupname = entry
        .header()
        .groupname()
        .map_err(|e| anyhow::anyhow!("invalid tar groupname: {e}"))?
        .map(|name| name.to_string());
    let mut has_xattr = false;

    if let Some(extensions) = entry.pax_extensions()? {
        for extension in extensions {
            let extension = extension?;
            let key = extension.key()?;
            if key.starts_with("SCHILY.xattr.") {
                has_xattr = true;
                break;
            }
        }
    }

    Ok(TarHeaderInfo {
        mtime,
        has_xattr,
        mode,
        uid,
        gid,
        username,
        groupname,
    })
}

fn read_zstd_tar_metadata(path: &Path) -> Result<(u64, bool)> {
    let info = read_zstd_tar_header_info(path)?;
    Ok((info.mtime, info.has_xattr))
}

fn expected_zip_timestamp(path: &Path) -> Option<zip::DateTime> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    let dt = time::OffsetDateTime::from(modified);
    let month = u8::from(dt.month());
    let second = dt.second() - (dt.second() % 2);
    zip::DateTime::from_date_and_time(
        dt.year() as u16,
        month,
        dt.day(),
        dt.hour(),
        dt.minute(),
        second,
    )
    .ok()
}

fn read_gzip_header_mtime(path: &Path) -> Result<u32> {
    use std::fs::File;
    use std::io::Read;

    let file = File::open(path)?;
    let mut decoder = flate2::read::GzDecoder::new(file);
    let mut buffer = [0u8; 1];
    let _ = decoder.read(&mut buffer)?;
    let header = decoder
        .header()
        .ok_or_else(|| anyhow::anyhow!("missing gzip header"))?;
    Ok(header.mtime())
}

fn file_mtime_seconds(path: &Path) -> Result<i64> {
    let metadata = fs::metadata(path)?;
    let mtime = FileTime::from_last_modification_time(&metadata);
    Ok(mtime.unix_seconds())
}

#[cfg(unix)]
fn file_mode_bits(path: &Path) -> Result<u32> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = fs::metadata(path)?;
    Ok(metadata.permissions().mode() & 0o7777)
}

fn zip_entry_mtime_seconds(archive_path: &Path) -> Result<Option<i64>> {
    use std::fs::File;
    use zip::ZipArchive;

    let file = File::open(archive_path)?;
    let mut archive = ZipArchive::new(file)?;
    let entry = archive.by_index(0)?;
    let Some(last_modified) = entry.last_modified() else {
        return Ok(None);
    };
    let Ok(offset_time) = time::OffsetDateTime::try_from(last_modified) else {
        return Ok(None);
    };
    let system_time: std::time::SystemTime = offset_time.into();
    let duration = system_time
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| anyhow::anyhow!("mtime before epoch: {e}"))?;
    Ok(Some(duration.as_secs() as i64))
}

#[cfg(unix)]
fn zip_entry_mode(archive_path: &Path) -> Result<Option<u32>> {
    use std::fs::File;
    use zip::ZipArchive;

    let file = File::open(archive_path)?;
    let mut archive = ZipArchive::new(file)?;
    let entry = archive.by_index(0)?;
    Ok(entry.unix_mode())
}

fn sevenz_entry_mtime_seconds(archive_path: &Path) -> Result<Option<i64>> {
    use sevenz_rust::{Password, SevenZReader};

    let reader = SevenZReader::open(archive_path, Password::empty())?;
    let entry = reader.archive().files.iter().find(|entry| entry.has_stream);
    let Some(entry) = entry else {
        return Ok(None);
    };
    if !entry.has_last_modified_date {
        return Ok(None);
    }
    let system_time = std::time::SystemTime::from(entry.last_modified_date);
    let duration = system_time
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| anyhow::anyhow!("mtime before epoch: {e}"))?;
    Ok(Some(duration.as_secs() as i64))
}

#[test]
fn test_redact_strips_timestamps_tar_zstd() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("note.txt");
    fs::write(&source_file, "metadata test")?;

    let default_archive = temp_dir.path().join("default.zst");
    zzz_cmd()
        .args(["compress", "-f", "zst", "-o"])
        .arg(&default_archive)
        .arg(&source_file)
        .assert()
        .success();
    let (default_mtime, _) = read_zstd_tar_metadata(&default_archive)?;
    assert!(default_mtime > 0);

    let redacted_archive = temp_dir.path().join("redacted.zst");
    zzz_cmd()
        .args(["compress", "--redact", "-f", "zst", "-o"])
        .arg(&redacted_archive)
        .arg(&source_file)
        .assert()
        .success();
    let (redacted_mtime, _) = read_zstd_tar_metadata(&redacted_archive)?;
    assert_eq!(redacted_mtime, 0);

    Ok(())
}

#[test]
fn test_strip_timestamps_flag_tar_zstd() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("note.txt");
    fs::write(&source_file, "metadata test")?;

    let archive_path = temp_dir.path().join("stripped.zst");
    zzz_cmd()
        .args(["compress", "--strip-timestamps", "-f", "zst", "-o"])
        .arg(&archive_path)
        .arg(&source_file)
        .assert()
        .success();

    let (mtime, _) = read_zstd_tar_metadata(&archive_path)?;
    assert_eq!(mtime, 0);

    Ok(())
}

#[cfg(unix)]
#[test]
fn test_default_strips_xattrs_in_tar_zstd() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("xattr.txt");
    fs::write(&source_file, "xattr test")?;

    if xattr::set(&source_file, "user.zzz_test", b"secret").is_err() {
        return Ok(());
    }

    let archive_path = temp_dir.path().join("xattr.zst");
    zzz_cmd()
        .args(["compress", "-f", "zst", "-o"])
        .arg(&archive_path)
        .arg(&source_file)
        .assert()
        .success();

    let (_, has_xattr) = read_zstd_tar_metadata(&archive_path)?;
    assert!(!has_xattr);

    Ok(())
}

#[cfg(unix)]
#[test]
fn test_extract_strips_xattrs_by_default_tar_zstd() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("xattr.txt");
    fs::write(&source_file, "xattr test")?;

    if xattr::set(&source_file, "user.zzz_test", b"secret").is_err() {
        return Ok(());
    }

    let archive_path = temp_dir.path().join("xattr.zst");
    zzz_cmd()
        .args(["compress", "--keep-xattrs", "-f", "zst", "-o"])
        .arg(&archive_path)
        .arg(&source_file)
        .assert()
        .success();

    let extract_dir = temp_dir.path().join("extract_default");
    zzz_cmd()
        .args(["extract", "-C"])
        .arg(&extract_dir)
        .arg(&archive_path)
        .assert()
        .success();

    let extracted = extract_dir.join("xattr.txt");
    let extracted_value = xattr::get(&extracted, "user.zzz_test")?;
    assert!(extracted_value.is_none());

    Ok(())
}

#[cfg(unix)]
#[test]
fn test_extract_keep_xattrs_tar_zstd() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("xattr.txt");
    fs::write(&source_file, "xattr test")?;

    if xattr::set(&source_file, "user.zzz_test", b"secret").is_err() {
        return Ok(());
    }

    let archive_path = temp_dir.path().join("xattr.zst");
    zzz_cmd()
        .args(["compress", "--keep-xattrs", "-f", "zst", "-o"])
        .arg(&archive_path)
        .arg(&source_file)
        .assert()
        .success();

    let extract_dir = temp_dir.path().join("extract_keep");
    zzz_cmd()
        .args(["extract", "--keep-xattrs", "-C"])
        .arg(&extract_dir)
        .arg(&archive_path)
        .assert()
        .success();

    let extracted = extract_dir.join("xattr.txt");
    let extracted_value = xattr::get(&extracted, "user.zzz_test")?;
    assert_eq!(extracted_value.as_deref(), Some(b"secret".as_slice()));

    Ok(())
}

#[test]
fn test_default_strips_ownership_in_tar_zstd() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("owner.txt");
    fs::write(&source_file, "ownership test")?;

    let archive_path = temp_dir.path().join("owner.zst");
    zzz_cmd()
        .args(["compress", "-f", "zst", "-o"])
        .arg(&archive_path)
        .arg(&source_file)
        .assert()
        .success();

    let info = read_zstd_tar_header_info(&archive_path)?;
    assert_eq!(info.uid, 0);
    assert_eq!(info.gid, 0);
    assert!(info.username.unwrap_or_default().is_empty());
    assert!(info.groupname.unwrap_or_default().is_empty());

    Ok(())
}

#[cfg(unix)]
#[test]
fn test_keep_ownership_tar_zstd() -> Result<()> {
    use std::os::unix::fs::MetadataExt;

    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("owner.txt");
    fs::write(&source_file, "ownership test")?;

    let metadata = fs::metadata(&source_file)?;
    let uid = metadata.uid() as u64;
    let gid = metadata.gid() as u64;

    let archive_path = temp_dir.path().join("owner_keep.zst");
    zzz_cmd()
        .args(["compress", "--keep-ownership", "-f", "zst", "-o"])
        .arg(&archive_path)
        .arg(&source_file)
        .assert()
        .success();

    let info = read_zstd_tar_header_info(&archive_path)?;
    assert_eq!(info.uid, uid);
    assert_eq!(info.gid, gid);

    Ok(())
}

#[cfg(unix)]
#[test]
fn test_keep_permissions_tar_zstd() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("mode.txt");
    fs::write(&source_file, "mode test")?;
    fs::set_permissions(&source_file, fs::Permissions::from_mode(0o700))?;

    let archive_path = temp_dir.path().join("mode_keep.zst");
    zzz_cmd()
        .args(["compress", "--keep-permissions", "-f", "zst", "-o"])
        .arg(&archive_path)
        .arg(&source_file)
        .assert()
        .success();

    let info = read_zstd_tar_header_info(&archive_path)?;
    assert_eq!(info.mode & 0o777, 0o700);

    Ok(())
}

#[cfg(unix)]
#[test]
fn test_keep_permissions_zip() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("mode.txt");
    fs::write(&source_file, "mode test")?;
    fs::set_permissions(&source_file, fs::Permissions::from_mode(0o700))?;

    let archive_path = temp_dir.path().join("mode_keep.zip");
    zzz_cmd()
        .args(["compress", "--keep-permissions", "-f", "zip", "-o"])
        .arg(&archive_path)
        .arg(&source_file)
        .assert()
        .success();

    let mode = zip_entry_mode(&archive_path)?.ok_or_else(|| {
        anyhow::anyhow!("zip entry missing unix mode for {}", archive_path.display())
    })?;
    assert_eq!(mode & 0o777, 0o700);

    Ok(())
}

#[cfg(unix)]
#[test]
fn test_redact_overrides_keep_xattrs_tar_zstd() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("xattr.txt");
    fs::write(&source_file, "xattr test")?;

    if xattr::set(&source_file, "user.zzz_test", b"secret").is_err() {
        return Ok(());
    }

    let archive_path = temp_dir.path().join("redacted_xattr.zst");
    zzz_cmd()
        .args(["compress", "--keep-xattrs", "--redact", "-f", "zst", "-o"])
        .arg(&archive_path)
        .arg(&source_file)
        .assert()
        .success();

    let info = read_zstd_tar_header_info(&archive_path)?;
    assert!(!info.has_xattr);

    Ok(())
}

#[test]
fn test_redact_strips_timestamps_zip() -> Result<()> {
    use std::fs::File;
    use zip::ZipArchive;

    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("zip.txt");
    fs::write(&source_file, "zip metadata test")?;

    let archive_path = temp_dir.path().join("redacted.zip");
    zzz_cmd()
        .args(["compress", "--redact", "-f", "zip", "-o"])
        .arg(&archive_path)
        .arg(&source_file)
        .assert()
        .success();

    let file = File::open(&archive_path)?;
    let mut archive = ZipArchive::new(file)?;
    let entry = archive.by_index(0)?;
    let timestamp = entry.last_modified().expect("zip entry missing timestamp");
    assert_eq!(timestamp.year(), 1980);

    Ok(())
}

#[test]
fn test_default_preserves_zip_timestamps() -> Result<()> {
    use std::fs::File;
    use zip::ZipArchive;

    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("zip.txt");
    fs::write(&source_file, "zip metadata test")?;

    let Some(expected) = expected_zip_timestamp(&source_file) else {
        return Ok(());
    };

    let archive_path = temp_dir.path().join("default.zip");
    zzz_cmd()
        .args(["compress", "-f", "zip", "-o"])
        .arg(&archive_path)
        .arg(&source_file)
        .assert()
        .success();

    let file = File::open(&archive_path)?;
    let mut archive = ZipArchive::new(file)?;
    let entry = archive.by_index(0)?;
    let timestamp = entry.last_modified().expect("zip entry missing timestamp");

    assert_eq!(timestamp.year(), expected.year());
    assert_eq!(timestamp.month(), expected.month());
    assert_eq!(timestamp.day(), expected.day());
    assert_eq!(timestamp.hour(), expected.hour());
    assert_eq!(timestamp.minute(), expected.minute());
    assert_eq!(timestamp.second(), expected.second());

    Ok(())
}

#[test]
fn test_strip_timestamps_flag_zip() -> Result<()> {
    use std::fs::File;
    use zip::ZipArchive;

    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("zip.txt");
    fs::write(&source_file, "zip metadata test")?;

    let archive_path = temp_dir.path().join("stripped.zip");
    zzz_cmd()
        .args(["compress", "--strip-timestamps", "-f", "zip", "-o"])
        .arg(&archive_path)
        .arg(&source_file)
        .assert()
        .success();

    let file = File::open(&archive_path)?;
    let mut archive = ZipArchive::new(file)?;
    let entry = archive.by_index(0)?;
    let timestamp = entry.last_modified().expect("zip entry missing timestamp");
    assert_eq!(timestamp.year(), 1980);

    Ok(())
}

#[test]
fn test_redact_strips_timestamps_sevenz() -> Result<()> {
    use sevenz_rust::{Password, SevenZReader};

    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("seven.txt");
    fs::write(&source_file, "7z metadata test")?;

    let archive_path = temp_dir.path().join("redacted.7z");
    zzz_cmd()
        .args(["compress", "--redact", "-f", "7z", "-o"])
        .arg(&archive_path)
        .arg(&source_file)
        .assert()
        .success();

    let reader = SevenZReader::open(&archive_path, Password::empty())?;
    let entry = reader
        .archive()
        .files
        .iter()
        .find(|entry| entry.has_stream)
        .ok_or_else(|| anyhow::anyhow!("missing 7z file entry"))?;

    assert!(!entry.has_creation_date);
    assert!(!entry.has_last_modified_date);
    assert!(!entry.has_access_date);

    Ok(())
}

#[test]
fn test_strip_timestamps_flag_sevenz() -> Result<()> {
    use sevenz_rust::{Password, SevenZReader};

    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("seven.txt");
    fs::write(&source_file, "7z metadata test")?;

    let archive_path = temp_dir.path().join("stripped.7z");
    zzz_cmd()
        .args(["compress", "--strip-timestamps", "-f", "7z", "-o"])
        .arg(&archive_path)
        .arg(&source_file)
        .assert()
        .success();

    let reader = SevenZReader::open(&archive_path, Password::empty())?;
    let entry = reader
        .archive()
        .files
        .iter()
        .find(|entry| entry.has_stream)
        .ok_or_else(|| anyhow::anyhow!("missing 7z file entry"))?;

    assert!(!entry.has_creation_date);
    assert!(!entry.has_last_modified_date);
    assert!(!entry.has_access_date);

    Ok(())
}

#[test]
fn test_default_preserves_gzip_mtime() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("note.txt");
    fs::write(&source_file, "gzip metadata test")?;

    let metadata = fs::metadata(&source_file)?;
    let modified = match metadata.modified() {
        Ok(modified) => modified,
        Err(_) => return Ok(()),
    };
    let duration = match modified.duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => duration,
        Err(_) => return Ok(()),
    };
    let expected = duration.as_secs().min(u64::from(u32::MAX)) as u32;

    let archive_path = temp_dir.path().join("note.txt.gz");
    zzz_cmd()
        .args(["compress", "-f", "gz", "-o"])
        .arg(&archive_path)
        .arg(&source_file)
        .assert()
        .success();

    let mtime = read_gzip_header_mtime(&archive_path)?;
    assert_eq!(mtime, expected);

    Ok(())
}

#[test]
fn test_strip_timestamps_flag_gzip_raw() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("note.txt");
    fs::write(&source_file, "gzip metadata test")?;

    let archive_path = temp_dir.path().join("note.txt.gz");
    zzz_cmd()
        .args(["compress", "--strip-timestamps", "-f", "gz", "-o"])
        .arg(&archive_path)
        .arg(&source_file)
        .assert()
        .success();

    let mtime = read_gzip_header_mtime(&archive_path)?;
    assert_eq!(mtime, 0);

    Ok(())
}

#[test]
fn test_extract_preserves_mtime_gzip_raw() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("note.txt");
    fs::write(&source_file, "gzip metadata test")?;

    let expected_secs = 1_600_000_000;
    filetime::set_file_mtime(&source_file, FileTime::from_unix_time(expected_secs, 0))?;

    let archive_path = temp_dir.path().join("note.txt.gz");
    zzz_cmd()
        .args(["compress", "-f", "gz", "-o"])
        .arg(&archive_path)
        .arg(&source_file)
        .assert()
        .success();

    let extract_dir = temp_dir.path().join("extract_gz");
    zzz_cmd()
        .args(["extract", "-C"])
        .arg(&extract_dir)
        .arg(&archive_path)
        .assert()
        .success();

    let extracted = extract_dir.join("note.txt");
    let actual = file_mtime_seconds(&extracted)?;
    assert_eq!(actual, expected_secs);

    Ok(())
}

#[test]
fn test_extract_strip_timestamps_gzip_raw() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("note.txt");
    fs::write(&source_file, "gzip metadata test")?;

    let expected_secs = 1_600_000_000;
    filetime::set_file_mtime(&source_file, FileTime::from_unix_time(expected_secs, 0))?;

    let archive_path = temp_dir.path().join("note.txt.gz");
    zzz_cmd()
        .args(["compress", "-f", "gz", "-o"])
        .arg(&archive_path)
        .arg(&source_file)
        .assert()
        .success();

    let extract_dir = temp_dir.path().join("extract_gz_strip");
    zzz_cmd()
        .args(["extract", "--strip-timestamps", "-C"])
        .arg(&extract_dir)
        .arg(&archive_path)
        .assert()
        .success();

    let extracted = extract_dir.join("note.txt");
    let actual = file_mtime_seconds(&extracted)?;
    assert_ne!(actual, expected_secs);

    Ok(())
}

#[test]
fn test_extract_preserves_mtime_tar_zstd() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("note.txt");
    fs::write(&source_file, "mtime test")?;

    let expected_secs = 1_600_000_000;
    filetime::set_file_mtime(&source_file, FileTime::from_unix_time(expected_secs, 0))?;

    let archive_path = temp_dir.path().join("mtime.zst");
    zzz_cmd()
        .args(["compress", "-f", "zst", "-o"])
        .arg(&archive_path)
        .arg(&source_file)
        .assert()
        .success();

    let extract_dir = temp_dir.path().join("extract_mtime");
    zzz_cmd()
        .args(["extract", "-C"])
        .arg(&extract_dir)
        .arg(&archive_path)
        .assert()
        .success();

    let extracted = extract_dir.join("note.txt");
    let extracted_secs = file_mtime_seconds(&extracted)?;
    assert_eq!(extracted_secs, expected_secs);

    Ok(())
}

#[cfg(unix)]
#[test]
fn test_extract_preserves_permissions_tar_zstd() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("mode.txt");
    fs::write(&source_file, "mode test")?;
    fs::set_permissions(&source_file, fs::Permissions::from_mode(0o700))?;

    let archive_path = temp_dir.path().join("mode_keep.zst");
    zzz_cmd()
        .args(["compress", "--keep-permissions", "-f", "zst", "-o"])
        .arg(&archive_path)
        .arg(&source_file)
        .assert()
        .success();

    let extract_dir = temp_dir.path().join("extract_mode");
    zzz_cmd()
        .args(["extract", "--keep-permissions", "-C"])
        .arg(&extract_dir)
        .arg(&archive_path)
        .assert()
        .success();

    let extracted = extract_dir.join("mode.txt");
    let mode = file_mode_bits(&extracted)?;
    assert_eq!(mode & 0o777, 0o700);

    Ok(())
}

#[cfg(unix)]
#[test]
fn test_extract_preserves_permissions_zip() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("mode.txt");
    fs::write(&source_file, "mode test")?;
    fs::set_permissions(&source_file, fs::Permissions::from_mode(0o700))?;

    let archive_path = temp_dir.path().join("mode_keep.zip");
    zzz_cmd()
        .args(["compress", "--keep-permissions", "-f", "zip", "-o"])
        .arg(&archive_path)
        .arg(&source_file)
        .assert()
        .success();

    let extract_dir = temp_dir.path().join("extract_mode_zip");
    zzz_cmd()
        .args(["extract", "--keep-permissions", "-C"])
        .arg(&extract_dir)
        .arg(&archive_path)
        .assert()
        .success();

    let extracted = extract_dir.join("mode.txt");
    let mode = file_mode_bits(&extracted)?;
    assert_eq!(mode & 0o777, 0o700);

    Ok(())
}

#[test]
fn test_extract_strip_timestamps_tar_zstd() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("note.txt");
    fs::write(&source_file, "mtime test")?;

    let expected_secs = 1_600_000_000;
    filetime::set_file_mtime(&source_file, FileTime::from_unix_time(expected_secs, 0))?;

    let archive_path = temp_dir.path().join("mtime.zst");
    zzz_cmd()
        .args(["compress", "-f", "zst", "-o"])
        .arg(&archive_path)
        .arg(&source_file)
        .assert()
        .success();

    let extract_dir = temp_dir.path().join("extract_strip");
    let start = std::time::SystemTime::now();
    zzz_cmd()
        .args(["extract", "--strip-timestamps", "-C"])
        .arg(&extract_dir)
        .arg(&archive_path)
        .assert()
        .success();

    let extracted = extract_dir.join("note.txt");
    let extracted_secs = file_mtime_seconds(&extracted)?;
    let start_secs = start
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(extracted_secs);

    assert_ne!(extracted_secs, expected_secs);
    assert!(extracted_secs >= start_secs.saturating_sub(2));

    Ok(())
}

#[test]
fn test_extract_preserves_mtime_zip() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("note.txt");
    fs::write(&source_file, "zip mtime test")?;

    let expected_secs = 1_600_000_000;
    filetime::set_file_mtime(&source_file, FileTime::from_unix_time(expected_secs, 0))?;

    let archive_path = temp_dir.path().join("note.zip");
    zzz_cmd()
        .args(["compress", "-f", "zip", "-o"])
        .arg(&archive_path)
        .arg(&source_file)
        .assert()
        .success();

    let expected = match zip_entry_mtime_seconds(&archive_path)? {
        Some(expected) => expected,
        None => return Ok(()),
    };

    let extract_dir = temp_dir.path().join("extract_zip");
    zzz_cmd()
        .args(["extract", "-C"])
        .arg(&extract_dir)
        .arg(&archive_path)
        .assert()
        .success();

    let extracted = extract_dir.join("note.txt");
    let extracted_secs = file_mtime_seconds(&extracted)?;
    assert_eq!(extracted_secs, expected);

    Ok(())
}

#[test]
fn test_extract_strip_timestamps_zip() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("note.txt");
    fs::write(&source_file, "zip mtime test")?;

    let expected_secs = 1_600_000_000;
    filetime::set_file_mtime(&source_file, FileTime::from_unix_time(expected_secs, 0))?;

    let archive_path = temp_dir.path().join("note.zip");
    zzz_cmd()
        .args(["compress", "-f", "zip", "-o"])
        .arg(&archive_path)
        .arg(&source_file)
        .assert()
        .success();

    let expected = match zip_entry_mtime_seconds(&archive_path)? {
        Some(expected) => expected,
        None => return Ok(()),
    };

    let extract_dir = temp_dir.path().join("extract_zip_strip");
    let start = std::time::SystemTime::now();
    zzz_cmd()
        .args(["extract", "--strip-timestamps", "-C"])
        .arg(&extract_dir)
        .arg(&archive_path)
        .assert()
        .success();

    let extracted = extract_dir.join("note.txt");
    let extracted_secs = file_mtime_seconds(&extracted)?;
    let start_secs = start
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(extracted_secs);

    assert_ne!(extracted_secs, expected);
    assert!(extracted_secs >= start_secs.saturating_sub(2));

    Ok(())
}

#[test]
fn test_extract_preserves_mtime_sevenz() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("note.txt");
    fs::write(&source_file, "7z mtime test")?;

    let expected_secs = 1_600_000_000;
    filetime::set_file_mtime(&source_file, FileTime::from_unix_time(expected_secs, 0))?;

    let archive_path = temp_dir.path().join("note.7z");
    zzz_cmd()
        .args(["compress", "-f", "7z", "-o"])
        .arg(&archive_path)
        .arg(&source_file)
        .assert()
        .success();

    let expected = match sevenz_entry_mtime_seconds(&archive_path)? {
        Some(expected) => expected,
        None => return Ok(()),
    };

    let extract_dir = temp_dir.path().join("extract_7z");
    zzz_cmd()
        .args(["extract", "-C"])
        .arg(&extract_dir)
        .arg(&archive_path)
        .assert()
        .success();

    let extracted = extract_dir.join("note.txt");
    let extracted_secs = file_mtime_seconds(&extracted)?;
    assert_eq!(extracted_secs, expected);

    Ok(())
}

#[test]
fn test_extract_strip_timestamps_sevenz() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("note.txt");
    fs::write(&source_file, "7z mtime test")?;

    let expected_secs = 1_600_000_000;
    filetime::set_file_mtime(&source_file, FileTime::from_unix_time(expected_secs, 0))?;

    let archive_path = temp_dir.path().join("note.7z");
    zzz_cmd()
        .args(["compress", "-f", "7z", "-o"])
        .arg(&archive_path)
        .arg(&source_file)
        .assert()
        .success();

    let expected = match sevenz_entry_mtime_seconds(&archive_path)? {
        Some(expected) => expected,
        None => return Ok(()),
    };

    let extract_dir = temp_dir.path().join("extract_7z_strip");
    let start = std::time::SystemTime::now();
    zzz_cmd()
        .args(["extract", "--strip-timestamps", "-C"])
        .arg(&extract_dir)
        .arg(&archive_path)
        .assert()
        .success();

    let extracted = extract_dir.join("note.txt");
    let extracted_secs = file_mtime_seconds(&extracted)?;
    let start_secs = start
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(extracted_secs);

    assert_ne!(extracted_secs, expected);
    assert!(extracted_secs >= start_secs.saturating_sub(2));

    Ok(())
}
