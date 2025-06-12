use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn zzz_cmd() -> Command {
    Command::cargo_bin("zzz").unwrap()
}

// Helper to create a dummy file
fn create_test_file(dir: &TempDir, filename: &str, content: &str) -> std::io::Result<std::path::PathBuf> {
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
    let test_file_path = create_test_file(&tmp_dir, "not_an_archive.txt", "This is not an archive.")?;

    zzz_cmd()
        .arg("test")
        .arg(test_file_path)
        .assert()
        .failure()
        .stderr(predicate::str::contains("unsupported archive format").or(predicate::str::contains("failed to determine mime type")));
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
    ("tgz", "archive.tgz"), // .tar.gz
    ("txz", "archive.txz"), // .tar.xz
];

#[test]
fn test_command_on_valid_archives_no_password() -> Result<()> {
    for (format_ext, archive_name_template) in ARCHIVE_FORMATS_FOR_TEST_CMD {
        println!("Testing 'test' command for format: {}", format_ext);
        let tmp_dir = TempDir::new()?;
        let input_dir = tmp_dir.path().join("input_for_test_cmd");
        fs::create_dir_all(&input_dir)?;

        let file1_path = input_dir.join("file1.txt");
        fs::write(&file1_path, "Hello from file1 for test command!")?;
        let _file2_path = input_dir.join("file2.txt"); // may not be used by single file formats
        fs::write(&_file2_path, "Hello from file2 for test command!")?;

        let archive_path = tmp_dir.path().join(archive_name_template);

        let files_to_compress: Vec<&std::path::Path> =
            if ["gz", "xz", "zst"].contains(format_ext) {
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
            .stdout(predicate::str::contains(format!("{} integrity: OK", archive_path.display())));
    }
    Ok(())
}

const ARCHIVE_FORMATS_WITH_PASSWORD: &[(&str, &str)] = &[
    ("7z", "pwd_archive.7z"),
];

#[test]
fn test_password_protection_flows() -> Result<()> {
    for (format_ext, archive_name) in ARCHIVE_FORMATS_WITH_PASSWORD {
        println!("Testing password protection for format: {}", format_ext);
        let tmp_dir = TempDir::new()?;
        let input_dir = tmp_dir.path().join("input_pwd");
        fs::create_dir_all(&input_dir)?;

        let file1_original_path = input_dir.join("file1.txt");
        let file1_content = format!("Content for password test {} file1", format_ext);
        fs::write(&file1_original_path, &file1_content)?;

        let file2_original_path = input_dir.join("file2.txt");
        let file2_content = format!("Content for password test {} file2", format_ext);
        fs::write(&file2_original_path, &file2_content)?;

        let archive_path = tmp_dir.path().join(archive_name);
        let password = "testpassword123";

        // 1. Compress with password
        println!("  Compressing with password...");
        run_compress_command(&archive_path, &[input_dir.as_path()], Some(format_ext), Some(password))?;
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

        assert_eq!(fs::read_to_string(extract_dir_correct.join("input_pwd/file1.txt"))?, file1_content);
        assert_eq!(fs::read_to_string(extract_dir_correct.join("input_pwd/file2.txt"))?, file2_content);

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
            .stderr(predicate::str::contains("Failed to decrypt").or(predicate::str::contains("password required").or(predicate::str::contains("invalid password")))); // Error message might vary

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
            .stderr(predicate::str::contains("password was provided").or(predicate::str::contains("password required"))); // Error message might vary

        // 5. 'test' command on password-protected archive
        println!("  Running 'test' command...");
        let test_cmd_assert = zzz_cmd()
            .arg("test")
            .arg(&archive_path)
            .assert();

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
                .stdout(predicate::str::contains(format!("{} integrity: OK", archive_path.display())));
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
        .stderr(predicate::str::contains("Password protection is not supported for ZIP format"));
    
    Ok(())
}