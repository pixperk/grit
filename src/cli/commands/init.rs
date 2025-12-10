use crate::provider::{Provider, ProviderKind, SpotifyProvider, YoutubeProvider};
use crate::state::{clear_staged, credentials, snapshot, JournalEntry, Operation};
use anyhow::{Context, Result};
use std::path::Path;

/// Extract playlist ID from URL or return as-is if already an ID
fn extract_playlist_id(input: &str) -> String {
    // Handle Spotify URLs: https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M
    if input.contains("spotify.com/playlist/") {
        return input
            .split("playlist/")
            .nth(1)
            .and_then(|s| s.split('?').next())
            .unwrap_or(input)
            .to_string();
    }

    // Handle YouTube URLs: https://www.youtube.com/playlist?list=PLrAXtmErZgOeiKm4sgNOknGvNjby9efdf
    if input.contains("youtube.com") || input.contains("youtu.be") {
        if let Some(list_start) = input.find("list=") {
            let id_part = &input[list_start + 5..];
            return id_part.split('&').next().unwrap_or(input).to_string();
        }
    }

    // Already an ID
    input.to_string()
}

/// Detect provider from playlist URL
pub fn detect_provider(input: &str) -> Option<ProviderKind> {
    if input.contains("spotify.com") {
        Some(ProviderKind::Spotify)
    } else if input.contains("youtube.com") || input.contains("youtu.be") {
        Some(ProviderKind::Youtube)
    } else {
        None
    }
}

pub async fn run(provider: ProviderKind, playlist: &str, grit_dir: &Path) -> Result<()> {
    let playlist_id = extract_playlist_id(playlist);
    //if already initialized, return error
    let snapshot_path = snapshot::snapshot_path(grit_dir, &playlist_id);
    if snapshot_path.exists() {
        anyhow::bail!(
            "Playlist {} already initialized. Use 'grit pull' to update.",
            playlist_id
        );
    }

    let token = credentials::load(grit_dir, provider)?
        .context("No credentials found. Please run 'grit auth <provider>' first.")?;

    let provider_impl: Box<dyn Provider> = match provider {
        ProviderKind::Spotify => {
            let client_id =
                std::env::var("SPOTIFY_CLIENT_ID").context("SPOTIFY_CLIENT_ID not set")?;
            let client_secret =
                std::env::var("SPOTIFY_CLIENT_SECRET").context("SPOTIFY_CLIENT_SECRET not set")?;

            Box::new(SpotifyProvider::new(client_id, client_secret).with_token(&token, grit_dir))
        }
        ProviderKind::Youtube => {
            let client_id =
                std::env::var("YOUTUBE_CLIENT_ID").context("YOUTUBE_CLIENT_ID not set")?;
            let client_secret =
                std::env::var("YOUTUBE_CLIENT_SECRET").context("YOUTUBE_CLIENT_SECRET not set")?;

            Box::new(YoutubeProvider::new(client_id, client_secret).with_token(&token, grit_dir))
        }
    };

    println!("Fetching playlist {}...", playlist_id);

    let playlist = provider_impl.fetch(&playlist_id).await?;

    println!("  Name: {}", playlist.name);
    println!("  Tracks: {}", playlist.tracks.len());

    snapshot::save(&playlist, &snapshot_path)?;
    let hash = snapshot::compute_hash(&playlist)?;

    // Save snapshot by hash for revert functionality
    snapshot::save_by_hash(&playlist, &hash, grit_dir, &playlist_id)?;

    let journal_path = JournalEntry::journal_path(grit_dir, &playlist_id);
    let entry = JournalEntry::new(Operation::Init, hash, playlist.tracks.len(), 0, 0);
    JournalEntry::append(&journal_path, &entry)?;

    // Clear any staged changes
    clear_staged(grit_dir, &playlist_id)?;

    println!("\nPlaylist initialized!");
    println!("  Snapshot: {:?}", snapshot_path);
    println!("  Journal: {:?}", journal_path);

    Ok(())
}
