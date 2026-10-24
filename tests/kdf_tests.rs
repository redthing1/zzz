use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use std::io::Write;
use tempfile::TempDir;

#[test]
fn kdf_requires_password_and_zstd() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let input = dir.path().join("input.txt");
    fs::write(&input, "contents")?;
    cargo_bin_cmd!("zzz")
        .args(["c", "--kdf", "lo"])
        .arg(&input)
        .assert()
        .failure()
        .stderr(predicate::str::contains("--kdf requires"));
    for format in ["7z", "zip", "gz", "xz"] {
        cargo_bin_cmd!("zzz")
            .args(["c", "-p", "--kdf", "lo", "-f", format])
            .arg(&input)
            .assert()
            .failure()
            .stderr(predicate::str::contains("only supported for encrypted zst"));
    }
    cargo_bin_cmd!("zzz")
        .args(["c", "-p", "--kdf", "invalid"])
        .arg(&input)
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value"));
    Ok(())
}

#[test]
fn stdin_errors_do_not_create_or_overwrite_archives() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let input = dir.path().join("input.txt");
    let archive = dir.path().join("output.zst");
    fs::write(&input, "contents")?;
    for empty in ["", "\n", "\r\n"] {
        cargo_bin_cmd!("zzz")
            .args(["c", "--password-stdin", "-o"])
            .arg(&archive)
            .arg(&input)
            .write_stdin(empty)
            .assert()
            .failure()
            .stderr(predicate::str::contains("Password cannot be empty"));
        assert!(!archive.exists());
    }
    fs::write(&archive, "existing archive")?;
    cargo_bin_cmd!("zzz")
        .args(["c", "--password-stdin", "-o"])
        .arg(&archive)
        .arg(&input)
        .write_stdin("yes\nsecret\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("use --overwrite"));
    assert_eq!(fs::read_to_string(&archive)?, "existing archive");
    Ok(())
}

#[test]
fn old_encrypted_archives_still_extract_without_extension() -> anyhow::Result<()> {
    use argon2::{Algorithm, Argon2, Params, Version};
    use zzz_arc::encryption::EncryptingWriter;

    let dir = TempDir::new()?;
    let archive = dir.path().join("old_archive");
    let destination = dir.path().join("extracted");
    let contents = b"legacy archive payload";
    let mut tar = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(contents.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append_data(&mut header, "file.txt", &contents[..])?;
    let compressed = zstd::encode_all(&tar.into_inner()?[..], 1)?;

    // Use fixed v1 parameters to check compatibility independently of the defaults.
    let salt = [42_u8; 16];
    let mut key = [0_u8; 32];
    let params = Params::new(65536, 3, 4, Some(32)).unwrap();
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(b"legacy password", &salt, &mut key)
        .unwrap();
    let mut file = fs::File::create(&archive)?;
    file.write_all(b"ZSTDECRYPT1")?;
    file.write_all(&salt)?;
    let mut writer = EncryptingWriter::new(file, &key, 65536)?;
    writer.write_all(&compressed)?;
    writer.flush()?;
    drop(writer);

    cargo_bin_cmd!("zzz")
        .args(["x", "--password-stdin", "-C"])
        .arg(&destination)
        .arg(&archive)
        .write_stdin("legacy password\n")
        .assert()
        .success();
    assert_eq!(fs::read(destination.join("file.txt"))?, contents);
    cargo_bin_cmd!("zzz")
        .arg("t")
        .arg(&archive)
        .assert()
        .success();
    cargo_bin_cmd!("zzz")
        .arg("l")
        .arg(&archive)
        .assert()
        .failure()
        .stderr(predicate::str::contains("password required"));
    Ok(())
}

#[test]
fn malformed_kdf_headers_fail_before_deriving_a_key() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let archive = dir.path().join("malformed");
    let mut encoded = b"ZSTDECRYPT2".to_vec();
    encoded.extend_from_slice(&u32::MAX.to_be_bytes());
    encoded.extend_from_slice(&3_u32.to_be_bytes());
    encoded.extend_from_slice(&4_u32.to_be_bytes());
    encoded.extend_from_slice(&[0; 16]);
    fs::write(&archive, &encoded)?;
    for command in ["x", "l", "t"] {
        cargo_bin_cmd!("zzz")
            .arg(command)
            .arg(&archive)
            .assert()
            .failure()
            .stderr(predicate::str::contains("exceed supported resource limits"));
    }
    encoded[10] = b'9';
    fs::write(&archive, encoded)?;
    cargo_bin_cmd!("zzz")
        .arg("t")
        .arg(&archive)
        .assert()
        .failure()
        .stderr(predicate::str::contains("header version"));
    Ok(())
}

#[test]
#[ignore = "requires over 8 GiB RAM; run with --release --ignored --test-threads=1"]
fn medium_and_default_high_kdf_round_trip() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let input = dir.path().join("input.txt");
    fs::write(&input, "high-cost Argon2id round trip")?;
    for (preset, memory_kib) in [(Some("med"), 1024 * 1024_u32), (None, 8 * 1024 * 1024)] {
        let name = preset.unwrap_or("hi");
        // No extension: extraction must recognize the v2 magic.
        let archive = dir.path().join(name);
        let destination = dir.path().join(format!("extracted_{name}"));
        let mut command = cargo_bin_cmd!("zzz");
        command
            .args(["c", "--password-stdin", "-j1", "-f", "zst", "-o"])
            .arg(&archive)
            .arg(&input)
            .write_stdin("  secret páßphrase  \r\n");
        if let Some(preset) = preset {
            command.args(["--kdf", preset]);
        }
        command.assert().success();
        let bytes = fs::read(&archive)?;
        assert_eq!(&bytes[..11], b"ZSTDECRYPT2");
        assert_eq!(&bytes[11..15], &memory_kib.to_be_bytes());
        cargo_bin_cmd!("zzz")
            .args(["x", "--password-stdin", "-C"])
            .arg(&destination)
            .arg(&archive)
            .write_stdin("  secret páßphrase  \n")
            .assert()
            .success();
        assert_eq!(fs::read(destination.join("input.txt"))?, fs::read(&input)?);
    }
    Ok(())
}
