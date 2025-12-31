//! Integration tests for multiple compression formats

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

/// Test helper to create test data
fn create_test_data(dir: &std::path::Path) -> std::io::Result<()> {
    fs::write(dir.join("file1.txt"), "Hello, World!")?;
    fs::write(dir.join("file2.txt"), "Another test file")?;

    let subdir = dir.join("subdir");
    fs::create_dir(&subdir)?;
    fs::write(subdir.join("nested.txt"), "Nested file content")?;

    Ok(())
}

#[test]
fn test_zstd_format() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_dir = temp_dir.path().join("test_data");
    fs::create_dir(&test_dir)?;
    create_test_data(&test_dir)?;

    let output_file = temp_dir.path().join("test.zst");

    // Test compression
    let mut cmd = cargo_bin_cmd!("zzz");
    cmd.arg("compress")
        .arg("-o")
        .arg(&output_file)
        .arg(&test_dir);
    cmd.assert().success();

    assert!(output_file.exists());

    // Test listing
    let mut cmd = cargo_bin_cmd!("zzz");
    cmd.arg("list").arg(&output_file);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("file1.txt"))
        .stdout(predicate::str::contains("file2.txt"))
        .stdout(predicate::str::contains("nested.txt"));

    // Test extraction
    let extract_dir = temp_dir.path().join("extract");
    let mut cmd = cargo_bin_cmd!("zzz");
    cmd.arg("extract")
        .arg(&output_file)
        .arg("-C")
        .arg(&extract_dir);
    cmd.assert().success();

    // Verify extracted files
    assert!(extract_dir.join("test_data/file1.txt").exists());
    assert!(extract_dir.join("test_data/file2.txt").exists());
    assert!(extract_dir.join("test_data/subdir/nested.txt").exists());

    let content = fs::read_to_string(extract_dir.join("test_data/file1.txt"))?;
    assert_eq!(content, "Hello, World!");

    Ok(())
}

#[test]
fn test_gzip_format() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_dir = temp_dir.path().join("test_data");
    fs::create_dir(&test_dir)?;
    create_test_data(&test_dir)?;

    let output_file = temp_dir.path().join("test.tgz");

    // Test compression
    let mut cmd = cargo_bin_cmd!("zzz");
    cmd.arg("compress")
        .arg("-o")
        .arg(&output_file)
        .arg(&test_dir);
    cmd.assert().success();

    assert!(output_file.exists());

    // Test listing
    let mut cmd = cargo_bin_cmd!("zzz");
    cmd.arg("list").arg(&output_file);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("file1.txt"))
        .stdout(predicate::str::contains("file2.txt"));

    // Test extraction
    let extract_dir = temp_dir.path().join("extract");
    let mut cmd = cargo_bin_cmd!("zzz");
    cmd.arg("extract")
        .arg(&output_file)
        .arg("-C")
        .arg(&extract_dir);
    cmd.assert().success();

    // Verify extracted files
    assert!(extract_dir.join("test_data/file1.txt").exists());
    let content = fs::read_to_string(extract_dir.join("test_data/file1.txt"))?;
    assert_eq!(content, "Hello, World!");

    Ok(())
}

#[test]
fn test_xz_format() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_dir = temp_dir.path().join("test_data");
    fs::create_dir(&test_dir)?;
    create_test_data(&test_dir)?;

    let output_file = temp_dir.path().join("test.txz");

    // Test compression
    let mut cmd = cargo_bin_cmd!("zzz");
    cmd.arg("compress")
        .arg("-o")
        .arg(&output_file)
        .arg(&test_dir);
    cmd.assert().success();

    assert!(output_file.exists());

    // Test listing and extraction
    let mut cmd = cargo_bin_cmd!("zzz");
    cmd.arg("list").arg(&output_file);
    cmd.assert().success();

    let extract_dir = temp_dir.path().join("extract");
    let mut cmd = cargo_bin_cmd!("zzz");
    cmd.arg("extract")
        .arg(&output_file)
        .arg("-C")
        .arg(&extract_dir);
    cmd.assert().success();

    assert!(extract_dir.join("test_data/file1.txt").exists());

    Ok(())
}

#[test]
fn test_zip_format() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_dir = temp_dir.path().join("test_data");
    fs::create_dir(&test_dir)?;
    create_test_data(&test_dir)?;

    let output_file = temp_dir.path().join("test.zip");

    // Test compression
    let mut cmd = cargo_bin_cmd!("zzz");
    cmd.arg("compress")
        .arg("-o")
        .arg(&output_file)
        .arg(&test_dir);
    cmd.assert().success();

    assert!(output_file.exists());

    // Test listing and extraction
    let mut cmd = cargo_bin_cmd!("zzz");
    cmd.arg("list").arg(&output_file);
    cmd.assert().success();

    let extract_dir = temp_dir.path().join("extract");
    let mut cmd = cargo_bin_cmd!("zzz");
    cmd.arg("extract")
        .arg(&output_file)
        .arg("-C")
        .arg(&extract_dir);
    cmd.assert().success();

    assert!(extract_dir.join("test_data/file1.txt").exists());

    Ok(())
}

#[test]
fn test_7z_format() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_dir = temp_dir.path().join("test_data");
    fs::create_dir(&test_dir)?;
    create_test_data(&test_dir)?;

    let output_file = temp_dir.path().join("test.7z");

    // Test compression
    let mut cmd = cargo_bin_cmd!("zzz");
    cmd.arg("compress")
        .arg("-o")
        .arg(&output_file)
        .arg(&test_dir);
    cmd.assert().success();

    assert!(output_file.exists());

    // Test listing and extraction
    let mut cmd = cargo_bin_cmd!("zzz");
    cmd.arg("list").arg(&output_file);
    cmd.assert().success();

    let extract_dir = temp_dir.path().join("extract");
    let mut cmd = cargo_bin_cmd!("zzz");
    cmd.arg("extract")
        .arg(&output_file)
        .arg("-C")
        .arg(&extract_dir);
    cmd.assert().success();

    assert!(extract_dir.join("test_data/file1.txt").exists());

    Ok(())
}

#[test]
fn test_format_detection_by_magic() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_file = temp_dir.path().join("test.txt");
    fs::write(&test_file, "test content")?;

    // Create a zst file without extension
    let zst_file = temp_dir.path().join("test.zst");
    let mut cmd = cargo_bin_cmd!("zzz");
    cmd.arg("compress").arg("-o").arg(&zst_file).arg(&test_file);
    cmd.assert().success();

    // Copy to file without extension
    let unknown_file = temp_dir.path().join("test_unknown");
    fs::copy(&zst_file, &unknown_file)?;

    // Test that we can still list it (magic number detection)
    let mut cmd = cargo_bin_cmd!("zzz");
    cmd.arg("list").arg(&unknown_file);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("test.txt"));

    Ok(())
}

#[test]
fn test_compression_levels() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_file = temp_dir.path().join("test.txt");
    fs::write(&test_file, "test content for compression level testing")?;

    // Test different compression levels
    for level in [1, 5, 10, 15, 20, 22] {
        let output_file = temp_dir.path().join(format!("test_level_{level}.zst"));

        let mut cmd = cargo_bin_cmd!("zzz");
        cmd.arg("compress")
            .arg("-l")
            .arg(level.to_string())
            .arg("-o")
            .arg(&output_file)
            .arg(&test_file);
        cmd.assert().success();

        assert!(output_file.exists());
    }

    Ok(())
}

#[test]
fn test_verbose_output() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_file = temp_dir.path().join("test.txt");
    fs::write(&test_file, "test content")?;

    // Test case insensitive format parsing
    let case_variations = ["ZIP", "Zip", "zIP", "ZsT", "TGZ", "7Z"];

    for format in case_variations {
        let output_file = temp_dir.path().join(format!("test_{format}.archive"));

        let mut cmd = cargo_bin_cmd!("zzz");
        cmd.arg("compress")
            .arg("-f")
            .arg(format)
            .arg("-o")
            .arg(&output_file)
            .arg(&test_file);
        cmd.assert().success();

        assert!(output_file.exists());
    }

    Ok(())
}

#[test]
fn test_unsupported_format() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_file = temp_dir.path().join("test.txt");
    fs::write(&test_file, "test content")?;

    let output_file = temp_dir.path().join("test.unknown");

    let mut cmd = cargo_bin_cmd!("zzz");
    cmd.arg("compress")
        .arg("-o")
        .arg(&output_file)
        .arg(&test_file);
    cmd.assert().failure();

    Ok(())
}

#[test]
fn test_format_flag_override() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_file = temp_dir.path().join("test.txt");
    fs::write(&test_file, "test content for format override")?;

    // Test format override with non-matching extension
    let output_file = temp_dir.path().join("test.archive");

    let mut cmd = cargo_bin_cmd!("zzz");
    cmd.arg("compress")
        .arg("-f")
        .arg("zip")
        .arg("-o")
        .arg(&output_file)
        .arg(&test_file);
    cmd.assert().success();

    assert!(output_file.exists());

    // Verify it was created as ZIP by listing it
    let mut cmd = cargo_bin_cmd!("zzz");
    cmd.arg("list").arg(&output_file);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("test.txt"));

    Ok(())
}

#[test]
fn test_format_flag_auto_extension() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_file = temp_dir.path().join("test.txt");
    fs::write(&test_file, "test content")?;

    // Test format flag with auto-generated extension
    let mut cmd = cargo_bin_cmd!("zzz");
    cmd.arg("compress").arg("-f").arg("7z").arg(&test_file);
    cmd.assert().success();

    // Should create test.txt.7z
    let expected_output = test_file.with_extension("txt.7z");
    assert!(expected_output.exists());

    Ok(())
}

#[test]
fn test_format_flag_all_formats() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_file = temp_dir.path().join("test.txt");
    fs::write(&test_file, "test content for all formats")?;

    let formats = [
        ("zst", "test1.archive"),
        ("tgz", "test2.archive"),
        ("txz", "test3.archive"),
        ("zip", "test4.archive"),
        ("7z", "test5.archive"),
    ];

    for (format, output_name) in formats {
        let output_file = temp_dir.path().join(output_name);

        let mut cmd = cargo_bin_cmd!("zzz");
        cmd.arg("compress")
            .arg("-f")
            .arg(format)
            .arg("-o")
            .arg(&output_file)
            .arg(&test_file);
        cmd.assert().success();

        assert!(output_file.exists());

        // Verify we can list the archive
        let mut cmd = cargo_bin_cmd!("zzz");
        cmd.arg("list").arg(&output_file);
        cmd.assert().success();
    }

    Ok(())
}

#[test]
fn test_format_flag_invalid() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_file = temp_dir.path().join("test.txt");
    fs::write(&test_file, "test content")?;

    let mut cmd = cargo_bin_cmd!("zzz");
    cmd.arg("compress")
        .arg("-f")
        .arg("invalid_format")
        .arg(&test_file);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("unsupported format"));

    Ok(())
}

#[test]
fn test_format_flag_aliases() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_file = temp_dir.path().join("test.txt");
    fs::write(&test_file, "test content")?;

    // Test format aliases
    let aliases = [
        ("gz", "test_gz.archive"),
        ("gzip", "test_gzip.archive"),
        ("xz", "test_xz.archive"),
        ("zstd", "test_zstd.archive"),
        ("sevenz", "test_sevenz.archive"),
    ];

    for (alias, output_name) in aliases {
        let output_file = temp_dir.path().join(output_name);

        let mut cmd = cargo_bin_cmd!("zzz");
        cmd.arg("compress")
            .arg("-f")
            .arg(alias)
            .arg("-o")
            .arg(&output_file)
            .arg(&test_file);
        cmd.assert().success();

        assert!(output_file.exists());
    }

    Ok(())
}

#[test]
fn test_format_flag_with_compression_levels() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_file = temp_dir.path().join("test.txt");
    fs::write(&test_file, "test content for compression levels")?;

    // Test format flag with different compression levels
    let test_cases = [("zst", 1), ("zip", 5), ("7z", 9), ("tgz", 3), ("txz", 6)];

    for (format, level) in test_cases {
        let output_file = temp_dir
            .path()
            .join(format!("test_{format}_level_{level}.archive"));

        let mut cmd = cargo_bin_cmd!("zzz");
        cmd.arg("compress")
            .arg("-f")
            .arg(format)
            .arg("-l")
            .arg(level.to_string())
            .arg("-o")
            .arg(&output_file)
            .arg(&test_file);
        cmd.assert().success();

        assert!(output_file.exists());
    }

    Ok(())
}

#[test]
fn test_format_flag_case_insensitive() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_file = temp_dir.path().join("test.txt");
    fs::write(&test_file, "test content")?;

    // Test case insensitive format parsing
    let case_variations = ["ZIP", "Zip", "zIP", "ZsT", "TGZ", "7Z"];

    for format in case_variations {
        let output_file = temp_dir.path().join(format!("test_{format}.archive"));

        let mut cmd = cargo_bin_cmd!("zzz");
        cmd.arg("compress")
            .arg("-f")
            .arg(format)
            .arg("-o")
            .arg(&output_file)
            .arg(&test_file);
        cmd.assert().success();

        assert!(output_file.exists());
    }

    Ok(())
}
