//! Encryption module for authenticated encryption of archives
//!
//! This module provides authenticated encryption for ZSTD archives using:
//! - Argon2id for key derivation from passwords
//! - AES-256-GCM for streaming authenticated encryption
//! - Chunked processing for memory efficiency

use crate::Result;
use anyhow::{anyhow, bail, Context};
use argon2::Argon2;
use rand::RngCore;
use std::io::{self, ErrorKind, Read};

// AES-GCM imports
use aes_gcm::{
    aead::{Aead, KeyInit, OsRng as AeadOsRng},
    Aes256Gcm, Nonce,
};

// AES-256-GCM constants
pub const AES_KEY_SIZE: usize = 32; // 256 bits
pub const NONCE_SIZE: usize = 12; // 96 bits, standard for GCM
pub const TAG_SIZE: usize = 16; // 128 bits, standard for GCM

// Argon2id parameters
pub const ARGON2_SALT_LEN: usize = 16; // Bytes

// Default Argon2 parameters (secure but reasonable performance)
pub const ARGON2_TIME_COST: u32 = 3;
pub const ARGON2_MEM_COST: u32 = 65536; // 64 MiB
pub const ARGON2_LANES: u32 = 4;

// Magic string to identify encrypted ZSTD files
pub const ENCRYPTED_ZSTD_MAGIC: &[u8; 11] = b"ZSTDECRYPT1";

// Size of the header for encrypted files = magic + salt_len
pub const ENCRYPTION_HEADER_SIZE: usize = ENCRYPTED_ZSTD_MAGIC.len() + ARGON2_SALT_LEN;

// Default chunk size for encryption (plaintext)
pub const DEFAULT_ENCRYPTION_CHUNK_SIZE: usize = 64 * 1024;

/// Derive an AES-256 key from a password using Argon2id
///
/// Returns (derived_key, salt) where salt is either the provided salt or a new random salt
pub fn derive_key(password: &str, salt_opt: Option<&[u8]>) -> Result<(Vec<u8>, Vec<u8>)> {
    let salt = match salt_opt {
        Some(s) => {
            if s.len() != ARGON2_SALT_LEN {
                return Err(anyhow!(
                    "Provided salt has incorrect length. Expected {}, got {}.",
                    ARGON2_SALT_LEN,
                    s.len()
                ));
            }
            s.to_vec()
        }
        None => {
            let mut new_salt = vec![0u8; ARGON2_SALT_LEN];
            rand::rngs::OsRng
                .try_fill_bytes(&mut new_salt)
                .context("Failed to generate random salt for Argon2")?;
            new_salt
        }
    };

    // Use Argon2id with specified parameters
    use argon2::{Algorithm, Params, Version};
    let params = Params::new(
        ARGON2_MEM_COST,
        ARGON2_TIME_COST,
        ARGON2_LANES,
        Some(AES_KEY_SIZE),
    )
    .map_err(|e| anyhow!("Failed to create Argon2 parameters: {}", e))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let mut derived_key = vec![0u8; AES_KEY_SIZE];
    argon2
        .hash_password_into(password.as_bytes(), &salt, &mut derived_key)
        .map_err(|e| anyhow!("Argon2 key derivation failed: {}", e))?;

    Ok((derived_key, salt))
}

/// A writer that encrypts data in chunks using AES-256-GCM before writing to the underlying writer
///
/// Each chunk is encrypted with a random nonce and written as:
/// [nonce: 12 bytes][ciphertext_length: 4 bytes BE][ciphertext_with_tag: variable]
pub struct EncryptingWriter<W: std::io::Write> {
    inner: W,
    cipher: Aes256Gcm,
    buffer: Vec<u8>,
    chunk_size: usize,
}

impl<W: std::io::Write> EncryptingWriter<W> {
    pub fn new(inner: W, key: &[u8], chunk_size: usize) -> Result<Self> {
        if key.len() != AES_KEY_SIZE {
            return Err(anyhow!(
                "Invalid key size for EncryptingWriter. Expected {}, got {}",
                AES_KEY_SIZE,
                key.len()
            ));
        }
        let cipher = Aes256Gcm::new_from_slice(key)
            .map_err(|e| anyhow!("Failed to initialize AES-GCM cipher: {}", e))?;
        Ok(Self {
            inner,
            cipher,
            buffer: Vec::with_capacity(chunk_size),
            chunk_size,
        })
    }

    fn encrypt_and_write_chunk(&mut self) -> Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }

        // Generate random nonce for this chunk
        let mut nonce_bytes = [0u8; NONCE_SIZE];
        AeadOsRng
            .try_fill_bytes(&mut nonce_bytes)
            .context("Failed to generate nonce for encryption")?;
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Encrypt the chunk (includes authentication tag)
        let ciphertext_with_tag = self
            .cipher
            .encrypt(nonce, self.buffer.as_slice())
            .map_err(|e| anyhow!("AES-GCM encryption failed: {}", e))?;

        // Write: nonce + length + ciphertext_with_tag
        self.inner
            .write_all(&nonce_bytes)
            .context("Failed to write nonce to inner writer")?;

        let len_bytes = (ciphertext_with_tag.len() as u32).to_be_bytes();
        self.inner
            .write_all(&len_bytes)
            .context("Failed to write ciphertext length to inner writer")?;

        self.inner
            .write_all(&ciphertext_with_tag)
            .context("Failed to write ciphertext with tag to inner writer")?;

        self.buffer.clear();
        Ok(())
    }
}

impl<W: std::io::Write> std::io::Write for EncryptingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut bytes_written_total = 0;
        let mut input_remaining = buf;

        while !input_remaining.is_empty() {
            let space_in_buffer = self.chunk_size - self.buffer.len();
            let bytes_to_buffer = std::cmp::min(space_in_buffer, input_remaining.len());

            self.buffer
                .extend_from_slice(&input_remaining[..bytes_to_buffer]);
            input_remaining = &input_remaining[bytes_to_buffer..];
            bytes_written_total += bytes_to_buffer;

            // If buffer is full, encrypt and write the chunk
            if self.buffer.len() == self.chunk_size {
                self.encrypt_and_write_chunk()
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
            }
        }
        Ok(bytes_written_total)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        // Encrypt and write any remaining data in buffer
        if !self.buffer.is_empty() {
            self.encrypt_and_write_chunk()
                .map_err(|e| std::io::Error::other(e.to_string()))?;
        }
        self.inner.flush()
    }
}

impl<W: std::io::Write> Drop for EncryptingWriter<W> {
    fn drop(&mut self) {
        if !self.buffer.is_empty() {
            if let Err(e) = self.encrypt_and_write_chunk() {
                eprintln!("Error encrypting remaining chunk during drop: {e}");
            }
        }
    }
}

/// A reader that decrypts chunked AES-256-GCM encrypted data
///
/// Reads chunks in format: [nonce: 12 bytes][ciphertext_length: 4 bytes BE][ciphertext_with_tag: variable]
pub struct DecryptingReader<R: Read> {
    inner: R,
    cipher: Aes256Gcm,
    buffer: Vec<u8>,
    buffer_pos: usize,
    eof_reached: bool,
}

impl<R: Read> DecryptingReader<R> {
    pub fn new(inner: R, key: &[u8]) -> Result<Self> {
        if key.len() != AES_KEY_SIZE {
            return Err(anyhow!(
                "Invalid key size for DecryptingReader. Expected {}, got {}",
                AES_KEY_SIZE,
                key.len()
            ));
        }
        let cipher = Aes256Gcm::new_from_slice(key)
            .map_err(|e| anyhow!("Failed to initialize AES-GCM cipher: {}", e))?;
        Ok(Self {
            inner,
            cipher,
            buffer: Vec::new(),
            buffer_pos: 0,
            eof_reached: false,
        })
    }

    fn read_and_decrypt_chunk(&mut self) -> Result<bool> {
        self.buffer.clear();
        self.buffer_pos = 0;

        // Read nonce
        let mut nonce_bytes = [0u8; NONCE_SIZE];
        match self.inner.read_exact(&mut nonce_bytes) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => {
                self.eof_reached = true;
                return Ok(false);
            }
            Err(e) => return Err(e).context("Failed to read nonce from inner reader"),
        }
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Read ciphertext length
        let mut len_bytes = [0u8; 4];
        match self.inner.read_exact(&mut len_bytes) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => {
                return Err(e)
                    .context("Failed to read ciphertext length (unexpected EOF after nonce)");
            }
            Err(e) => return Err(e).context("Failed to read ciphertext length from inner reader"),
        }
        let ct_with_tag_len = u32::from_be_bytes(len_bytes) as usize;

        if ct_with_tag_len < TAG_SIZE {
            bail!(
                "Invalid ciphertext length: {} (must be at least TAG_SIZE {})",
                ct_with_tag_len,
                TAG_SIZE
            );
        }

        // Read ciphertext with tag
        let mut ciphertext_with_tag = vec![0u8; ct_with_tag_len];
        self.inner
            .read_exact(&mut ciphertext_with_tag)
            .context("Failed to read ciphertext with tag from inner reader")?;

        // Decrypt and verify
        let plaintext = self
            .cipher
            .decrypt(nonce, ciphertext_with_tag.as_slice())
            .map_err(|e| {
                anyhow!(
                    "AES-GCM decryption failed (data integrity or key error): {}",
                    e
                )
            })?;

        self.buffer = plaintext;
        Ok(true)
    }
}

impl<R: Read> Read for DecryptingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // If we've consumed all buffered data, try to read and decrypt the next chunk
        if self.buffer_pos == self.buffer.len() {
            if self.eof_reached {
                return Ok(0);
            }
            match self.read_and_decrypt_chunk() {
                Ok(true) => {}
                Ok(false) => {
                    self.eof_reached = true;
                    return Ok(0);
                }
                Err(e) => {
                    let io_err_kind = if e.to_string().contains("AES-GCM decryption failed") {
                        ErrorKind::InvalidData
                    } else {
                        ErrorKind::Other
                    };
                    return Err(io::Error::new(
                        io_err_kind,
                        format!("Decryption error: {e}"),
                    ));
                }
            }
        }

        let bytes_to_read = std::cmp::min(buf.len(), self.buffer.len() - self.buffer_pos);

        if bytes_to_read == 0 && self.buffer_pos == self.buffer.len() && self.eof_reached {
            return Ok(0);
        }

        buf[..bytes_to_read]
            .copy_from_slice(&self.buffer[self.buffer_pos..self.buffer_pos + bytes_to_read]);
        self.buffer_pos += bytes_to_read;

        Ok(bytes_to_read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read, Write};

    #[test]
    fn test_derive_key_new_salt() {
        let password = "test_password";
        let (key1, salt1) = derive_key(password, None).unwrap();
        assert_eq!(key1.len(), AES_KEY_SIZE);
        assert_eq!(salt1.len(), ARGON2_SALT_LEN);

        let (key2, salt2) = derive_key(password, None).unwrap();
        assert_eq!(key2.len(), AES_KEY_SIZE);
        assert_eq!(salt2.len(), ARGON2_SALT_LEN);
        assert_ne!(salt1, salt2);
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_derive_key_with_provided_salt() {
        let password = "test_password";
        let mut salt = vec![0u8; ARGON2_SALT_LEN];
        rand::rngs::OsRng.try_fill_bytes(&mut salt).unwrap();

        let (key1, used_salt1) = derive_key(password, Some(&salt)).unwrap();
        assert_eq!(key1.len(), AES_KEY_SIZE);
        assert_eq!(used_salt1, salt);

        let (key2, used_salt2) = derive_key(password, Some(&salt)).unwrap();
        assert_eq!(key2, key1);
        assert_eq!(used_salt2, salt);
    }

    #[test]
    fn test_derive_key_invalid_salt_length() {
        let password = "test_password";
        let invalid_salt = vec![0u8; ARGON2_SALT_LEN - 1];
        let result = derive_key(password, Some(&invalid_salt));
        assert!(result.is_err());
    }

    #[test]
    fn test_encrypting_writer_simple() -> Result<()> {
        let key = [0u8; AES_KEY_SIZE];
        let mut output_buffer = Vec::new();
        let chunk_size = 16;

        {
            let mut writer = EncryptingWriter::new(&mut output_buffer, &key, chunk_size)?;
            writer.write_all(b"test data that is longer than one chunk")?; // 38 bytes
            writer.flush()?;
        }
        // Should have encrypted data (nonce + length + ciphertext_with_tag for each chunk)
        assert!(!output_buffer.is_empty());
        Ok(())
    }

    #[test]
    fn test_decrypting_reader_simple_roundtrip() -> Result<()> {
        let key = [3u8; AES_KEY_SIZE];
        let original_data = b"Hello, world! This is a test of the decryption system.";
        let chunk_size = 16;

        let mut encrypted_data = Vec::new();
        {
            let mut enc_writer = EncryptingWriter::new(&mut encrypted_data, &key, chunk_size)?;
            enc_writer.write_all(original_data)?;
            enc_writer.flush()?;
        }

        assert!(!encrypted_data.is_empty());

        let mut dec_reader = DecryptingReader::new(Cursor::new(&encrypted_data), &key)?;
        let mut decrypted_data = Vec::new();
        dec_reader.read_to_end(&mut decrypted_data)?;

        assert_eq!(original_data.to_vec(), decrypted_data);
        Ok(())
    }

    #[test]
    fn test_decrypting_reader_wrong_key() -> Result<()> {
        let key_encrypt = [7u8; AES_KEY_SIZE];
        let key_decrypt = [8u8; AES_KEY_SIZE];
        let original_data = b"secret message";
        let chunk_size = 128;

        let mut encrypted_data = Vec::new();
        {
            let mut enc_writer =
                EncryptingWriter::new(&mut encrypted_data, &key_encrypt, chunk_size)?;
            enc_writer.write_all(original_data)?;
        }

        let mut dec_reader = DecryptingReader::new(Cursor::new(&encrypted_data), &key_decrypt)?;
        let mut decrypted_data = Vec::new();
        let result = dec_reader.read_to_end(&mut decrypted_data);

        assert!(result.is_err());
        if let Err(e) = result {
            assert!(
                e.to_string().contains("AES-GCM decryption failed")
                    || e.to_string().contains("Decryption error")
            );
        }
        Ok(())
    }
}
