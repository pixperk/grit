use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::{
    cli::commands::utils::create_provider,
    state::{diff, load_staged, snapshot, JournalEntry, Operation},
};

pub async fn push(playlist: Option<&str>, grit_dir: &Path) -> Result<()> {
    let playlist_id = playlist.context("Playlist required (use --playlist)")?;

    let snapshot_path = snapshot::snapshot_path(grit_dir, playlist_id);
    if !snapshot_path.exists() {
        bail!("Playlist not initialized. Run 'grit init' first.");
    }

    let staged = load_staged(grit_dir, playlist_id)?;
    if !staged.changes.is_empty() {
        bail!(
            "You have {} uncommitted staged change(s). Please commit or reset before pushing.",
            staged.changes.len()
        );
    }

    let local_snapshot = snapshot::load(&snapshot_path)?;
    let provider = create_provider(local_snapshot.provider, grit_dir)?;

    println!("Verifying write permissions...");
    let can_modify = provider.can_modify_playlist(playlist_id).await?;
    if !can_modify {
        bail!(
            "You don't have write access to this playlist. Only the owner or collaborators can push changes."
        );
    }

    println!("Fetching remote playlist state...");
    let remote_snapshot = provider.fetch(playlist_id).await?;

    let patch = diff(&remote_snapshot, &local_snapshot);

    if patch.changes.is_empty() {
        println!("\nNo changes to push. Local and remote are in sync.");
        return Ok(());
    }

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

    println!(
        "\nPushing changes to remote: +{} -{} ~{}",
        added, removed, moved
    );

    // Apply patch to remote to match local snapshot
    provider.apply(playlist_id, &patch, &local_snapshot).await?;

    // Record in journal
    let hash = snapshot::compute_hash(&local_snapshot)?;
    let journal_path = JournalEntry::journal_path(grit_dir, playlist_id);
    let entry = JournalEntry::new(Operation::Push, hash, added, removed, moved);
    JournalEntry::append(&journal_path, &entry)?;

    println!("\nSuccessfully pushed to remote!");
    println!("  {} changes applied", patch.changes.len());

    Ok(())
}

pub async fn log(playlist: Option<&str>, grit_dir: &Path) -> Result<()> {
    let playlist_id = playlist.context("Playlist required (use --playlist")?;

    let snapshot_path = snapshot::snapshot_path(grit_dir, playlist_id);
    if !snapshot_path.exists() {
        bail!("Playlist not initialized. Run 'grit init' first.");
    }

    let journal_path = JournalEntry::journal_path(grit_dir, playlist_id);
    let entries = JournalEntry::read_all(&journal_path)?;

    if entries.is_empty() {
        println!("No history yet.");
        return Ok(());
    }

    println!("\nCommit History:\n");

    for entry in entries.iter().rev() {
        let hash_short = &entry.snapshot_hash[..8.min(entry.snapshot_hash.len())];
        let timestamp = entry.timestamp.format("%Y-%m-%d %H:%M:%S");

        let operation_str = match entry.operation {
            Operation::Init => "init",
            Operation::Pull => "pull",
            Operation::Push => "push",
            Operation::Apply => "apply",
            Operation::Commit => "commit",
        };

        let changes = format!("+{} -{} ~{}", entry.added, entry.removed, entry.moved);

        if let Some(msg) = &entry.message {
            println!(
                "[{}] {} | {} | {}",
                hash_short, timestamp, operation_str, msg
            );
        } else {
            println!("[{}] {} | {}", hash_short, timestamp, operation_str);
        }

        println!("  {}", changes);
        println!();
    }

    Ok(())
}

pub async fn pull(playlist: Option<&str>, grit_dir: &Path) -> Result<()> {
    let playlist_id = playlist.context("Playlist required (use --playlist)")?;

    let snapshot_path = snapshot::snapshot_path(grit_dir, playlist_id);
    if !snapshot_path.exists() {
        bail!("Playlist not initialized. Run 'grit init' first.");
    }

    let staged = load_staged(grit_dir, playlist_id)?;
    if !staged.changes.is_empty() {
        bail!(
            "You have {} uncommitted staged change(s). Please commit or reset before pulling.",
            staged.changes.len()
        );
    }

    let local_snapshot = snapshot::load(&snapshot_path)?;
    let provider = create_provider(local_snapshot.provider, grit_dir)?;

    println!("Fetching remote playlist state...");
    let remote_snapshot = provider.fetch(playlist_id).await?;

    let local_hash = snapshot::compute_hash(&local_snapshot)?;
    let remote_hash = snapshot::compute_hash(&remote_snapshot)?;

    if local_hash == remote_hash {
        println!("\nAlready up to date.");
        return Ok(());
    }

    let patch = diff(&local_snapshot, &remote_snapshot);

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

    println!(
        "\nPulling changes from remote: +{} -{} ~{}",
        added, removed, moved
    );

    // Update local snapshot to match remote
    snapshot::save(&remote_snapshot, &snapshot_path)?;

    // Record in journal
    let journal_path = JournalEntry::journal_path(grit_dir, playlist_id);
    let entry = JournalEntry::new(Operation::Pull, remote_hash, added, removed, moved);
    JournalEntry::append(&journal_path, &entry)?;

    println!("\nSuccessfully pulled from remote!");
    println!("  {} changes applied", patch.changes.len());

    Ok(())
}

pub async fn diff_cmd(
    playlist: Option<&str>,
    grit_dir: &Path,
    staged: bool,
    remote: bool,
) -> Result<()> {
    let playlist_id = playlist.context("Playlist required (use --playlist)")?;

    let snapshot_path = snapshot::snapshot_path(grit_dir, playlist_id);
    if !snapshot_path.exists() {
        bail!("Playlist not initialized. Run 'grit init' first.");
    }

    let local_snapshot = snapshot::load(&snapshot_path)?;

    // Default to showing staged changes if no flag is specified
    let show_staged = staged || !remote;

    if show_staged {
        println!("\n[Staged Changes]\n");

        let patch = load_staged(grit_dir, playlist_id)?;

        if patch.changes.is_empty() {
            println!("No staged changes.\n");
        } else {
            for change in &patch.changes {
                match change {
                    crate::provider::TrackChange::Added { track, index } => {
                        println!(
                            "+ [{}] {} - {}",
                            index,
                            track.name,
                            track.artists.join(", ")
                        );
                    }
                    crate::provider::TrackChange::Removed { track, index } => {
                        println!(
                            "- [{}] {} - {}",
                            index,
                            track.name,
                            track.artists.join(", ")
                        );
                    }
                    crate::provider::TrackChange::Moved { track, from, to } => {
                        println!(
                            "~ {} - {} (from {} to {})",
                            track.name,
                            track.artists.join(", "),
                            from,
                            to
                        );
                    }
                };
            }
            println!();
        }
    }

    if remote {
        println!("\n[Local vs Remote]\n");

        let provider = create_provider(local_snapshot.provider, grit_dir)?;

        match provider.fetch(playlist_id).await {
            std::result::Result::Ok(remote_snapshot) => {
                use crate::state::diff as compute_diff;
                let patch = compute_diff(&remote_snapshot, &local_snapshot);

                if patch.changes.is_empty() {
                    println!("Local and remote are in sync.\n");
                } else {
                    for change in &patch.changes {
                        match change {
                            crate::provider::TrackChange::Added { track, index } => {
                                println!(
                                    "+ [{}] {} - {}",
                                    index,
                                    track.name,
                                    track.artists.join(", ")
                                );
                            }
                            crate::provider::TrackChange::Removed { track, index } => {
                                println!(
                                    "- [{}] {} - {}",
                                    index,
                                    track.name,
                                    track.artists.join(", ")
                                );
                            }
                            crate::provider::TrackChange::Moved { track, from, to } => {
                                println!(
                                    "~ {} - {} (from {} to {})",
                                    track.name,
                                    track.artists.join(", "),
                                    from,
                                    to
                                );
                            }
                        }
                    }
                    println!();
                }
            }
            Err(e) => {
                println!("Could not fetch remote: {}\n", e);
            }
        }
    }

    Ok(())
}

pub async fn revert(hash: Option<&str>, playlist: Option<&str>, grit_dir: &Path) -> Result<()> {
    let playlist_id = playlist.context("Playlist required (use --playlist)")?;

    let snapshot_path = snapshot::snapshot_path(grit_dir, playlist_id);
    if !snapshot_path.exists() {
        bail!("Playlist not initialized. Run 'grit init' first.");
    }

    // Check for uncommitted staged changes
    let staged = load_staged(grit_dir, playlist_id)?;
    if !staged.changes.is_empty() {
        bail!(
            "You have {} uncommitted staged change(s). Commit or reset before reverting.",
            staged.changes.len()
        );
    }

    // Determine which hash to revert to
    let target_hash = if let Some(h) = hash {
        h.to_string()
    } else {
        // No hash provided - revert to previous commit
        let journal_path = JournalEntry::journal_path(grit_dir, playlist_id);
        let entries = JournalEntry::read_all(&journal_path)?;

        if entries.len() < 2 {
            bail!("Not enough commits to revert. Need at least 2 commits in history.");
        }

        // Get the second-to-last entry (the one before HEAD)
        entries[entries.len() - 2].snapshot_hash.clone()
    };

    // Load the target snapshot by hash
    let target_snapshot = snapshot::load_by_hash(&target_hash, grit_dir, playlist_id)
        .with_context(|| format!("Failed to load snapshot with hash '{}'", target_hash))?;

    let full_hash = snapshot::compute_hash(&target_snapshot)?;

    // Save as current snapshot
    snapshot::save(&target_snapshot, &snapshot_path)?;

    // Record in journal
    let journal_path = JournalEntry::journal_path(grit_dir, playlist_id);
    let entry = JournalEntry::new_with_message(
        Operation::Commit,
        full_hash.clone(),
        0,
        0,
        0,
        format!("Revert to {}", target_hash),
    );
    JournalEntry::append(&journal_path, &entry)?;

    println!("\nReverted to commit [{}]", full_hash);
    println!("Playlist: {}", target_snapshot.name);
    println!("Tracks: {}", target_snapshot.tracks.len());
    println!("\nUse 'grit push' to sync with remote if desired.");

    Ok(())
}

pub async fn apply(file_path: &str, playlist: Option<&str>, grit_dir: &Path) -> Result<()> {
    // Load the snapshot from YAML file
    let file_content = std::fs::read_to_string(file_path)
        .with_context(|| format!("Failed to read file: {}", file_path))?;

    let snapshot: crate::provider::PlaylistSnapshot = serde_yaml::from_str(&file_content)
        .with_context(|| "Failed to parse YAML file as PlaylistSnapshot")?;

    let playlist_id = playlist.unwrap_or(&snapshot.id);

    let snapshot_path = snapshot::snapshot_path(grit_dir, playlist_id);
    if !snapshot_path.exists() {
        bail!(
            "Playlist {} not initialized. Run 'grit init' first.",
            playlist_id
        );
    }

    // Load current snapshot to check provider compatibility
    let current_snapshot = snapshot::load(&snapshot_path)?;
    if current_snapshot.provider != snapshot.provider {
        bail!(
            "Provider mismatch: playlist is {:?} but file contains {:?} snapshot",
            current_snapshot.provider,
            snapshot.provider
        );
    }

    // Check for uncommitted staged changes
    let staged = load_staged(grit_dir, playlist_id)?;
    if !staged.changes.is_empty() {
        bail!(
            "You have {} uncommitted staged change(s). Commit or reset before applying.",
            staged.changes.len()
        );
    }

    // Compute hash and save snapshot
    let hash = snapshot::compute_hash(&snapshot)?;
    snapshot::save(&snapshot, &snapshot_path)?;
    snapshot::save_by_hash(&snapshot, &hash, grit_dir, playlist_id)?;

    // Record in journal
    let journal_path = JournalEntry::journal_path(grit_dir, playlist_id);
    let entry = JournalEntry::new_with_message(
        Operation::Apply,
        hash.clone(),
        0,
        0,
        0,
        format!("Applied from {}", file_path),
    );
    JournalEntry::append(&journal_path, &entry)?;

    println!("\nApplied playlist state from file!");
    println!("  Playlist: {}", snapshot.name);
    println!("  Tracks: {}", snapshot.tracks.len());
    println!("  Hash: [{}]", hash);
    println!("\nUse 'grit push' to sync with remote if desired.");

    Ok(())
}
