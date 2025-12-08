use anyhow::{bail, Context, Ok, Result};
use std::io::{self, Write};
use std::path::Path;

use crate::{
    cli::commands::utils::create_provider,
    provider::{ProviderKind, TrackChange},
    state::{
        apply_patch, clear_staged, load_staged, snapshot, stage_change, JournalEntry, Operation,
    },
};

pub async fn status(playlist: Option<&str>, plr_dir: &Path) -> Result<()> {
    let playlist_id = playlist.context("Playlist required (use --playlist)")?;

    let snapshot_path = snapshot::snapshot_path(plr_dir, playlist_id);
    if !snapshot_path.exists() {
        bail!("Playlist not initialized. Run 'plr init' first.");
    }

    let local_snapshot = snapshot::load(&snapshot_path)?;
    let staged_patch = load_staged(plr_dir, playlist_id)?;

    // Display staged changes
    println!("\n[Staged Changes]");
    if staged_patch.changes.is_empty() {
        println!("  No staged changes");
    } else {
        let mut added = 0;
        let mut removed = 0;
        let mut moved = 0;

        for change in &staged_patch.changes {
            match change {
                crate::provider::TrackChange::Added { track, index } => {
                    added += 1;
                    println!(
                        "  + [{}] {} - {}",
                        index,
                        track.name,
                        track.artists.join(", ")
                    );
                }
                crate::provider::TrackChange::Removed { track, index } => {
                    removed += 1;
                    println!(
                        "  - [{}] {} - {}",
                        index,
                        track.name,
                        track.artists.join(", ")
                    );
                }
                crate::provider::TrackChange::Moved { track, from, to } => {
                    moved += 1;
                    println!(
                        "  ~ {} - {} (from {} to {})",
                        track.name,
                        track.artists.join(", "),
                        from,
                        to
                    );
                }
            }
        }

        println!("\n  Summary: +{} -{} ~{}", added, removed, moved);
        println!("\nUse 'plr commit -m \"message\"' to commit these changes");
        println!("Use 'plr reset' to discard staged changes");
    }

    // Compare local vs remote
    println!("\n[Local vs Remote]");
    let provider = create_provider(local_snapshot.provider, plr_dir)?;

    match provider.fetch(playlist_id).await {
        std::result::Result::Ok(remote_snapshot) => {
            use crate::state::diff;
            let local_vs_remote = diff(&remote_snapshot, &local_snapshot);

            if local_vs_remote.changes.is_empty() {
                println!("  Local and remote are in sync");
            } else {
                let mut added = 0;
                let mut removed = 0;
                let mut moved = 0;

                for change in &local_vs_remote.changes {
                    match change {
                        crate::provider::TrackChange::Added { .. } => added += 1,
                        crate::provider::TrackChange::Removed { .. } => removed += 1,
                        crate::provider::TrackChange::Moved { .. } => moved += 1,
                    }
                }

                println!(
                    "  Your local branch is ahead by {} change(s): +{} -{} ~{}",
                    local_vs_remote.changes.len(),
                    added,
                    removed,
                    moved
                );
                println!("\n  Use 'plr push' to sync with remote");
            }
        }
        Err(e) => {
            println!("  Could not fetch remote: {}", e);
            println!("  (Local changes can still be committed)");
        }
    }

    println!();

    Ok(())
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

    const PAGE_SIZE: usize = 5;
    let mut start = 0;

    loop {
        let end = (start + PAGE_SIZE).min(tracks.len());
        let page_tracks = &tracks[start..end];

        for (i, track) in page_tracks.iter().enumerate() {
            let artists = track.artists.join(", ");
            let duration_sec = track.duration_ms / 1000;
            let min = duration_sec / 60;
            let sec = duration_sec % 60;

            println!("{}. {} - {}", start + i + 1, track.name, artists);
            println!("   ID: {} | Duration: {}:{:02}", track.id, min, sec);
            println!();
        }

        start = end;

        if start >= tracks.len() {
            // All results shown
            break;
        }

        // Prompt for more
        print!("Show more? [Enter] or 'q' to quit: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if input.trim().eq_ignore_ascii_case("q") {
            break;
        }
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

    // Validate provider match
    if track.provider != snapshot.provider {
        bail!(
            "Cannot add {:?} track to {:?} playlist. Provider mismatch.",
            track.provider,
            snapshot.provider
        );
    }

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

pub async fn reset(playlist: Option<&str>, plr_dir: &Path) -> Result<()> {
    let playlist_id = playlist.context("Playlist required (use --playlist)")?;

    let snapshot_path = snapshot::snapshot_path(plr_dir, playlist_id);
    if !snapshot_path.exists() {
        bail!("Playlist not initialized. Run 'plr init' first.");
    }

    let patch = load_staged(plr_dir, playlist_id)?;
    if patch.changes.is_empty() {
        println!("No staged changes to reset.");
        return Ok(());
    }

    clear_staged(plr_dir, playlist_id)?;

    println!("Staged changes cleared.");
    println!("  {} operations discarded", patch.changes.len());

    Ok(())
}

pub async fn commit(message: &str, playlist: Option<&str>, plr_dir: &Path) -> Result<()> {
    let playlist_id = playlist.context("Playlist required (use --playlist)")?;

    let snapshot_path = snapshot::snapshot_path(plr_dir, playlist_id);
    if !snapshot_path.exists() {
        bail!("Playlist not initialized. Run 'plr init' first.");
    }

    let patch = load_staged(plr_dir, playlist_id)?;
    if patch.changes.is_empty() {
        println!("No staged changes to commit.");
        return Ok(());
    }

    let mut snapshot_copy = snapshot::load(&snapshot_path)?;

    let mut added = 0;
    let mut removed = 0;
    let mut moved = 0;

    for change in &patch.changes {
        match change {
            crate::provider::TrackChange::Added { .. } => added += 1,
            crate::provider::TrackChange::Removed { .. } => removed += 1,
            crate::provider::TrackChange::Moved { .. } => moved += 1,
        }
    }

    apply_patch(&mut snapshot_copy, &patch)?;

    let hash = snapshot::compute_hash(&snapshot_copy)?;

    // Save snapshot by hash for revert functionality
    snapshot::save_by_hash(&snapshot_copy, &hash, plr_dir, playlist_id)?;

    snapshot::save(&snapshot_copy, &snapshot_path)?;

    let journal_path = JournalEntry::journal_path(plr_dir, playlist_id);
    let entry = JournalEntry::new_with_message(
        Operation::Commit,
        hash.clone(),
        added,
        removed,
        moved,
        message.to_string(),
    );
    JournalEntry::append(&journal_path, &entry)?;

    clear_staged(plr_dir, playlist_id)?;

    println!("\n[{}] {}", hash, message);
    println!("  +{} -{} ~{} tracks", added, removed, moved);
    println!("\nChanges committed to local snapshot.");
    println!("Use 'plr push' to sync with remote.");

    Ok(())
}
