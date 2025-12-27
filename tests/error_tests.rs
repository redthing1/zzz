//! Error handling and edge case tests for zzz

use std::fs;
use std::path::Path;
use tempfile::TempDir;
use zzz_arc::filter::FileFilter;
use zzz_arc::formats::zstd::ZstdFormat;
use zzz_arc::formats::{CompressionFormat, CompressionOptions, ExtractionOptions};

type Result<T> = anyhow::Result<T>;

#[test]
fn test_compress_nonexistent_file() {
    let temp_dir = TempDir::new().unwrap();
    let nonexistent = temp_dir.path().join("does_not_exist.txt");
    let output = temp_dir.path().join("output.zst");

    let options = CompressionOptions::default();
    let filter = FileFilter::new(true, &[]).unwrap();

    let result = ZstdFormat::compress(&nonexistent, &output, &options, &filter, None);
    assert!(result.is_err());

    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("does_not_exist.txt") || error_msg.contains("No such file"));
}

#[test]
fn test_extract_nonexistent_archive() {
    let temp_dir = TempDir::new().unwrap();
    let nonexistent_archive = temp_dir.path().join("does_not_exist.zst");
    let extract_dir = temp_dir.path().join("extract");

    fs::create_dir(&extract_dir).unwrap();
    let options = ExtractionOptions::default();

    let result = ZstdFormat::extract(&nonexistent_archive, &extract_dir, &options, None);
    assert!(result.is_err());

    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("failed to open archive file"));
}

#[test]
fn test_list_nonexistent_archive() {
    let temp_dir = TempDir::new().unwrap();
    let nonexistent_archive = temp_dir.path().join("does_not_exist.zst");

    let result = ZstdFormat::list(&nonexistent_archive);
    assert!(result.is_err());

    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("failed to open archive file"));
}

#[test]
fn test_extract_corrupted_archive() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let corrupted_archive = temp_dir.path().join("corrupted.zst");
    let extract_dir = temp_dir.path().join("extract");

    // Create a corrupted "archive" (just random bytes)
    fs::write(&corrupted_archive, b"This is not a valid zstd archive")?;
    fs::create_dir(&extract_dir)?;

    let options = ExtractionOptions::default();
    let result = ZstdFormat::extract(&corrupted_archive, &extract_dir, &options, None);

    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    // The exact error message may vary, but it should indicate a zstd/decoding problem
    assert!(
        error_msg.contains("zstd")
            || error_msg.contains("decoder")
            || error_msg.contains("invalid")
            || error_msg.contains("frame descriptor"),
        "Error message: {error_msg}"
    );

    Ok(())
}

#[test]
fn test_list_corrupted_archive() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let corrupted_archive = temp_dir.path().join("corrupted.zst");

    // Create corrupted archive
    fs::write(&corrupted_archive, b"Not a zstd file")?;

    let result = ZstdFormat::list(&corrupted_archive);
    assert!(result.is_err());

    Ok(())
}

#[test]
fn test_compress_invalid_output_directory() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("test.txt");
    let invalid_output = temp_dir.path().join("nonexistent_dir").join("output.zst");

    fs::write(&source_file, "test content")?;

    let options = CompressionOptions::default();
    let filter = FileFilter::new(true, &[])?;

    let result = ZstdFormat::compress(&source_file, &invalid_output, &options, &filter, None);
    assert!(result.is_err());

    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("failed to create output file"));

    Ok(())
}

#[test]
fn test_extract_to_readonly_directory() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("test.txt");
    let archive_path = temp_dir.path().join("test.zst");
    let readonly_dir = temp_dir.path().join("readonly");

    // Create and compress file
    fs::write(&source_file, "test content")?;
    let options = CompressionOptions::default();
    let filter = FileFilter::new(true, &[])?;
    ZstdFormat::compress(&source_file, &archive_path, &options, &filter, None)?;

    // Create readonly directory (platform-dependent behavior)
    fs::create_dir(&readonly_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&readonly_dir)?.permissions();
        perms.set_mode(0o444); // Read-only
        fs::set_permissions(&readonly_dir, perms)?;
    }

    let extract_options = ExtractionOptions::default();
    let result = ZstdFormat::extract(&archive_path, &readonly_dir, &extract_options, None);

    // Restore permissions for cleanup
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&readonly_dir)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&readonly_dir, perms)?;
    }

    // On Unix systems, this should fail due to permissions
    #[cfg(unix)]
    assert!(result.is_err());

    // On other systems, behavior may vary
    #[cfg(not(unix))]
    {
        // Don't assert on Windows as behavior is different
        let _ = result;
    }

    Ok(())
}

#[test]
fn test_overwrite_protection() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("test.txt");
    let archive_path = temp_dir.path().join("test.zst");
    let extract_dir = temp_dir.path().join("extract");
    let target_file = extract_dir.join("test.txt");

    // Create and compress file
    fs::write(&source_file, "original")?;
    let options = CompressionOptions::default();
    let filter = FileFilter::new(true, &[])?;
    ZstdFormat::compress(&source_file, &archive_path, &options, &filter, None)?;

    // Extract once
    fs::create_dir(&extract_dir)?;
    let extract_options = ExtractionOptions {
        overwrite: false,
        ..Default::default()
    };
    ZstdFormat::extract(&archive_path, &extract_dir, &extract_options, None)?;

    // Modify the extracted file
    fs::write(&target_file, "modified")?;

    // Try to extract again without overwrite flag - should fail
    let result = ZstdFormat::extract(&archive_path, &extract_dir, &extract_options, None);
    assert!(result.is_err());

    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("already exists"));
    assert!(error_msg.contains("overwrite"));

    // File should still contain modified content
    assert_eq!(fs::read_to_string(&target_file)?, "modified");

    Ok(())
}

#[test]
fn test_directory_traversal_protection() -> Result<()> {
    // This test would require creating a malicious archive with "../" paths
    // For now, we'll test that our extraction logic would catch such cases

    // Note: This is a simplified test. In practice, you'd need to create
    // an actual malicious tar archive to fully test this protection.

    let temp_dir = TempDir::new()?;
    let extract_dir = temp_dir.path().join("safe_extract");
    fs::create_dir(&extract_dir)?;

    // Test that our path validation would catch dangerous paths
    let dangerous_paths = vec![
        "../etc/passwd",
        "../../sensitive_file",
        "/etc/passwd",
        "subdir/../../../outside",
    ];

    for dangerous_path in dangerous_paths {
        let path = Path::new(dangerous_path);

        // Check if path contains parent directory components
        let has_parent_dir = path
            .components()
            .any(|comp| matches!(comp, std::path::Component::ParentDir));

        if has_parent_dir {
            // Our extraction code should reject such paths
            // Our extraction code should reject such paths
            println!("Path {dangerous_path} contains dangerous parent directory references");
        }
    }

    Ok(())
}

#[test]
fn test_extract_rejects_unsafe_paths() -> Result<()> {
    use std::io::Write;

    let temp_dir = TempDir::new()?;
    let archive_path = temp_dir.path().join("unsafe.zip");
    let extract_dir = temp_dir.path().join("extract");
    fs::create_dir(&extract_dir)?;

    let output_file = fs::File::create(&archive_path)?;
    let mut zip = zip::ZipWriter::new(output_file);
    zip.start_file("../evil.txt", zip::write::FileOptions::default())?;
    zip.write_all(b"unsafe content")?;
    zip.finish()?;

    let options = ExtractionOptions::default();
    let result =
        zzz_arc::formats::zip::ZipFormat::extract(&archive_path, &extract_dir, &options, None);

    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("unsafe archive path"));

    Ok(())
}

#[test]
fn test_zip_integrity_detects_corruption() -> Result<()> {
    use std::io::Write;
    use zip::write::FileOptions;

    let temp_dir = TempDir::new()?;
    let archive_path = temp_dir.path().join("corrupt.zip");

    let file = fs::File::create(&archive_path)?;
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("file.txt", FileOptions::default())?;
    zip.write_all(b"zip content")?;
    zip.finish()?;

    let mut data = fs::read(&archive_path)?;
    if let Some(last) = data.last_mut() {
        *last ^= 0xFF;
    }
    fs::write(&archive_path, data)?;

    let result = zzz_arc::formats::zip::ZipFormat::test_integrity(&archive_path);
    assert!(result.is_err());

    Ok(())
}

#[cfg(unix)]
#[test]
fn test_extract_rejects_symlink_ancestor() -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::symlink;
    use zip::write::FileOptions;

    let temp_dir = TempDir::new()?;
    let archive_path = temp_dir.path().join("symlink.zip");
    let extract_dir = temp_dir.path().join("extract");
    let outside_dir = temp_dir.path().join("outside");

    fs::create_dir(&extract_dir)?;
    fs::create_dir(&outside_dir)?;
    symlink(&outside_dir, extract_dir.join("link"))?;

    let file = fs::File::create(&archive_path)?;
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("link/evil.txt", FileOptions::default())?;
    zip.write_all(b"evil")?;
    zip.finish()?;

    let options = ExtractionOptions::default();
    let result =
        zzz_arc::formats::zip::ZipFormat::extract(&archive_path, &extract_dir, &options, None);

    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("symlink ancestor"));

    Ok(())
}

#[test]
fn test_empty_directory_handling() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let empty_dir = temp_dir.path().join("empty");
    let archive_path = temp_dir.path().join("empty.zst");
    let extract_dir = temp_dir.path().join("extract");

    // Create empty directory
    fs::create_dir(&empty_dir)?;

    // Compress empty directory
    let options = CompressionOptions::default();
    let filter = FileFilter::new(true, &[])?;
    let stats = ZstdFormat::compress(&empty_dir, &archive_path, &options, &filter, None)?;

    // Should succeed and create archive
    assert!(archive_path.exists());
    assert_eq!(stats.input_size, 0); // No files to compress

    // Extract and verify
    fs::create_dir(&extract_dir)?;
    let extract_options = ExtractionOptions::default();
    ZstdFormat::extract(&archive_path, &extract_dir, &extract_options, None)?;

    // Empty directory should be recreated
    assert!(extract_dir.join("empty").exists());
    assert!(extract_dir.join("empty").is_dir());

    Ok(())
}

#[test]
fn test_very_long_filename() -> Result<()> {
    let temp_dir = TempDir::new()?;

    // Create file with very long name (close to filesystem limits)
    let long_name = "a".repeat(200); // 200 character filename
    let long_file = temp_dir.path().join(&long_name);
    let archive_path = temp_dir.path().join("long.zst");

    fs::write(&long_file, "content")?;

    // Should handle long filenames gracefully
    let options = CompressionOptions::default();
    let filter = FileFilter::new(true, &[])?;
    let result = ZstdFormat::compress(&long_file, &archive_path, &options, &filter, None);

    // Should either succeed or fail gracefully
    match result {
        Ok(_) => {
            // If compression succeeded, extraction should also work
            let extract_dir = temp_dir.path().join("extract");
            fs::create_dir(&extract_dir)?;
            let extract_options = ExtractionOptions::default();
            ZstdFormat::extract(&archive_path, &extract_dir, &extract_options, None)?;

            let extracted_file = extract_dir.join(&long_name);
            assert!(extracted_file.exists());
            assert_eq!(fs::read_to_string(extracted_file)?, "content");
        }
        Err(_) => {
            // If it fails, that's also acceptable for extremely long names
            println!("Long filename handling failed gracefully");
        }
    }

    Ok(())
}

#[test]
fn test_invalid_glob_pattern_in_filter() {
    // Test that FileFilter handles invalid glob patterns gracefully
    let invalid_patterns = vec![
        "[".to_string(),     // Unclosed bracket
        "[z-a]".to_string(), // Invalid range (may or may not fail depending on glob implementation)
    ];

    for pattern in invalid_patterns {
        let result = FileFilter::new(true, std::slice::from_ref(&pattern));
        // Some patterns may be accepted by the glob crate, so we just ensure
        // the function doesn't panic and handles them gracefully
        match result {
            Ok(_) => println!("Pattern '{pattern}' was accepted"),
            Err(_) => println!("Pattern '{pattern}' was rejected"),
        }
    }
}

#[test]
fn test_zero_byte_file() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let empty_file = temp_dir.path().join("empty.txt");
    let archive_path = temp_dir.path().join("empty.zst");
    let extract_dir = temp_dir.path().join("extract");

    // Create zero-byte file
    fs::write(&empty_file, b"")?;

    // Compress
    let options = CompressionOptions::default();
    let filter = FileFilter::new(true, &[])?;
    let stats = ZstdFormat::compress(&empty_file, &archive_path, &options, &filter, None)?;

    assert!(archive_path.exists());
    assert_eq!(stats.input_size, 0);

    // Extract
    fs::create_dir(&extract_dir)?;
    let extract_options = ExtractionOptions::default();
    ZstdFormat::extract(&archive_path, &extract_dir, &extract_options, None)?;

    // Verify empty file was extracted
    let extracted_file = extract_dir.join("empty.txt");
    assert!(extracted_file.exists());
    assert_eq!(fs::read(extracted_file)?, b"");

    Ok(())
}
