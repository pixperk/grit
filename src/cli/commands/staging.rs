use anyhow::{bail, Context, Ok, Result};
use std::path::Path;

use crate::{
    provider::{Provider, ProviderKind, SpotifyProvider, TrackChange, YoutubeProvider},
    state::{credentials, snapshot, stage_change},
};

fn create_provider(provider_kind: ProviderKind, plr_dir: &Path) -> Result<Box<dyn Provider>> {
    let token = credentials::load(plr_dir, provider_kind)?
        .context("No credentials found. Please run 'plr auth <provider>' first.")?;

    let provider: Box<dyn Provider> = match provider_kind {
        ProviderKind::Spotify => {
            let client_id =
                std::env::var("SPOTIFY_CLIENT_ID").context("SPOTIFY_CLIENT_ID not set")?;
            let client_secret =
                std::env::var("SPOTIFY_CLIENT_SECRET").context("SPOTIFY_CLIENT_SECRET not set")?;

            Box::new(SpotifyProvider::new(client_id, client_secret).with_token(&token, plr_dir))
        }
        ProviderKind::Youtube => {
            let client_id =
                std::env::var("YOUTUBE_CLIENT_ID").context("YOUTUBE_CLIENT_ID not set")?;
            let client_secret =
                std::env::var("YOUTUBE_CLIENT_SECRET").context("YOUTUBE_CLIENT_SECRET not set")?;

            Box::new(YoutubeProvider::new(client_id, client_secret).with_token(&token, plr_dir))
        }
    };
    Ok(provider)
}

pub async fn search(query: &str, provider: Option<ProviderKind>, plr_dir: &Path) -> Result<()> {
    let provider_kind = provider.context("Provider required for search (use --provider)")?;
    let provider = create_provider(provider_kind, plr_dir)?;

    let tracks = provider.search_by_query(query).await?;

    if tracks.is_empty() {
        println!("No tracks found for '{}'", query);
        return Ok(());
    }

    println!("\nSearch results for '{}':\n", query);
    for (i, track) in tracks.iter().enumerate() {
        let artists = track.artists.join(", ");
        let duration_sec = track.duration_ms / 1000;
        let min = duration_sec / 60;
        let sec = duration_sec % 60;

        println!("{}. {} - {}", i + 1, track.name, artists);
        println!("   ID: {} | Duration: {}:{:02}", track.id, min, sec);
        println!();
    }

    println!("Use 'plr add <track-id>' to stage a track for addition");

    Ok(())
}

pub async fn add(track_id: &str, playlist: Option<&str>, plr_dir: &Path) -> Result<()> {
    let playlist_id = playlist.context("Playlist required (use --playlist)")?;

    let snapshot_path = snapshot::snapshot_path(plr_dir, playlist_id);
    if !snapshot_path.exists() {
        bail!("Playlist not initialized. Run 'plr init' first.");
    }

    let snapshot = snapshot::load(&snapshot_path)?;
    let provider = create_provider(snapshot.provider, plr_dir)?;

    let track = provider.fetch_track(track_id).await?;

    let index = snapshot.tracks.len();

    let change = TrackChange::Added {
        track: track.clone(),
        index,
    };

    stage_change(plr_dir, playlist_id, change)?;

    println!(
        "Staged for addition: {} - {}",
        track.name,
        track.artists.join(", ")
    );
    println!("  Position: {}", index);
    println!("\nUse 'plr status' to see all staged changes");
    println!("Use 'plr commit -m \"message\"' to commit");

    Ok(())
}

pub async fn remove(track_id: &str, playlist: Option<&str>, plr_dir: &Path) -> Result<()> {
    let playlist_id = playlist.context("Playlist required (use --playlist)")?;

    let snapshot_path = snapshot::snapshot_path(plr_dir, playlist_id);
    if !snapshot_path.exists() {
        bail!("Playlist not initialized. Run 'plr init' first.");
    }

    let snapshot = snapshot::load(&snapshot_path)?;

    let (index, track) = snapshot
        .tracks
        .iter()
        .enumerate()
        .find(|(_, t)| t.id == track_id)
        .context("Track not found in playlist")?;

    let change = TrackChange::Removed {
        track: track.clone(),
        index,
    };

    stage_change(plr_dir, playlist_id, change)?;

    println!(
        "Staged for removal: {} - {}",
        track.name,
        track.artists.join(", ")
    );
    println!("  Position: {}", index);
    println!("\nUse 'plr status' to see all staged changes");
    println!("Use 'plr commit -m \"message\"' to commit");

    Ok(())
}

pub async fn move_track(
    track_id: &str,
    new_index: usize,
    playlist: Option<&str>,
    plr_dir: &Path,
) -> Result<()> {
    let playlist_id = playlist.context("Playlist required (use --playlist)")?;

    let snapshot_path = snapshot::snapshot_path(plr_dir, playlist_id);
    if !snapshot_path.exists() {
        bail!("Playlist not initialized. Run 'plr init' first.");
    }

    let snapshot = snapshot::load(&snapshot_path)?;

    let (from_index, track) = snapshot
        .tracks
        .iter()
        .enumerate()
        .find(|(_, t)| t.id == track_id)
        .context("Track not found in playlist")?;

    if from_index == new_index {
        bail!("Track is already at position {}", new_index);
    }

    if new_index >= snapshot.tracks.len() {
        bail!(
            "Invalid index {}. Playlist has {} tracks.",
            new_index,
            snapshot.tracks.len()
        );
    }

    let change = TrackChange::Moved {
        track: track.clone(),
        from: from_index,
        to: new_index,
    };

    stage_change(plr_dir, playlist_id, change)?;

    println!("Staged move: {} - {}", track.name, track.artists.join(", "));
    println!("  From: {} → To: {}", from_index, new_index);
    println!("\nUse 'plr status' to see all staged changes");
    println!("Use 'plr commit -m \"message\"' to commit");

    Ok(())
}
