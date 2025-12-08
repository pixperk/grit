use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::{
    cli::commands::utils::create_provider,
    state::{diff, load_staged, snapshot, JournalEntry, Operation},
};

pub async fn push(playlist: Option<&str>, plr_dir: &Path) -> Result<()> {
    let playlist_id = playlist.context("Playlist required (use --playlist)")?;

    let snapshot_path = snapshot::snapshot_path(plr_dir, playlist_id);
    if !snapshot_path.exists() {
        bail!("Playlist not initialized. Run 'plr init' first.");
    }

    let staged = load_staged(plr_dir, playlist_id)?;
    if !staged.changes.is_empty() {
        bail!(
            "You have {} uncommitted staged change(s). Please commit or reset before pushing.",
            staged.changes.len()
        );
    }

    let local_snapshot = snapshot::load(&snapshot_path)?;
    let provider = create_provider(local_snapshot.provider, plr_dir)?;

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
    let journal_path = JournalEntry::journal_path(plr_dir, playlist_id);
    let entry = JournalEntry::new(Operation::Push, hash, added, removed, moved);
    JournalEntry::append(&journal_path, &entry)?;

    println!("\nSuccessfully pushed to remote!");
    println!("  {} changes applied", patch.changes.len());

    Ok(())
}

pub async fn log(playlist: Option<&str>, plr_dir: &Path) -> Result<()> {
    let playlist_id = playlist.context("Playlist required (use --playlist")?;

    let snapshot_path = snapshot::snapshot_path(plr_dir, playlist_id);
    if !snapshot_path.exists() {
        bail!("Playlist not initialized. Run 'plr init' first.");
    }

    let journal_path = JournalEntry::journal_path(plr_dir, playlist_id);
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

pub async fn pull(playlist: Option<&str>, plr_dir: &Path) -> Result<()> {
    let playlist_id = playlist.context("Playlist required (use --playlist)")?;

    let snapshot_path = snapshot::snapshot_path(plr_dir, playlist_id);
    if !snapshot_path.exists() {
        bail!("Playlist not initialized. Run 'plr init' first.");
    }

    let staged = load_staged(plr_dir, playlist_id)?;
    if !staged.changes.is_empty() {
        bail!(
            "You have {} uncommitted staged change(s). Please commit or reset before pulling.",
            staged.changes.len()
        );
    }

    let local_snapshot = snapshot::load(&snapshot_path)?;
    let provider = create_provider(local_snapshot.provider, plr_dir)?;

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
    let journal_path = JournalEntry::journal_path(plr_dir, playlist_id);
    let entry = JournalEntry::new(Operation::Pull, remote_hash, added, removed, moved);
    JournalEntry::append(&journal_path, &entry)?;

    println!("\nSuccessfully pulled from remote!");
    println!("  {} changes applied", patch.changes.len());

    Ok(())
}

pub async fn diff_cmd(
    playlist: Option<&str>,
    plr_dir: &Path,
    staged: bool,
    remote: bool,
) -> Result<()> {
    let playlist_id = playlist.context("Playlist required (use --playlist)")?;

    let snapshot_path = snapshot::snapshot_path(plr_dir, playlist_id);
    if !snapshot_path.exists() {
        bail!("Playlist not initialized. Run 'plr init' first.");
    }

    let local_snapshot = snapshot::load(&snapshot_path)?;

    // Default to showing staged changes if no flag is specified
    let show_staged = staged || !remote;

    if show_staged {
        println!("\n[Staged Changes]\n");

        let patch = load_staged(plr_dir, playlist_id)?;

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

        let provider = create_provider(local_snapshot.provider, plr_dir)?;

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
