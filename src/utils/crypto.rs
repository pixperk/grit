use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use anyhow::{Context, Result};
use rand::RngCore;
use std::fs;
use std::path::Path;

const KEY_FILE: &str = "encryption.key";
const NONCE_SIZE: usize = 12;

fn get_or_create_key(plr_dir: &Path) -> Result<Vec<u8>> {
    let key_path = plr_dir.join(KEY_FILE);

    if key_path.exists() {
        let key = fs::read(&key_path).context("Failed to read encryption key")?;
        if key.len() != 32 {
            anyhow::bail!("Invalid encryption key size");
        }
        Ok(key)
    } else {
        let mut key = vec![0u8; 32];
        OsRng.fill_bytes(&mut key);

        fs::create_dir_all(plr_dir)?;
        fs::write(&key_path, &key).context("Failed to write encryption key")?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600))?;
        }

        Ok(key)
    }
}

pub fn encrypt(data: &[u8], plr_dir: &Path) -> Result<Vec<u8>> {
    let key_bytes = get_or_create_key(plr_dir)?;
    let cipher = Aes256Gcm::new_from_slice(&key_bytes)
        .map_err(|e| anyhow::anyhow!("Failed to create cipher: {}", e))?;

    let mut nonce_bytes = vec![0u8; NONCE_SIZE];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, data)
        .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

    let mut result = nonce_bytes;
    result.extend_from_slice(&ciphertext);

    Ok(result)
}

pub fn decrypt(encrypted_data: &[u8], plr_dir: &Path) -> Result<Vec<u8>> {
    if encrypted_data.len() < NONCE_SIZE {
        anyhow::bail!("Invalid encrypted data: too short");
    }

    let key_bytes = get_or_create_key(plr_dir)?;
    let cipher = Aes256Gcm::new_from_slice(&key_bytes)
        .map_err(|e| anyhow::anyhow!("Failed to create cipher: {}", e))?;

    let (nonce_bytes, ciphertext) = encrypted_data.split_at(NONCE_SIZE);
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("Decryption failed: {}", e))?;

    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_encrypt_decrypt() {
        let temp = TempDir::new().unwrap();
        let data = b"secret credential data";

        let encrypted = encrypt(data, temp.path()).unwrap();
        assert_ne!(encrypted.as_slice(), data);

        let decrypted = decrypt(&encrypted, temp.path()).unwrap();
        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_key_persistence() {
        let temp = TempDir::new().unwrap();
        let data = b"test data";

        let encrypted1 = encrypt(data, temp.path()).unwrap();
        let decrypted = decrypt(&encrypted1, temp.path()).unwrap();
        assert_eq!(decrypted, data);
    }
}
