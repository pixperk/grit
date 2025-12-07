use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::state::snapshot;

pub async fn list(playlist: Option<&str>, plr_dir: &Path) -> Result<()> {
    let playlist_id = playlist.context("Playlist required (use --playlist)")?;

    let snapshot_path = snapshot::snapshot_path(plr_dir, playlist_id);
    if !snapshot_path.exists() {
        bail!("Playlist not initialized. Run 'plr init' first.");
    }

    let snapshot = snapshot::load(&snapshot_path)?;

    println!("\nPlaylist: {}", snapshot.name);
    if let Some(desc) = &snapshot.description {
        println!("Description: {}", desc);
    }
    println!("Tracks: {}\n", snapshot.tracks.len());

    for (i, track) in snapshot.tracks.iter().enumerate() {
        let duration_sec = track.duration_ms / 1000;
        let min = duration_sec / 60;
        let sec = duration_sec % 60;
        let artists = track.artists.join(", ");

        println!(
            "{}. [{:02}:{:02}] {} - {}",
            i, min, sec, track.name, artists
        );
    }

    println!("\nTotal duration: {} tracks", snapshot.tracks.len());

    Ok(())
}

pub async fn find(query: &str, playlist: Option<&str>, plr_dir: &Path) -> Result<()> {
    let playlist_id = playlist.context("Playlist required (use --playlist)")?;

    let snapshot_path = snapshot::snapshot_path(plr_dir, playlist_id);
    if !snapshot_path.exists() {
        bail!("Playlist not initialized. Run 'plr init' first.");
    }

    let snapshot = snapshot::load(&snapshot_path)?;
    let query_lower = query.to_lowercase();

    let matches: Vec<(usize, &crate::provider::Track)> = snapshot
        .tracks
        .iter()
        .enumerate()
        .filter(|(_, track)| {
            track.name.to_lowercase().contains(&query_lower)
                || track
                    .artists
                    .iter()
                    .any(|a| a.to_lowercase().contains(&query_lower))
        })
        .collect();

    if matches.is_empty() {
        println!("No tracks found matching '{}'", query);
        return Ok(());
    }

    println!(
        "\nFound {} track(s) matching '{}' in {}:\n",
        matches.len(),
        query,
        snapshot.name
    );

    for (i, track) in matches {
        let duration_sec = track.duration_ms / 1000;
        let min = duration_sec / 60;
        let sec = duration_sec % 60;
        let artists = track.artists.join(", ");

        println!(
            "{}. [{:02}:{:02}] {} - {}",
            i, min, sec, track.name, artists
        );
        println!("   ID: {}", track.id);
        println!();
    }

    Ok(())
}
