use crate::provider::{DiffPatch, TrackChange};
use anyhow::{Context, Ok, Result};
use std::fs;
use std::path::Path;

pub fn load_staged(grit_dir: &Path, playlist_id: &str) -> Result<DiffPatch> {
    let staged_path = grit_dir
        .join("playlists")
        .join(playlist_id)
        .join("staged.json");

    if !staged_path.exists() {
        return Ok(DiffPatch { changes: vec![] });
    }

    let contents = fs::read_to_string(&staged_path).context("Failed to read staged.json")?;

    let patch: DiffPatch =
        serde_json::from_str(&contents).context("Failed to parse staged.json")?;

    Ok(patch)
}

pub fn save_staged(grit_dir: &Path, playlist_id: &str, patch: &DiffPatch) -> Result<()> {
    let staged_path = grit_dir
        .join("playlists")
        .join(playlist_id)
        .join("staged.json");

    let json = serde_json::to_string_pretty(patch).context("Failed to serialize staged changes")?;

    fs::write(&staged_path, json).context("Failed to write staged.json")?;

    Ok(())
}

pub fn clear_staged(grit_dir: &Path, playlist_id: &str) -> Result<()> {
    save_staged(grit_dir, playlist_id, &DiffPatch { changes: vec![] })
}

pub fn stage_change(grit_dir: &Path, playlist_id: &str, change: TrackChange) -> Result<()> {
    let mut patch = load_staged(grit_dir, playlist_id)?;
    patch.changes.push(change);
    save_staged(grit_dir, playlist_id, &patch)
}

#[allow(dead_code)]
pub fn has_staged_changes(grit_dir: &Path, playlist_id: &str) -> Result<bool> {
    let patch = load_staged(grit_dir, playlist_id)?;
    Ok(!patch.changes.is_empty())
}
