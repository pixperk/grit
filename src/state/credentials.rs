use crate::provider::{OAuthToken, ProviderKind};
use crate::utils::crypto;
use anyhow::{Context, Result};
use base64::Engine;
use std::fs;
use std::path::Path;

pub fn save(grit_dir: &Path, provider: ProviderKind, token: &OAuthToken) -> Result<()> {
    let path = credentials_path(grit_dir, provider);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create credentials dir {:?}", parent))?;
    }

    let json = serde_json::to_string(token).context("Failed to serialize token")?;

    let encrypted =
        crypto::encrypt(json.as_bytes(), grit_dir).context("Failed to encrypt credentials")?;

    let encoded = base64::engine::general_purpose::STANDARD.encode(&encrypted);

    fs::write(&path, encoded)
        .with_context(|| format!("Failed to write credentials to {:?}", path))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

pub fn load(grit_dir: &Path, provider: ProviderKind) -> Result<Option<OAuthToken>> {
    let path = credentials_path(grit_dir, provider);

    if !path.exists() {
        return Ok(None);
    }

    let encoded = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read credentials from {:?}", path))?;

    let encrypted = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .context("Failed to decode credentials")?;

    let decrypted =
        crypto::decrypt(&encrypted, grit_dir).context("Failed to decrypt credentials")?;

    let json = String::from_utf8(decrypted).context("Invalid UTF-8 in decrypted credentials")?;

    let token = serde_json::from_str(&json).context("Failed to parse credentials")?;

    Ok(Some(token))
}

#[allow(dead_code)]
pub fn is_expired(token: &OAuthToken) -> bool {
    match token.expires_at {
        Some(expires_at) => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            now >= expires_at.saturating_sub(300)
        }
        None => false,
    }
}

/// Delete credentials for a provider
pub fn delete(grit_dir: &Path, provider: ProviderKind) -> Result<()> {
    let path = credentials_path(grit_dir, provider);

    if path.exists() {
        fs::remove_file(&path)
            .with_context(|| format!("Failed to delete credentials {:?}", path))?;
    }

    Ok(())
}

fn credentials_path(grit_dir: &Path, provider: ProviderKind) -> std::path::PathBuf {
    let filename = match provider {
        ProviderKind::Spotify => "spotify.json",
        ProviderKind::Youtube => "youtube.json",
    };
    grit_dir.join("credentials").join(filename)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_token() -> OAuthToken {
        OAuthToken {
            access_token: "test_access_token".to_string(),
            refresh_token: Some("test_refresh_token".to_string()),
            expires_at: Some(9999999999),
            token_type: "Bearer".to_string(),
            scope: Some("playlist-read-private".to_string()),
        }
    }

    #[test]
    fn test_save_and_load() {
        let temp = TempDir::new().unwrap();
        let token = sample_token();

        save(temp.path(), ProviderKind::Spotify, &token).unwrap();
        let loaded = load(temp.path(), ProviderKind::Spotify).unwrap();

        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.access_token, token.access_token);
        assert_eq!(loaded.refresh_token, token.refresh_token);
    }

    #[test]
    fn test_load_nonexistent() {
        let temp = TempDir::new().unwrap();
        let loaded = load(temp.path(), ProviderKind::Spotify).unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn test_is_expired_future() {
        let token = OAuthToken {
            access_token: "test".to_string(),
            refresh_token: None,
            expires_at: Some(9999999999), // Far future
            token_type: "Bearer".to_string(),
            scope: None,
        };
        assert!(!is_expired(&token));
    }

    #[test]
    fn test_is_expired_past() {
        let token = OAuthToken {
            access_token: "test".to_string(),
            refresh_token: None,
            expires_at: Some(1000), // Long past
            token_type: "Bearer".to_string(),
            scope: None,
        };
        assert!(is_expired(&token));
    }

    #[test]
    fn test_is_expired_none() {
        let token = OAuthToken {
            access_token: "test".to_string(),
            refresh_token: None,
            expires_at: None, // No expiry
            token_type: "Bearer".to_string(),
            scope: None,
        };
        assert!(!is_expired(&token));
    }

    #[test]
    fn test_delete() {
        let temp = TempDir::new().unwrap();
        let token = sample_token();

        save(temp.path(), ProviderKind::Spotify, &token).unwrap();
        assert!(load(temp.path(), ProviderKind::Spotify).unwrap().is_some());

        delete(temp.path(), ProviderKind::Spotify).unwrap();
        assert!(load(temp.path(), ProviderKind::Spotify).unwrap().is_none());
    }

    #[test]
    fn test_separate_providers() {
        let temp = TempDir::new().unwrap();

        let spotify_token = OAuthToken {
            access_token: "spotify_token".to_string(),
            refresh_token: None,
            expires_at: None,
            token_type: "Bearer".to_string(),
            scope: None,
        };

        let youtube_token = OAuthToken {
            access_token: "youtube_token".to_string(),
            refresh_token: None,
            expires_at: None,
            token_type: "Bearer".to_string(),
            scope: None,
        };

        save(temp.path(), ProviderKind::Spotify, &spotify_token).unwrap();
        save(temp.path(), ProviderKind::Youtube, &youtube_token).unwrap();

        let loaded_spotify = load(temp.path(), ProviderKind::Spotify).unwrap().unwrap();
        let loaded_youtube = load(temp.path(), ProviderKind::Youtube).unwrap().unwrap();

        assert_eq!(loaded_spotify.access_token, "spotify_token");
        assert_eq!(loaded_youtube.access_token, "youtube_token");
    }
}
