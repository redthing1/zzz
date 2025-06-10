//! Tests for explicit format flag functionality

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_format_flag_with_compression_levels() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_file = temp_dir.path().join("test.txt");
    fs::write(&test_file, "test content for compression levels")?;

    // Test format flag with different compression levels
    let test_cases = [
        ("zst", 1),
        ("zip", 5),
        ("7z", 9),
        ("tgz", 3),
        ("txz", 6),
    ];

    for (format, level) in test_cases {
        let output_file = temp_dir.path().join(format!("test_{}_level_{}.archive", format, level));

        let mut cmd = Command::cargo_bin("zzz")?;
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
        let output_file = temp_dir.path().join(format!("test_{}.archive", format));

        let mut cmd = Command::cargo_bin("zzz")?;
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
fn test_format_flag_short_option() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_file = temp_dir.path().join("test.txt");
    fs::write(&test_file, "test content")?;

    let output_file = temp_dir.path().join("test_short.archive");

    // Test short option -f
    let mut cmd = Command::cargo_bin("zzz")?;
    cmd.arg("compress")
        .arg("-f")
        .arg("zip")
        .arg("-o")
        .arg(&output_file)
        .arg(&test_file);
    cmd.assert().success();

    assert!(output_file.exists());

    Ok(())
}

#[test]
fn test_format_flag_overrides_extension() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_file = temp_dir.path().join("test.txt");
    fs::write(&test_file, "test content")?;

    // Create output with .zip extension but force 7z format
    let output_file = temp_dir.path().join("test.zip");

    let mut cmd = Command::cargo_bin("zzz")?;
    cmd.arg("compress")
        .arg("-f")
        .arg("7z")
        .arg("-o")
        .arg(&output_file)
        .arg(&test_file);
    cmd.assert().success();

    // Should be created as 7z format despite .zip extension
    assert!(output_file.exists());

    // Verify it was created as 7z format by checking we can list it
    // (magic number detection should work regardless of extension)
    let mut cmd = Command::cargo_bin("zzz")?;
    cmd.arg("list")
        .arg(&output_file);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("test.txt"));

    Ok(())
}

#[test]
fn test_format_flag_with_excludes() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_dir = temp_dir.path().join("test_data");
    fs::create_dir(&test_dir)?;
    
    fs::write(test_dir.join("keep.txt"), "keep this")?;
    fs::write(test_dir.join("exclude.log"), "exclude this")?;
    fs::write(test_dir.join("also_keep.txt"), "also keep")?;

    let output_file = temp_dir.path().join("test_excludes.archive");

    let mut cmd = Command::cargo_bin("zzz")?;
    cmd.arg("compress")
        .arg("-f")
        .arg("tgz")
        .arg("--exclude")
        .arg("*.log")
        .arg("-o")
        .arg(&output_file)
        .arg(&test_dir);
    cmd.assert().success();

    assert!(output_file.exists());

    // Verify excluded file is not in archive
    let mut cmd = Command::cargo_bin("zzz")?;
    cmd.arg("list").arg(&output_file);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("keep.txt"))
        .stdout(predicate::str::contains("also_keep.txt"))
        .stdout(predicate::str::contains("exclude.log").not());

    Ok(())
}