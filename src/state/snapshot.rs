use std::{fs, path::Path};

use anyhow::{Context, Ok};
use sha2::{Digest, Sha256};

use crate::provider::PlaylistSnapshot;

pub fn compute_hash(snapshot: &PlaylistSnapshot) -> anyhow::Result<String> {
    let yaml = serde_yaml::to_string(snapshot)
        .with_context(|| "Failed to serialize snapshot for hashing")?;

    let mut hasher = Sha256::new();
    hasher.update(yaml.as_bytes());
    let result = hasher.finalize();

    let hex = result
        .iter()
        .take(6) //6 bytes = 12 hex chars
        .map(|b| format!("{:02x}", b))
        .collect();

    Ok(hex)
}

pub fn save(snapshot: &PlaylistSnapshot, path: &Path) -> anyhow::Result<()> {
    let yaml = serde_yaml::to_string(snapshot).with_context(|| "Failed to serialize snapshot")?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {:?}", parent))?;
    }

    fs::write(path, yaml).with_context(|| format!("Failed to write snapshot to {:?}", path))
}

pub fn load(path: &Path) -> anyhow::Result<PlaylistSnapshot> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read snapshot from {:?}", path))?;

    serde_yaml::from_str(&content).with_context(|| "Failed to parse snapshot YAML")
}

pub fn snapshot_path(grit_dir: &Path, playlist_id: &str) -> std::path::PathBuf {
    grit_dir
        .join("playlists")
        .join(playlist_id)
        .join("playlist.yaml")
}

/// Get the snapshots directory path for a playlist
pub fn snapshots_dir(grit_dir: &Path, playlist_id: &str) -> std::path::PathBuf {
    grit_dir
        .join("playlists")
        .join(playlist_id)
        .join("snapshots")
}

/// Save a snapshot with its hash for historical reference
pub fn save_by_hash(
    snapshot: &PlaylistSnapshot,
    hash: &str,
    grit_dir: &Path,
    playlist_id: &str,
) -> anyhow::Result<()> {
    let snapshots_dir = snapshots_dir(grit_dir, playlist_id);
    fs::create_dir_all(&snapshots_dir)
        .with_context(|| format!("Failed to create snapshots directory {:?}", snapshots_dir))?;

    let path = snapshots_dir.join(format!("{}.yaml", hash));
    save(snapshot, &path)
}

/// Load a snapshot by its hash
pub fn load_by_hash(
    hash: &str,
    grit_dir: &Path,
    playlist_id: &str,
) -> anyhow::Result<PlaylistSnapshot> {
    let snapshots_dir = snapshots_dir(grit_dir, playlist_id);

    // Support partial hash matching
    if let std::result::Result::Ok(entries) = fs::read_dir(&snapshots_dir) {
        for entry in entries.flatten() {
            if let Some(filename) = entry.file_name().to_str() {
                if filename.starts_with(hash) && filename.ends_with(".yaml") {
                    return load(&entry.path());
                }
            }
        }
    }

    anyhow::bail!("No snapshot found with hash '{}'", hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ProviderKind, Track};
    use tempfile::TempDir;

    fn sample_snapshot() -> PlaylistSnapshot {
        PlaylistSnapshot {
            id: "playlist123".to_string(),
            name: "Test Playlist".to_string(),
            description: Some("A test".to_string()),
            tracks: vec![Track {
                id: "track1".to_string(),
                name: "Song One".to_string(),
                artists: vec!["Artist A".to_string()],
                duration_ms: 180000,
                provider: ProviderKind::Spotify,
                metadata: None,
            }],
            provider: ProviderKind::Spotify,
            snapshot_hash: String::new(),
            metadata: None,
        }
    }

    #[test]
    fn test_compute_hash_deterministic() {
        let snapshot = sample_snapshot();
        let hash1 = compute_hash(&snapshot).unwrap();
        let hash2 = compute_hash(&snapshot).unwrap();
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 12); // Short hash
    }

    #[test]
    fn test_save_and_load() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("playlist.yaml");

        let snapshot = sample_snapshot();
        save(&snapshot, &path).unwrap();
        let loaded = load(&path).unwrap();

        assert_eq!(loaded.id, snapshot.id);
        assert_eq!(loaded.name, snapshot.name);
        assert_eq!(loaded.tracks.len(), 1);
    }
}
