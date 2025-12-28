//! CLI integration tests for zzz

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

type Result<T> = anyhow::Result<T>;

fn zzz_cmd() -> Command {
    Command::cargo_bin("zzz").expect("failed to find zzz binary")
}

#[test]
fn test_cli_help() {
    zzz_cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Create and extract archives in multiple formats",
        ))
        .stdout(predicate::str::contains("Commands:"))
        .stdout(predicate::str::contains("compress"))
        .stdout(predicate::str::contains("extract"))
        .stdout(predicate::str::contains("list"));
}

#[test]
fn test_cli_version() {
    zzz_cmd()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("zzz"));
}

#[test]
fn test_compress_help() {
    zzz_cmd()
        .args(["compress", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "compress files/directories (supports",
        ))
        .stdout(predicate::str::contains("--level"))
        .stdout(predicate::str::contains("--output"))
        .stdout(predicate::str::contains("--progress"))
        .stdout(predicate::str::contains("--exclude"))
        .stdout(predicate::str::contains("--keep-xattrs"))
        .stdout(predicate::str::contains("--keep-permissions"))
        .stdout(predicate::str::contains("--keep-ownership"))
        .stdout(predicate::str::contains("--follow-symlinks"))
        .stdout(predicate::str::contains("--allow-symlink-escape"))
        .stdout(predicate::str::contains("--redact"))
        .stdout(predicate::str::contains("--strip-timestamps"));
}

#[test]
fn test_extract_help() {
    zzz_cmd()
        .args(["extract", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("extract archives"))
        .stdout(predicate::str::contains("--strip-components"))
        .stdout(predicate::str::contains("--keep-xattrs"))
        .stdout(predicate::str::contains("--keep-permissions"))
        .stdout(predicate::str::contains("--keep-ownership"))
        .stdout(predicate::str::contains("--strip-timestamps"))
        .stdout(predicate::str::contains("--overwrite"));
}

#[test]
fn test_compress_single_file() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("test.txt");
    let output_file = temp_dir.path().join("test.zst");

    // Create source file
    fs::write(&source_file, "Hello, CLI test!")?;

    // Compress via CLI
    zzz_cmd()
        .args(["compress", "--output"])
        .arg(&output_file)
        .arg(&source_file)
        .assert()
        .success()
        .stdout(predicate::str::contains("compressed"))
        .stdout(predicate::str::contains("test.txt"))
        .stdout(predicate::str::contains("test.zst"));

    // Verify output file exists
    assert!(output_file.exists());

    Ok(())
}

#[test]
fn test_compress_single_file_excluded() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("skip.tmp");
    fs::write(&source_file, "excluded content")?;

    let cases = [
        ("zst", "skip.zst"),
        ("zip", "skip.zip"),
        ("7z", "skip.7z"),
        ("gz", "skip.tgz"),
        ("xz", "skip.txz"),
    ];

    for (format, filename) in cases {
        let output_file = temp_dir.path().join(filename);

        zzz_cmd()
            .args(["compress", "--exclude", "*.tmp", "-f", format, "-o"])
            .arg(&output_file)
            .arg(&source_file)
            .assert()
            .success();

        zzz_cmd()
            .args(["list"])
            .arg(&output_file)
            .assert()
            .success()
            .stdout(predicate::str::is_empty());
    }

    Ok(())
}

#[test]
fn test_compress_with_short_alias() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("test.txt");
    let output_file = temp_dir.path().join("test.zst");

    fs::write(&source_file, "Test content")?;

    // Use 'c' alias for compress
    zzz_cmd()
        .args(["c", "-o"])
        .arg(&output_file)
        .arg(&source_file)
        .assert()
        .success();

    assert!(output_file.exists());

    Ok(())
}

#[test]
fn test_compress_directory() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_dir = temp_dir.path().join("testdir");
    let output_file = temp_dir.path().join("testdir.zst");

    // Create test directory structure
    fs::create_dir(&source_dir)?;
    fs::write(source_dir.join("file1.txt"), "content1")?;
    fs::write(source_dir.join("file2.txt"), "content2")?;

    let subdir = source_dir.join("subdir");
    fs::create_dir(&subdir)?;
    fs::write(subdir.join("nested.txt"), "nested")?;

    // Compress directory
    zzz_cmd()
        .args(["compress", "-o"])
        .arg(&output_file)
        .arg(&source_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("compressed"));

    assert!(output_file.exists());

    Ok(())
}

#[cfg(unix)]
#[test]
fn test_compress_follow_symlinks_cli() -> Result<()> {
    use std::os::unix::fs::symlink;

    let temp_dir = TempDir::new()?;
    let source_dir = temp_dir.path().join("source");
    let output_file = temp_dir.path().join("archive.zst");

    fs::create_dir(&source_dir)?;
    let target = source_dir.join("target.txt");
    fs::write(&target, "symlink content")?;
    symlink(&target, source_dir.join("link.txt"))?;

    zzz_cmd()
        .args(["compress", "--follow-symlinks", "-o"])
        .arg(&output_file)
        .arg(&source_dir)
        .assert()
        .success();

    zzz_cmd()
        .args(["list"])
        .arg(&output_file)
        .assert()
        .success()
        .stdout(predicate::str::contains("link.txt"));

    Ok(())
}

#[test]
fn test_compress_allow_symlink_escape_requires_follow() {
    zzz_cmd()
        .args(["compress", "--allow-symlink-escape", "dummy.txt"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("requires --follow-symlinks"));
}

#[test]
fn test_compress_with_custom_level() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("test.txt");
    let output_file = temp_dir.path().join("test.zst");

    fs::write(&source_file, "Test compression level")?;

    // Compress with level 1 (fast)
    zzz_cmd()
        .args(["compress", "--level", "1", "-o"])
        .arg(&output_file)
        .arg(&source_file)
        .assert()
        .success();

    assert!(output_file.exists());

    Ok(())
}

#[test]
fn test_compress_with_custom_excludes() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_dir = temp_dir.path().join("source");
    let output_file = temp_dir.path().join("filtered.zst");

    // Create files to test filtering
    fs::create_dir(&source_dir)?;
    fs::write(source_dir.join("keep.txt"), "keep this")?;
    fs::write(source_dir.join("exclude.log"), "exclude this")?;
    fs::write(source_dir.join("test_file.txt"), "exclude this too")?;

    // Compress with custom excludes
    zzz_cmd()
        .args([
            "compress",
            "--exclude",
            "*.log",
            "--exclude",
            "test_*",
            "-o",
        ])
        .arg(&output_file)
        .arg(&source_dir)
        .assert()
        .success();

    assert!(output_file.exists());

    Ok(())
}

#[test]
fn test_redact_excludes_sensitive_files() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_dir = temp_dir.path().join("source");
    let output_file = temp_dir.path().join("redact.zst");

    fs::create_dir(&source_dir)?;
    fs::write(source_dir.join(".env"), "SECRET=1")?;
    fs::write(source_dir.join("keep.txt"), "safe")?;

    zzz_cmd()
        .args(["compress", "--redact", "-f", "zst", "-o"])
        .arg(&output_file)
        .arg(&source_dir)
        .assert()
        .success();

    zzz_cmd()
        .args(["list"])
        .arg(&output_file)
        .assert()
        .success()
        .stdout(predicate::str::contains("keep.txt"))
        .stdout(predicate::str::contains(".env").not());

    Ok(())
}

#[test]
fn test_redact_excludes_sensitive_files_without_defaults() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_dir = temp_dir.path().join("source");
    let output_file = temp_dir.path().join("redact_no_defaults.zst");

    fs::create_dir(&source_dir)?;
    fs::write(source_dir.join(".env"), "SECRET=1")?;
    fs::write(source_dir.join("keep.txt"), "safe")?;

    zzz_cmd()
        .args(["compress", "--redact", "-E", "-f", "zst", "-o"])
        .arg(&output_file)
        .arg(&source_dir)
        .assert()
        .success();

    zzz_cmd()
        .args(["list"])
        .arg(&output_file)
        .assert()
        .success()
        .stdout(predicate::str::contains("keep.txt"))
        .stdout(predicate::str::contains(".env").not());

    Ok(())
}

#[test]
fn test_defaults_do_not_exclude_sensitive_files() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_dir = temp_dir.path().join("source");
    let output_file = temp_dir.path().join("defaults_include.zst");

    fs::create_dir(&source_dir)?;
    fs::write(source_dir.join(".env"), "SECRET=1")?;
    fs::write(source_dir.join("keep.txt"), "safe")?;

    zzz_cmd()
        .args(["compress", "-f", "zst", "-o"])
        .arg(&output_file)
        .arg(&source_dir)
        .assert()
        .success();

    zzz_cmd()
        .args(["list"])
        .arg(&output_file)
        .assert()
        .success()
        .stdout(predicate::str::contains("keep.txt"))
        .stdout(predicate::str::contains(".env"));

    Ok(())
}

#[test]
fn test_extract_archive() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("test.txt");
    let archive_file = temp_dir.path().join("test.zst");
    let extract_dir = temp_dir.path().join("extracted");

    // Create and compress file
    fs::write(&source_file, "Extract test content")?;
    zzz_cmd()
        .args(["compress", "-o"])
        .arg(&archive_file)
        .arg(&source_file)
        .assert()
        .success();

    // Create extraction directory
    fs::create_dir(&extract_dir)?;

    // Extract via CLI
    zzz_cmd()
        .args(["extract", "-C"])
        .arg(&extract_dir)
        .arg(&archive_file)
        .assert()
        .success()
        .stdout(predicate::str::contains("extracting").or(predicate::str::is_empty()));

    // Verify extracted file
    let extracted_file = extract_dir.join("test.txt");
    assert!(extracted_file.exists());
    assert_eq!(fs::read_to_string(extracted_file)?, "Extract test content");

    Ok(())
}

#[test]
fn test_extract_with_alias() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("test.txt");
    let archive_file = temp_dir.path().join("test.zst");
    let extract_dir = temp_dir.path().join("extracted");

    fs::write(&source_file, "Test content")?;
    zzz_cmd()
        .args(["c", "-o"])
        .arg(&archive_file)
        .arg(&source_file)
        .assert()
        .success();

    fs::create_dir(&extract_dir)?;

    // Use 'x' alias for extract
    zzz_cmd()
        .args(["x", "-C"])
        .arg(&extract_dir)
        .arg(&archive_file)
        .assert()
        .success();

    assert!(extract_dir.join("test.txt").exists());

    Ok(())
}

#[test]
fn test_list_archive() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_dir = temp_dir.path().join("source");
    let archive_file = temp_dir.path().join("list_test.zst");

    // Create test structure
    fs::create_dir(&source_dir)?;
    fs::write(source_dir.join("file1.txt"), "content1")?;
    fs::write(source_dir.join("file2.txt"), "content2")?;

    let subdir = source_dir.join("subdir");
    fs::create_dir(&subdir)?;
    fs::write(subdir.join("nested.txt"), "nested")?;

    // Compress
    zzz_cmd()
        .args(["compress", "-o"])
        .arg(&archive_file)
        .arg(&source_dir)
        .assert()
        .success();

    // List contents
    zzz_cmd()
        .args(["list"])
        .arg(&archive_file)
        .assert()
        .success()
        .stdout(predicate::str::contains("file1.txt"))
        .stdout(predicate::str::contains("file2.txt"))
        .stdout(predicate::str::contains("nested.txt"));

    Ok(())
}

#[test]
fn test_list_with_alias_and_verbose() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("test.txt");
    let archive_file = temp_dir.path().join("test.zst");

    fs::write(&source_file, "Test content for verbose listing")?;

    zzz_cmd()
        .args(["c", "-o"])
        .arg(&archive_file)
        .arg(&source_file)
        .assert()
        .success();

    // Use 'l' alias with verbose flag
    zzz_cmd()
        .args(["l", "--verbose"])
        .arg(&archive_file)
        .assert()
        .success()
        .stdout(predicate::str::contains("test.txt"))
        .stdout(predicate::str::contains("B")); // Should show file size

    Ok(())
}

#[test]
fn test_compress_auto_output_name() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("auto_name.txt");
    let expected_output = temp_dir.path().join("auto_name.txt.zst");

    fs::write(&source_file, "Auto naming test")?;

    // Compress without specifying output (should auto-generate name)
    zzz_cmd()
        .args(["compress"])
        .arg(&source_file)
        .current_dir(&temp_dir)
        .assert()
        .success();

    assert!(expected_output.exists());

    Ok(())
}

#[test]
fn test_error_missing_input_file() {
    zzz_cmd()
        .args(["compress", "/nonexistent/file.txt"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}

#[test]
fn test_error_invalid_compression_level() {
    let temp_dir = TempDir::new().unwrap();
    let source_file = temp_dir.path().join("test.txt");
    fs::write(&source_file, "test").unwrap();

    zzz_cmd()
        .args(["compress", "--level", "25"]) // Invalid level (max is 22)
        .arg(&source_file)
        .assert()
        .failure();
}

#[test]
fn test_verbose_output() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_file = temp_dir.path().join("verbose_test.txt");
    let output_file = temp_dir.path().join("verbose_test.zst");

    fs::write(&source_file, "Verbose test content")?;

    // Test verbose compression
    zzz_cmd()
        .args(["--verbose", "compress", "-o"])
        .arg(&output_file)
        .arg(&source_file)
        .assert()
        .success()
        .stdout(predicate::str::contains("compressing"))
        .stdout(predicate::str::contains("compressed"));

    Ok(())
}
