use std::fs;
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

pub async fn playlists(query: Option<&str>, plr_dir: &Path) -> Result<()> {
    let playlists_dir = plr_dir.join("playlists");

    if !playlists_dir.exists() {
        println!("No playlists tracked yet. Use 'plr init <playlist-id>' to start tracking.");
        return Ok(());
    }

    let entries = fs::read_dir(&playlists_dir)
        .with_context(|| format!("Failed to read playlists directory: {:?}", playlists_dir))?;

    let mut playlists_info = Vec::new();

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            let playlist_id = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");

            let snapshot_path = snapshot::snapshot_path(plr_dir, playlist_id);
            if snapshot_path.exists() {
                match snapshot::load(&snapshot_path) {
                    Ok(snapshot) => {
                        playlists_info.push((playlist_id.to_string(), snapshot));
                    }
                    Err(e) => {
                        eprintln!("Warning: Failed to load playlist {}: {}", playlist_id, e);
                    }
                }
            }
        }
    }

    if playlists_info.is_empty() {
        println!("No playlists tracked yet. Use 'plr init <playlist-id>' to start tracking.");
        return Ok(());
    }

    // Filter by query if provided
    let filtered: Vec<_> = if let Some(q) = query {
        let q_lower = q.to_lowercase();
        playlists_info
            .into_iter()
            .filter(|(_, snapshot)| {
                snapshot.name.to_lowercase().contains(&q_lower)
                    || snapshot
                        .description
                        .as_ref()
                        .map(|d| d.to_lowercase().contains(&q_lower))
                        .unwrap_or(false)
            })
            .collect()
    } else {
        playlists_info
    };

    if filtered.is_empty() {
        println!("No playlists found matching '{}'", query.unwrap_or(""));
        return Ok(());
    }

    if let Some(q) = query {
        println!(
            "\nFound {} playlist(s) matching '{}':\n",
            filtered.len(),
            q
        );
    } else {
        println!("\nLocally tracked playlists ({}):\n", filtered.len());
    }

    for (id, snapshot) in filtered {
        println!("• {}", snapshot.name);
        println!("  ID: {}", id);
        println!("  Provider: {:?}", snapshot.provider);
        println!("  Tracks: {}", snapshot.tracks.len());
        if let Some(desc) = &snapshot.description {
            let desc_short = if desc.len() > 80 {
                format!("{}...", &desc[..77])
            } else {
                desc.clone()
            };
            println!("  Description: {}", desc_short);
        }
        println!();
    }

    Ok(())
}
