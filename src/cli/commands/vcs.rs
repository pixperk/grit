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

    // Apply patch to remote
    provider.apply(playlist_id, &patch).await?;

    // Record in journal
    let hash = snapshot::compute_hash(&local_snapshot)?;
    let journal_path = JournalEntry::journal_path(plr_dir, playlist_id);
    let entry = JournalEntry::new(Operation::Push, hash, added, removed, moved);
    JournalEntry::append(&journal_path, &entry)?;

    println!("\nSuccessfully pushed to remote!");
    println!("  {} changes applied", patch.changes.len());

    Ok(())
}
