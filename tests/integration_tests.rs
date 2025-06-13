//! Integration tests for zzz compression functionality

use std::fs;
use std::path::Path;
use tempfile::TempDir;
use zzz_arc::filter::FileFilter;
use zzz_arc::formats::zstd::ZstdFormat;
use zzz_arc::formats::{CompressionFormat, CompressionOptions, ExtractionOptions};

type Result<T> = anyhow::Result<T>;

/// Helper function to create a test directory structure
fn create_test_structure(base: &Path) -> Result<()> {
    // Create files
    fs::write(base.join("file1.txt"), "Hello, World!")?;
    fs::write(
        base.join("file2.txt"),
        "This is a test file with more content.",
    )?;

    // Create subdirectory with files
    let subdir = base.join("subdir");
    fs::create_dir(&subdir)?;
    fs::write(subdir.join("nested.txt"), "Nested file content")?;

    // Create some garbage files that should be filtered
    fs::write(base.join(".DS_Store"), "macos junk")?;
    fs::write(base.join("thumbs.db"), "windows junk")?;

    // Create empty subdirectory
    fs::create_dir(base.join("empty_dir"))?;

    Ok(())
}

#[test]
fn test_compress_and_extract_single_file() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("test.txt");
    let archive_path = temp_dir.path().join("test.zst");
    let extract_dir = temp_dir.path().join("extracted");

    // Create source file
    fs::write(&source_file, "Single file test content")?;

    // Compress
    let options = CompressionOptions::default();
    let filter = FileFilter::new(true, &[])?;
    let stats = ZstdFormat::compress(&source_file, &archive_path, &options, &filter, None)?;

    // Verify archive was created
    assert!(archive_path.exists());
    assert!(stats.input_size > 0);
    assert!(stats.output_size > 0);

    // Extract
    fs::create_dir(&extract_dir)?;
    let extract_options = ExtractionOptions::default();
    ZstdFormat::extract(&archive_path, &extract_dir, &extract_options, None)?;

    // Verify extracted file
    let extracted_file = extract_dir.join("test.txt");
    assert!(extracted_file.exists());
    assert_eq!(
        fs::read_to_string(&extracted_file)?,
        "Single file test content"
    );

    Ok(())
}

#[test]
fn test_compress_and_extract_directory() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_dir = temp_dir.path().join("source");
    let archive_path = temp_dir.path().join("archive.zst");
    let extract_dir = temp_dir.path().join("extracted");

    // Create test structure
    fs::create_dir(&source_dir)?;
    create_test_structure(&source_dir)?;

    // Compress with default filtering (should exclude .DS_Store, thumbs.db)
    let options = CompressionOptions::default();
    let filter = FileFilter::new(true, &[])?;
    let stats = ZstdFormat::compress(&source_dir, &archive_path, &options, &filter, None)?;

    // Verify compression
    assert!(archive_path.exists());
    assert!(stats.input_size > 0);
    assert!(stats.output_size > 0);
    assert!(stats.compression_ratio > 0.0);

    // Extract
    fs::create_dir(&extract_dir)?;
    let extract_options = ExtractionOptions::default();
    ZstdFormat::extract(&archive_path, &extract_dir, &extract_options, None)?;

    // Verify extracted structure
    assert!(extract_dir.join("source/file1.txt").exists());
    assert!(extract_dir.join("source/file2.txt").exists());
    assert!(extract_dir.join("source/subdir/nested.txt").exists());
    assert!(extract_dir.join("source/empty_dir").exists());

    // Verify garbage files were filtered out
    assert!(!extract_dir.join("source/.DS_Store").exists());
    assert!(!extract_dir.join("source/thumbs.db").exists());

    // Verify file contents
    assert_eq!(
        fs::read_to_string(extract_dir.join("source/file1.txt"))?,
        "Hello, World!"
    );
    assert_eq!(
        fs::read_to_string(extract_dir.join("source/subdir/nested.txt"))?,
        "Nested file content"
    );

    Ok(())
}

#[test]
fn test_list_archive_contents() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_dir = temp_dir.path().join("source");
    let archive_path = temp_dir.path().join("list_test.zst");

    // Create test structure
    fs::create_dir(&source_dir)?;
    fs::write(source_dir.join("file1.txt"), "content1")?;
    fs::write(source_dir.join("file2.txt"), "content2")?;
    let subdir = source_dir.join("subdir");
    fs::create_dir(&subdir)?;
    fs::write(subdir.join("nested.txt"), "nested")?;

    // Compress
    let options = CompressionOptions::default();
    let filter = FileFilter::new(true, &[])?;
    ZstdFormat::compress(&source_dir, &archive_path, &options, &filter, None)?;

    // List contents
    let entries = ZstdFormat::list(&archive_path)?;

    // Verify entries
    assert!(!entries.is_empty());

    let paths: Vec<_> = entries.iter().map(|e| &e.path).collect();
    assert!(paths.iter().any(|p| p.contains("file1.txt")));
    assert!(paths.iter().any(|p| p.contains("file2.txt")));
    assert!(paths.iter().any(|p| p.contains("nested.txt")));

    // Check file sizes are recorded
    let file_entries: Vec<_> = entries.iter().filter(|e| e.is_file).collect();
    assert!(!file_entries.is_empty());
    assert!(file_entries.iter().all(|e| e.size > 0));

    Ok(())
}

#[test]
fn test_custom_compression_levels() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("test.txt");

    // Create a larger file for better compression testing
    let content = "This is test content that should compress well. ".repeat(100);
    fs::write(&source_file, &content)?;

    let filter = FileFilter::new(true, &[])?;

    // Test different compression levels
    for level in [1, 3, 19, 22] {
        let archive_path = temp_dir.path().join(format!("test_level_{level}.zst"));
        let options = CompressionOptions {
            level,
            ..Default::default()
        };

        let stats = ZstdFormat::compress(&source_file, &archive_path, &options, &filter, None)?;

        assert!(archive_path.exists());
        assert!(stats.input_size > 0);
        assert!(stats.output_size > 0);
        assert!(stats.compression_ratio > 0.0 && stats.compression_ratio < 1.0);
    }

    Ok(())
}

#[test]
fn test_custom_exclude_patterns() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_dir = temp_dir.path().join("source");
    let archive_path = temp_dir.path().join("filtered.zst");
    let extract_dir = temp_dir.path().join("extracted");

    // Create test files
    fs::create_dir(&source_dir)?;
    fs::write(source_dir.join("important.txt"), "keep this")?;
    fs::write(source_dir.join("secret.log"), "exclude this")?;
    fs::write(source_dir.join("test_file.txt"), "exclude this too")?;
    fs::write(source_dir.join("production.txt"), "keep this")?;

    // Compress with custom excludes
    let options = CompressionOptions::default();
    let custom_patterns = vec!["*.log".to_string(), "test_*".to_string()];
    let filter = FileFilter::new(true, &custom_patterns)?;
    ZstdFormat::compress(&source_dir, &archive_path, &options, &filter, None)?;

    // Extract and verify filtering
    fs::create_dir(&extract_dir)?;
    let extract_options = ExtractionOptions::default();
    ZstdFormat::extract(&archive_path, &extract_dir, &extract_options, None)?;

    // Should include
    assert!(extract_dir.join("source/important.txt").exists());
    assert!(extract_dir.join("source/production.txt").exists());

    // Should exclude
    assert!(!extract_dir.join("source/secret.log").exists());
    assert!(!extract_dir.join("source/test_file.txt").exists());

    Ok(())
}

#[test]
fn test_no_default_excludes() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_dir = temp_dir.path().join("source");
    let archive_path = temp_dir.path().join("no_filter.zst");
    let extract_dir = temp_dir.path().join("extracted");

    // Create test structure with garbage files
    fs::create_dir(&source_dir)?;
    fs::write(source_dir.join("normal.txt"), "normal file")?;
    fs::write(source_dir.join(".DS_Store"), "macos junk")?;
    fs::write(source_dir.join("thumbs.db"), "windows junk")?;

    // Compress with no default excludes
    let options = CompressionOptions::default();
    let filter = FileFilter::new(false, &[])?; // Disable default excludes
    ZstdFormat::compress(&source_dir, &archive_path, &options, &filter, None)?;

    // Extract and verify no filtering occurred
    fs::create_dir(&extract_dir)?;
    let extract_options = ExtractionOptions::default();
    ZstdFormat::extract(&archive_path, &extract_dir, &extract_options, None)?;

    // All files should be present
    assert!(extract_dir.join("source/normal.txt").exists());
    assert!(extract_dir.join("source/.DS_Store").exists());
    assert!(extract_dir.join("source/thumbs.db").exists());

    Ok(())
}

#[test]
fn test_overwrite_protection() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("test.txt");
    let archive_path = temp_dir.path().join("test.zst");
    let extract_dir = temp_dir.path().join("extracted");
    let target_file = extract_dir.join("test.txt");

    // Create source and archive
    fs::write(&source_file, "original content")?;
    let options = CompressionOptions::default();
    let filter = FileFilter::new(true, &[])?;
    ZstdFormat::compress(&source_file, &archive_path, &options, &filter, None)?;

    // Extract once
    fs::create_dir(&extract_dir)?;
    let extract_options = ExtractionOptions::default();
    ZstdFormat::extract(&archive_path, &extract_dir, &extract_options, None)?;

    // Modify extracted file
    fs::write(&target_file, "modified content")?;

    // Try to extract again without overwrite - should fail
    let result = ZstdFormat::extract(&archive_path, &extract_dir, &extract_options, None);
    assert!(result.is_err());

    // File should still have modified content
    assert_eq!(fs::read_to_string(&target_file)?, "modified content");

    // Extract with overwrite - should succeed
    let overwrite_options = ExtractionOptions {
        overwrite: true,
        ..Default::default()
    };
    ZstdFormat::extract(&archive_path, &extract_dir, &overwrite_options, None)?;

    // File should be restored to original content
    assert_eq!(fs::read_to_string(&target_file)?, "original content");

    Ok(())
}
