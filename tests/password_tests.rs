use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn zzz_cmd() -> Command {
    Command::cargo_bin("zzz").unwrap()
}

// Helper to create a dummy file
fn create_test_file(
    dir: &TempDir,
    filename: &str,
    content: &str,
) -> std::io::Result<std::path::PathBuf> {
    let file_path = dir.path().join(filename);
    std::fs::write(&file_path, content)?;
    Ok(file_path)
}

// Helper function to run zzz compress for testing purposes
fn run_compress_command(
    output_archive: &std::path::Path,
    input_paths: &[&std::path::Path],
    format_arg: Option<&str>,
    password: Option<&str>,
) -> Result<()> {
    let mut cmd = zzz_cmd();
    cmd.arg("compress");
    for path in input_paths {
        cmd.arg(path);
    }
    cmd.arg("-o").arg(output_archive);

    if let Some(fmt) = format_arg {
        cmd.arg("-f").arg(fmt);
    }
    if let Some(pass) = password {
        cmd.arg("--password").arg(pass);
    }

    cmd.assert().success();
    Ok(())
}

#[test]
fn test_command_on_non_archive_file() -> Result<()> {
    let tmp_dir = TempDir::new()?;
    let test_file_path =
        create_test_file(&tmp_dir, "not_an_archive.txt", "This is not an archive.")?;

    zzz_cmd()
        .arg("test")
        .arg(test_file_path)
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("unsupported archive format")
                .or(predicate::str::contains("failed to determine mime type")),
        );
    Ok(())
}

// Archive types to test for the 'test' command (no password)
// Format extension and expected archive name (can be same if simple)
const ARCHIVE_FORMATS_FOR_TEST_CMD: &[(&str, &str)] = &[
    ("zip", "archive.zip"),
    ("7z", "archive.7z"),
    ("gz", "file.txt.gz"),   // Single file .gz
    ("xz", "file.txt.xz"),   // Single file .xz
    ("zst", "file.txt.zst"), // Single file .zst
    ("tgz", "archive.tgz"),  // .tar.gz
    ("txz", "archive.txz"),  // .tar.xz
];

#[test]
fn test_command_on_valid_archives_no_password() -> Result<()> {
    for (format_ext, archive_name_template) in ARCHIVE_FORMATS_FOR_TEST_CMD {
        println!("Testing 'test' command for format: {format_ext}");
        let tmp_dir = TempDir::new()?;
        let input_dir = tmp_dir.path().join("input_for_test_cmd");
        fs::create_dir_all(&input_dir)?;

        let file1_path = input_dir.join("file1.txt");
        fs::write(&file1_path, "Hello from file1 for test command!")?;
        let _file2_path = input_dir.join("file2.txt"); // may not be used by single file formats
        fs::write(&_file2_path, "Hello from file2 for test command!")?;

        let archive_path = tmp_dir.path().join(archive_name_template);

        let files_to_compress: Vec<&std::path::Path> = if ["gz", "xz", "zst"].contains(format_ext) {
            // Single file compression
            vec![&file1_path]
        } else {
            // Directory compression for tarballs, zip, 7z
            vec![input_dir.as_path()]
        };

        run_compress_command(&archive_path, &files_to_compress, Some(format_ext), None)?;

        zzz_cmd()
            .arg("test")
            .arg(&archive_path)
            .assert()
            .success()
            .stdout(predicate::str::contains(format!(
                "{} integrity: OK",
                archive_path.display()
            )));
    }
    Ok(())
}

const ARCHIVE_FORMATS_WITH_PASSWORD: &[(&str, &str)] =
    &[("7z", "pwd_archive.7z"), ("zst", "pwd_archive.zst")];

#[test]
fn test_password_protection_flows() -> Result<()> {
    for (format_ext, archive_name) in ARCHIVE_FORMATS_WITH_PASSWORD {
        println!("Testing password protection for format: {format_ext}");
        let tmp_dir = TempDir::new()?;
        let input_dir = tmp_dir.path().join("input_pwd");
        fs::create_dir_all(&input_dir)?;

        let file1_original_path = input_dir.join("file1.txt");
        let file1_content = format!("Content for password test {format_ext} file1");
        fs::write(&file1_original_path, &file1_content)?;

        let file2_original_path = input_dir.join("file2.txt");
        let file2_content = format!("Content for password test {format_ext} file2");
        fs::write(&file2_original_path, &file2_content)?;

        let archive_path = tmp_dir.path().join(archive_name);
        let password = "testpassword123";

        // 1. Compress with password
        println!("  Compressing with password...");
        run_compress_command(
            &archive_path,
            &[input_dir.as_path()],
            Some(format_ext),
            Some(password),
        )?;
        assert!(archive_path.exists());

        // 2. Extract with correct password
        println!("  Extracting with correct password...");
        let extract_dir_correct = tmp_dir.path().join("extract_correct_pwd");
        fs::create_dir(&extract_dir_correct)?;
        zzz_cmd()
            .args(["extract", "--password", password, "-C"])
            .arg(&extract_dir_correct)
            .arg(&archive_path)
            .assert()
            .success();

        assert_eq!(
            fs::read_to_string(extract_dir_correct.join("input_pwd/file1.txt"))?,
            file1_content
        );
        assert_eq!(
            fs::read_to_string(extract_dir_correct.join("input_pwd/file2.txt"))?,
            file2_content
        );

        // 3. Extract with incorrect password
        println!("  Extracting with incorrect password...");
        let extract_dir_incorrect = tmp_dir.path().join("extract_incorrect_pwd");
        fs::create_dir(&extract_dir_incorrect)?;
        zzz_cmd()
            .args(["extract", "--password", "wrongpassword", "-C"])
            .arg(&extract_dir_incorrect)
            .arg(&archive_path)
            .assert()
            .failure()
            .stderr(
                predicate::str::contains("Failed to decrypt")
                    .or(predicate::str::contains("password required"))
                    .or(predicate::str::contains("invalid password"))
                    .or(predicate::str::contains("Decryption error")),
            ); // Error message might vary by format

        // 4. Extract password-protected archive without password
        println!("  Extracting without password...");
        let extract_dir_no_pwd = tmp_dir.path().join("extract_no_pwd");
        fs::create_dir(&extract_dir_no_pwd)?;
        zzz_cmd()
            .args(["extract", "-C"])
            .arg(&extract_dir_no_pwd)
            .arg(&archive_path)
            .assert()
            .failure()
            .stderr(
                predicate::str::contains("password was provided")
                    .or(predicate::str::contains("password required"))
                    .or(predicate::str::contains("requires a password")),
            ); // Error message might vary by format

        // 5. 'test' command on password-protected archive
        println!("  Running 'test' command...");
        let test_cmd_assert = zzz_cmd().arg("test").arg(&archive_path).assert();

        if *format_ext == "7z" {
            // For 7z, if headers are encrypted (common with password), 'test' without password should fail.
            // sevenz-rust's open with empty password will fail if headers are encrypted.
            test_cmd_assert
                .failure()
                .stderr(predicate::str::contains("Failed to open 7-Zip archive"));
        } else {
            // For ZIP, ZipCrypto doesn't encrypt headers, so basic listing/test should pass.
            test_cmd_assert
                .success()
                .stdout(predicate::str::contains(format!(
                    "{} integrity: OK",
                    archive_path.display()
                )));
        }
    }
    Ok(())
}

#[test]
fn test_zip_password_rejection() -> Result<()> {
    let tmp_dir = TempDir::new()?;
    let input_file = create_test_file(&tmp_dir, "test.txt", "test content")?;
    let output_archive = tmp_dir.path().join("test.zip");

    // Test that compressing with password fails for ZIP
    zzz_cmd()
        .args(["compress", "--password", "test123", "-f", "zip", "-o"])
        .arg(&output_archive)
        .arg(&input_file)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Password protection is not supported for ZIP format",
        ));

    Ok(())
}

#[test]
fn test_zstd_password_support() -> Result<()> {
    let tmp_dir = TempDir::new()?;
    let input_file = create_test_file(&tmp_dir, "test.txt", "ZSTD password test content")?;
    let output_archive = tmp_dir.path().join("test.zst");

    // Test that compressing with password succeeds for ZSTD
    zzz_cmd()
        .args(["compress", "--password", "zstdtest123", "-f", "zst", "-o"])
        .arg(&output_archive)
        .arg(&input_file)
        .assert()
        .success();

    assert!(output_archive.exists());

    // Test extraction with correct password
    let extract_dir = tmp_dir.path().join("extract");
    fs::create_dir(&extract_dir)?;
    zzz_cmd()
        .args(["extract", "--password", "zstdtest123", "-C"])
        .arg(&extract_dir)
        .arg(&output_archive)
        .assert()
        .success();

    // Verify extracted content
    let extracted_content = fs::read_to_string(extract_dir.join("test.txt"))?;
    assert_eq!(extracted_content, "ZSTD password test content");

    Ok(())
}

#[test]
fn test_zstd_encryption_comprehensive() -> Result<()> {
    let tmp_dir = TempDir::new()?;

    // Create a complex directory structure for testing
    let input_dir = tmp_dir.path().join("complex_input");
    fs::create_dir_all(input_dir.join("subdir1"))?;
    fs::create_dir_all(input_dir.join("subdir2/nested"))?;
    fs::create_dir_all(input_dir.join("empty_dir"))?;

    // Create various file types and sizes
    fs::write(input_dir.join("small.txt"), "small file content")?;
    fs::write(input_dir.join("subdir1/medium.txt"), "x".repeat(1024))?; // 1KB
    fs::write(input_dir.join("subdir2/large.txt"), "y".repeat(100_000))?; // 100KB
    fs::write(
        input_dir.join("subdir2/nested/binary.bin"),
        [0u8, 1u8, 255u8, 127u8],
    )?;
    fs::write(input_dir.join("unicode.txt"), "Hello 世界 🌍 тест")?;
    fs::write(input_dir.join("empty.txt"), "")?; // Empty file

    let output_archive = tmp_dir.path().join("complex.zst");
    let password = "complex_test_password_123!@#";

    // Compress with password
    zzz_cmd()
        .args(["compress", "--password", password, "-f", "zst", "-o"])
        .arg(&output_archive)
        .arg(&input_dir)
        .assert()
        .success();

    assert!(output_archive.exists());

    // Extract with correct password
    let extract_dir = tmp_dir.path().join("extracted");
    fs::create_dir(&extract_dir)?;
    zzz_cmd()
        .args(["extract", "--password", password, "-C"])
        .arg(&extract_dir)
        .arg(&output_archive)
        .assert()
        .success();

    // Verify all files and directories exist and have correct content
    assert!(extract_dir.join("complex_input/small.txt").exists());
    assert!(extract_dir
        .join("complex_input/subdir1/medium.txt")
        .exists());
    assert!(extract_dir.join("complex_input/subdir2/large.txt").exists());
    assert!(extract_dir
        .join("complex_input/subdir2/nested/binary.bin")
        .exists());
    assert!(extract_dir.join("complex_input/unicode.txt").exists());
    assert!(extract_dir.join("complex_input/empty.txt").exists());
    assert!(extract_dir.join("complex_input/empty_dir").is_dir());

    // Verify content integrity
    assert_eq!(
        fs::read_to_string(extract_dir.join("complex_input/small.txt"))?,
        "small file content"
    );
    assert_eq!(
        fs::read_to_string(extract_dir.join("complex_input/subdir1/medium.txt"))?,
        "x".repeat(1024)
    );
    assert_eq!(
        fs::read_to_string(extract_dir.join("complex_input/subdir2/large.txt"))?,
        "y".repeat(100_000)
    );
    assert_eq!(
        fs::read(extract_dir.join("complex_input/subdir2/nested/binary.bin"))?,
        vec![0u8, 1u8, 255u8, 127u8]
    );
    assert_eq!(
        fs::read_to_string(extract_dir.join("complex_input/unicode.txt"))?,
        "Hello 世界 🌍 тест"
    );
    assert_eq!(
        fs::read_to_string(extract_dir.join("complex_input/empty.txt"))?,
        ""
    );

    Ok(())
}

#[test]
fn test_zstd_encryption_edge_cases() -> Result<()> {
    let tmp_dir = TempDir::new()?;

    // Test with empty password (should be rejected)
    let input_file = create_test_file(&tmp_dir, "test.txt", "content")?;
    let output_archive = tmp_dir.path().join("empty_pass.zst");

    zzz_cmd()
        .args(["compress", "--password", "", "-f", "zst", "-o"])
        .arg(&output_archive)
        .arg(&input_file)
        .assert()
        .success(); // Empty password should still work (just means no encryption)

    // Test with very long password
    let long_password = "a".repeat(1000);
    let output_archive2 = tmp_dir.path().join("long_pass.zst");

    zzz_cmd()
        .args(["compress", "--password", &long_password, "-f", "zst", "-o"])
        .arg(&output_archive2)
        .arg(&input_file)
        .assert()
        .success();

    // Test extraction with long password
    let extract_dir = tmp_dir.path().join("extract_long");
    fs::create_dir(&extract_dir)?;
    zzz_cmd()
        .args(["extract", "--password", &long_password, "-C"])
        .arg(&extract_dir)
        .arg(&output_archive2)
        .assert()
        .success();

    // Test with special characters in password
    let special_password = "páßwörd!@#$%^&*()_+-={}[]|\\:;\"'<>?,./~`";
    let output_archive3 = tmp_dir.path().join("special_pass.zst");

    zzz_cmd()
        .args([
            "compress",
            "--password",
            special_password,
            "-f",
            "zst",
            "-o",
        ])
        .arg(&output_archive3)
        .arg(&input_file)
        .assert()
        .success();

    let extract_dir2 = tmp_dir.path().join("extract_special");
    fs::create_dir(&extract_dir2)?;
    zzz_cmd()
        .args(["extract", "--password", special_password, "-C"])
        .arg(&extract_dir2)
        .arg(&output_archive3)
        .assert()
        .success();

    Ok(())
}

#[test]
fn test_zstd_encryption_vs_standard() -> Result<()> {
    let tmp_dir = TempDir::new()?;
    let input_file = create_test_file(&tmp_dir, "compare.txt", "Content for comparison testing")?;

    // Create standard (unencrypted) archive
    let standard_archive = tmp_dir.path().join("standard.zst");
    zzz_cmd()
        .args(["compress", "-f", "zst", "-o"])
        .arg(&standard_archive)
        .arg(&input_file)
        .assert()
        .success();

    // Create encrypted archive
    let encrypted_archive = tmp_dir.path().join("encrypted.zst");
    zzz_cmd()
        .args(["compress", "--password", "testpass", "-f", "zst", "-o"])
        .arg(&encrypted_archive)
        .arg(&input_file)
        .assert()
        .success();

    // Verify both archives exist and have different sizes
    assert!(standard_archive.exists());
    assert!(encrypted_archive.exists());

    let standard_size = fs::metadata(&standard_archive)?.len();
    let encrypted_size = fs::metadata(&encrypted_archive)?.len();

    // Encrypted archive should be larger due to encryption headers and auth tags
    assert!(
        encrypted_size > standard_size,
        "Encrypted archive ({encrypted_size} bytes) should be larger than standard archive ({standard_size} bytes)"
    );

    // Test that encrypted archive fails without password
    zzz_cmd()
        .args(["extract", "-C", "should_fail"])
        .arg(&encrypted_archive)
        .assert()
        .failure()
        .stderr(predicate::str::contains("requires a password"));

    // Test that standard archive works without password
    let extract_standard = tmp_dir.path().join("extract_standard");
    fs::create_dir(&extract_standard)?;
    zzz_cmd()
        .args(["extract", "-C"])
        .arg(&extract_standard)
        .arg(&standard_archive)
        .assert()
        .success();

    // Test that providing password to standard archive shows warning but works
    let extract_standard_with_pass = tmp_dir.path().join("extract_standard_pass");
    fs::create_dir(&extract_standard_with_pass)?;
    zzz_cmd()
        .args(["extract", "--password", "unused", "-C"])
        .arg(&extract_standard_with_pass)
        .arg(&standard_archive)
        .assert()
        .success()
        .stderr(predicate::str::contains("warning"));

    Ok(())
}

#[test]
fn test_zstd_encryption_list_command() -> Result<()> {
    let tmp_dir = TempDir::new()?;
    let input_file = create_test_file(&tmp_dir, "listtest.txt", "Content for list testing")?;

    // Create standard archive
    let standard_archive = tmp_dir.path().join("standard_list.zst");
    zzz_cmd()
        .args(["compress", "-f", "zst", "-o"])
        .arg(&standard_archive)
        .arg(&input_file)
        .assert()
        .success();

    // Create encrypted archive
    let encrypted_archive = tmp_dir.path().join("encrypted_list.zst");
    zzz_cmd()
        .args(["compress", "--password", "listpass", "-f", "zst", "-o"])
        .arg(&encrypted_archive)
        .arg(&input_file)
        .assert()
        .success();

    // Test list command on standard archive works
    zzz_cmd()
        .args(["list"])
        .arg(&standard_archive)
        .assert()
        .success()
        .stdout(predicate::str::contains("listtest.txt"));

    // Test list command on encrypted archive fails
    zzz_cmd()
        .args(["list"])
        .arg(&encrypted_archive)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Cannot list encrypted ZSTD archive",
        ));

    Ok(())
}

#[test]
fn test_zstd_encryption_test_command() -> Result<()> {
    let tmp_dir = TempDir::new()?;
    let input_file = create_test_file(&tmp_dir, "testcmd.txt", "Content for test command testing")?;

    // Create standard archive
    let standard_archive = tmp_dir.path().join("standard_test.zst");
    zzz_cmd()
        .args(["compress", "-f", "zst", "-o"])
        .arg(&standard_archive)
        .arg(&input_file)
        .assert()
        .success();

    // Create encrypted archive
    let encrypted_archive = tmp_dir.path().join("encrypted_test.zst");
    zzz_cmd()
        .args(["compress", "--password", "testcmdpass", "-f", "zst", "-o"])
        .arg(&encrypted_archive)
        .arg(&input_file)
        .assert()
        .success();

    // Test 'test' command on standard archive
    zzz_cmd()
        .args(["test"])
        .arg(&standard_archive)
        .assert()
        .success()
        .stdout(predicate::str::contains("integrity: OK"));

    // Test 'test' command on encrypted archive (should pass - format validation only)
    zzz_cmd()
        .args(["test"])
        .arg(&encrypted_archive)
        .assert()
        .success()
        .stdout(predicate::str::contains("integrity: OK"));

    Ok(())
}

#[test]
fn test_zstd_encryption_compression_levels() -> Result<()> {
    let tmp_dir = TempDir::new()?;
    let input_file = create_test_file(&tmp_dir, "levels.txt", &"x".repeat(10000))?; // 10KB for better compression testing

    // Test different compression levels with encryption
    for level in [1, 5, 10, 15, 20] {
        let output_archive = tmp_dir.path().join(format!("level_{level}.zst"));

        zzz_cmd()
            .args([
                "compress",
                "--password",
                "leveltest",
                "-f",
                "zst",
                "--level",
                &level.to_string(),
                "-o",
            ])
            .arg(&output_archive)
            .arg(&input_file)
            .assert()
            .success();

        assert!(output_archive.exists());

        // Test extraction
        let extract_dir = tmp_dir.path().join(format!("extract_{level}"));
        fs::create_dir(&extract_dir)?;
        zzz_cmd()
            .args(["extract", "--password", "leveltest", "-C"])
            .arg(&extract_dir)
            .arg(&output_archive)
            .assert()
            .success();

        // Verify content
        let extracted_content = fs::read_to_string(extract_dir.join("levels.txt"))?;
        assert_eq!(extracted_content, "x".repeat(10000));
    }

    Ok(())
}

#[test]
fn test_zstd_encryption_corruption_handling() -> Result<()> {
    let tmp_dir = TempDir::new()?;
    let input_file = create_test_file(&tmp_dir, "corrupt.txt", "Content to be corrupted")?;

    // Create encrypted archive
    let archive_path = tmp_dir.path().join("corrupt_test.zst");
    zzz_cmd()
        .args(["compress", "--password", "corrupttest", "-f", "zst", "-o"])
        .arg(&archive_path)
        .arg(&input_file)
        .assert()
        .success();

    // Read the archive data
    let original_data = fs::read(&archive_path)?;
    assert!(
        original_data.len() > 50,
        "Archive should be reasonably sized for corruption testing"
    );

    // Corrupt different parts of the archive
    let corruption_positions = [
        20,                       // Header corruption
        original_data.len() / 2,  // Middle corruption
        original_data.len() - 20, // End corruption (auth tag area)
    ];

    for (i, &pos) in corruption_positions.iter().enumerate() {
        let mut corrupted_data = original_data.clone();
        if pos < corrupted_data.len() {
            corrupted_data[pos] ^= 0xFF; // Flip all bits
        }

        let corrupted_archive = tmp_dir.path().join(format!("corrupted_{i}.zst"));
        fs::write(&corrupted_archive, &corrupted_data)?;

        // Try to extract corrupted archive
        let extract_dir = tmp_dir.path().join(format!("extract_corrupt_{i}"));
        fs::create_dir(&extract_dir)?;

        zzz_cmd()
            .args(["extract", "--password", "corrupttest", "-C"])
            .arg(&extract_dir)
            .arg(&corrupted_archive)
            .assert()
            .failure()
            .stderr(
                predicate::str::contains("Decryption error")
                    .or(predicate::str::contains("Authentication failed"))
                    .or(predicate::str::contains("Failed to decrypt"))
                    .or(predicate::str::contains("corruption")),
            );
    }

    Ok(())
}

#[test]
fn test_zstd_encryption_large_files() -> Result<()> {
    let tmp_dir = TempDir::new()?;

    // Create a larger file (1MB) to test streaming encryption
    let large_content = "A".repeat(1024 * 1024); // 1MB
    let input_file = tmp_dir.path().join("large.txt");
    fs::write(&input_file, &large_content)?;

    let archive_path = tmp_dir.path().join("large.zst");
    let password = "largefiletest";

    // Compress large file with encryption
    zzz_cmd()
        .args(["compress", "--password", password, "-f", "zst", "-o"])
        .arg(&archive_path)
        .arg(&input_file)
        .assert()
        .success();

    assert!(archive_path.exists());

    // Extract and verify
    let extract_dir = tmp_dir.path().join("extract_large");
    fs::create_dir(&extract_dir)?;
    zzz_cmd()
        .args(["extract", "--password", password, "-C"])
        .arg(&extract_dir)
        .arg(&archive_path)
        .assert()
        .success();

    // Verify content integrity
    let extracted_content = fs::read_to_string(extract_dir.join("large.txt"))?;
    assert_eq!(extracted_content.len(), large_content.len());
    assert_eq!(extracted_content, large_content);

    Ok(())
}

#[test]
fn test_zstd_encryption_multiple_files() -> Result<()> {
    let tmp_dir = TempDir::new()?;

    // Create a directory with multiple files (since ZSTD format compresses directories, not individual files)
    let input_dir = tmp_dir.path().join("multi_input");
    fs::create_dir_all(&input_dir)?;

    let files = vec![
        ("file1.txt", "Content of file 1"),
        ("file2.txt", "Content of file 2"),
        ("file3.txt", "Content of file 3"),
    ];

    for (name, content) in &files {
        fs::write(input_dir.join(name), content)?;
    }

    let archive_path = tmp_dir.path().join("multi.zst");
    let password = "multifiletest";

    // Compress directory with multiple files with encryption
    zzz_cmd()
        .args(["compress", "--password", password, "-f", "zst", "-o"])
        .arg(&archive_path)
        .arg(&input_dir)
        .assert()
        .success();

    assert!(archive_path.exists());

    // Extract and verify all files
    let extract_dir = tmp_dir.path().join("extract_multi");
    fs::create_dir(&extract_dir)?;
    zzz_cmd()
        .args(["extract", "--password", password, "-C"])
        .arg(&extract_dir)
        .arg(&archive_path)
        .assert()
        .success();

    // Verify all files were extracted correctly
    for (name, expected_content) in &files {
        let extracted_content = fs::read_to_string(extract_dir.join("multi_input").join(name))?;
        assert_eq!(extracted_content, *expected_content);
    }

    Ok(())
}

#[test]
fn test_zstd_encryption_performance_comparison() -> Result<()> {
    let tmp_dir = TempDir::new()?;

    // Create a moderately sized file for performance testing
    let test_content = "Performance test data ".repeat(10000); // ~240KB
    let input_file = create_test_file(&tmp_dir, "perf.txt", &test_content)?;

    let standard_archive = tmp_dir.path().join("perf_standard.zst");
    let encrypted_archive = tmp_dir.path().join("perf_encrypted.zst");

    // Create standard archive
    let start = std::time::Instant::now();
    zzz_cmd()
        .args(["compress", "-f", "zst", "-o"])
        .arg(&standard_archive)
        .arg(&input_file)
        .assert()
        .success();
    let standard_time = start.elapsed();

    // Create encrypted archive
    let start = std::time::Instant::now();
    zzz_cmd()
        .args(["compress", "--password", "perftest", "-f", "zst", "-o"])
        .arg(&encrypted_archive)
        .arg(&input_file)
        .assert()
        .success();
    let encrypted_time = start.elapsed();

    // Both should complete successfully
    assert!(standard_archive.exists());
    assert!(encrypted_archive.exists());

    // Get file sizes
    let standard_size = fs::metadata(&standard_archive)?.len();
    let encrypted_size = fs::metadata(&encrypted_archive)?.len();

    println!("Performance comparison:");
    println!("  Standard: {standard_size} bytes, {standard_time:?}");
    println!("  Encrypted: {encrypted_size} bytes, {encrypted_time:?}");

    // Encrypted should be larger but not excessively so (allow up to 2x for small files due to encryption headers)
    let size_overhead = encrypted_size as f64 / standard_size as f64;
    assert!(
        size_overhead < 3.0,
        "Encryption overhead too high: {size_overhead}x"
    );

    // Time overhead should be reasonable for Argon2 (key derivation is intentionally slow)
    // For small files, Argon2 dominates the time, so we allow up to 30 seconds total
    assert!(
        encrypted_time.as_secs() < 30,
        "Encryption took too long: {encrypted_time:?}"
    );

    Ok(())
}

#[test]
fn test_zstd_encryption_format_validation() -> Result<()> {
    let tmp_dir = TempDir::new()?;
    let input_file = create_test_file(&tmp_dir, "format.txt", "Format validation content")?;

    // Create encrypted archive
    let archive_path = tmp_dir.path().join("format_test.zst");
    zzz_cmd()
        .args(["compress", "--password", "formattest", "-f", "zst", "-o"])
        .arg(&archive_path)
        .arg(&input_file)
        .assert()
        .success();

    // Read first few bytes to check for magic header
    let archive_data = fs::read(&archive_path)?;
    assert!(
        archive_data.len() > 20,
        "Archive should have reasonable size"
    );

    // Check for our custom magic header "ZSTDECRYPT1"
    let magic_header = b"ZSTDECRYPT1";
    assert_eq!(
        &archive_data[0..magic_header.len()],
        magic_header,
        "Archive should start with ZSTDECRYPT1 magic header"
    );

    // Verify it's not a standard ZSTD file (which would start with 0x28, 0xB5, 0x2F, 0xFD)
    let zstd_magic = &[0x28, 0xB5, 0x2F, 0xFD];
    assert_ne!(
        &archive_data[0..4],
        zstd_magic,
        "Encrypted archive should not start with standard ZSTD magic"
    );

    Ok(())
}
