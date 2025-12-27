//! Tests for raw gzip/xz streams (non-tar)

use flate2::{write::GzEncoder, Compression};
use std::fs::{self, File};
use tempfile::TempDir;
use xz2::write::XzEncoder;
use zzz_arc::formats::{gz::GzipFormat, xz::XzFormat, CompressionFormat, ExtractionOptions};

type Result<T> = anyhow::Result<T>;

#[test]
fn test_raw_gzip_extract_and_list() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("file.txt");
    let archive_path = temp_dir.path().join("file.txt.gz");
    let extract_dir = temp_dir.path().join("extract");

    fs::write(&source_file, "raw gzip content")?;

    let output_file = File::create(&archive_path)?;
    let mut encoder = GzEncoder::new(output_file, Compression::default());
    let mut input = File::open(&source_file)?;
    std::io::copy(&mut input, &mut encoder)?;
    encoder.finish()?;

    let entries = GzipFormat::list(&archive_path)?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, "file.txt");
    assert!(entries[0].size > 0);

    fs::create_dir(&extract_dir)?;
    let options = ExtractionOptions::default();
    GzipFormat::extract(&archive_path, &extract_dir, &options, None)?;

    assert_eq!(
        fs::read_to_string(extract_dir.join("file.txt"))?,
        "raw gzip content"
    );

    Ok(())
}

#[test]
fn test_raw_xz_extract_and_list() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("file.txt");
    let archive_path = temp_dir.path().join("file.txt.xz");
    let extract_dir = temp_dir.path().join("extract");

    fs::write(&source_file, "raw xz content")?;

    let output_file = File::create(&archive_path)?;
    let mut encoder = XzEncoder::new(output_file, 6);
    let mut input = File::open(&source_file)?;
    std::io::copy(&mut input, &mut encoder)?;
    encoder.finish()?;

    let entries = XzFormat::list(&archive_path)?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, "file.txt");
    assert!(entries[0].size > 0);

    fs::create_dir(&extract_dir)?;
    let options = ExtractionOptions::default();
    XzFormat::extract(&archive_path, &extract_dir, &options, None)?;

    assert_eq!(
        fs::read_to_string(extract_dir.join("file.txt"))?,
        "raw xz content"
    );

    Ok(())
}
