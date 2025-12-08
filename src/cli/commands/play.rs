use std::io::{self, Write};
use std::path::Path;

use anyhow::{bail, Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

use crate::playback::{MpvPlayer, Queue, SpotifyPlayer};
use crate::provider::ProviderKind;
use crate::state::{credentials, snapshot};

pub async fn run(playlist: Option<&str>, shuffle: bool, grit_dir: &Path) -> Result<()> {
    let playlist_id = playlist.context("Playlist required (use --playlist or -l)")?;

    let snapshot_path = snapshot::snapshot_path(grit_dir, playlist_id);
    if !snapshot_path.exists() {
        bail!("Playlist not tracked. Run 'grit init <playlist>' first.");
    }

    let snap = snapshot::load(&snapshot_path)?;
    if snap.tracks.is_empty() {
        bail!("Playlist is empty");
    }

    println!("Playing: {} ({} tracks)", snap.name, snap.tracks.len());

    // Use appropriate backend based on provider
    match snap.provider {
        ProviderKind::Spotify => play_spotify(&snap, shuffle, grit_dir).await,
        ProviderKind::Youtube => play_mpv(&snap, shuffle, grit_dir).await,
    }
}

/// Play using Spotify Connect (for Spotify playlists)
async fn play_spotify(
    snap: &crate::provider::PlaylistSnapshot,
    shuffle: bool,
    grit_dir: &Path,
) -> Result<()> {
    // Load Spotify credentials
    let token = credentials::load(grit_dir, ProviderKind::Spotify)?
        .context("No Spotify credentials. Run 'grit auth spotify' first.")?;

    let client_id = std::env::var("SPOTIFY_CLIENT_ID").context("SPOTIFY_CLIENT_ID not set")?;
    let client_secret =
        std::env::var("SPOTIFY_CLIENT_SECRET").context("SPOTIFY_CLIENT_SECRET not set")?;

    let mut player = SpotifyPlayer::new(token, client_id, client_secret, grit_dir);

    // Find a Spotify Connect device
    player.select_device().await?;

    // Build list of track URIs
    let uris: Vec<String> = snap
        .tracks
        .iter()
        .map(|t| format!("spotify:track:{}", t.id))
        .collect();

    // Set shuffle before playing
    if shuffle {
        player.set_shuffle(true).await?;
        println!("Shuffle: ON");
    }

    // Start playback with all tracks
    println!(
        "\n[1/{}] {} - {}",
        snap.tracks.len(),
        snap.tracks[0].name,
        snap.tracks[0].artists.join(", ")
    );
    player.play(uris, 0).await?;

    println!("\nControls: [space] pause  [n] next  [p] prev  [s] shuffle  [q] quit");
    println!("(Playback controlled via Spotify Connect)");

    let mut is_paused = false;
    let mut shuffle_on = shuffle;
    enable_raw_mode()?;

    loop {
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(KeyEvent { code, .. }) = event::read()? {
                match code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char(' ') => {
                        is_paused = !is_paused;
                        let result = if is_paused {
                            player.pause().await
                        } else {
                            player.resume().await
                        };
                        match result {
                            Ok(_) => {
                                if is_paused {
                                    print!("\r[Paused]                              ");
                                } else {
                                    print!("\r[Playing]                             ");
                                }
                            }
                            Err(e) => print!("\rError: {}                    ", e),
                        }
                        io::stdout().flush()?;
                    }
                    KeyCode::Char('n') => {
                        match player.next().await {
                            Ok(_) => {
                                // Small delay for Spotify to update
                                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                                if let Ok(Some((name, artists))) = player.get_currently_playing().await {
                                    print!("\r▶ {} - {}                              ", name, artists);
                                } else {
                                    print!("\r[Next]                                ");
                                }
                            }
                            Err(e) => print!("\rError: {}                    ", e),
                        }
                        io::stdout().flush()?;
                    }
                    KeyCode::Char('p') => {
                        match player.previous().await {
                            Ok(_) => {
                                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                                if let Ok(Some((name, artists))) = player.get_currently_playing().await {
                                    print!("\r▶ {} - {}                              ", name, artists);
                                } else {
                                    print!("\r[Prev]                                ");
                                }
                            }
                            Err(e) => print!("\rError: {}                    ", e),
                        }
                        io::stdout().flush()?;
                    }
                    KeyCode::Char('s') => {
                        shuffle_on = !shuffle_on;
                        match player.set_shuffle(shuffle_on).await {
                            Ok(_) => {
                                if shuffle_on {
                                    print!("\r[Shuffle: ON]                         ");
                                } else {
                                    print!("\r[Shuffle: OFF]                        ");
                                }
                            }
                            Err(e) => print!("\rError: {}                    ", e),
                        }
                        io::stdout().flush()?;
                    }
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    player.pause().await?; // Pause on quit
    println!();

    Ok(())
}

/// Play using mpv (for YouTube playlists)
async fn play_mpv(
    snap: &crate::provider::PlaylistSnapshot,
    shuffle: bool,
    grit_dir: &Path,
) -> Result<()> {
    use crate::cli::commands::utils::create_provider;

    let provider = create_provider(snap.provider, grit_dir)?;

    let mut queue = Queue::new(snap.tracks.clone());
    if shuffle {
        queue.toggle_shuffle();
        println!("Shuffle: ON");
    }

    let mut player = MpvPlayer::spawn().await?;

    if let Some(track) = queue.current_track() {
        let url = provider.playable_url(track).await?;
        println!(
        "\n[{}/{}] {} - {}",
        queue.position() + 1,
        queue.len(),
        track.name,
        track.artists.join(", ")
    );
        player.load(&url).await?;
    }

    println!("\nControls: [space] pause  [n] next  [p] prev  [s] shuffle  [q] quit");

    let mut is_paused = false;
    enable_raw_mode()?;

    loop {
        // Check for keyboard input (non-blocking)
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(KeyEvent { code, .. }) = event::read()? {
                match code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char(' ') => {
                        is_paused = !is_paused;
                        if is_paused {
                            player.pause().await?;
                        } else {
                            player.resume().await?;
                        }
                    }
                    KeyCode::Char('n') => {
                        if let Some(track) = queue.next() {
                            let (name, artists) = (track.name.clone(), track.artists.join(", "));
                            let url = provider.playable_url(track).await?;
                            print!(
                                "\r[{}/{}] {} - {}                    ",
                                queue.position() + 1,
                                queue.len(),
                                name,
                                artists
                            );
                            io::stdout().flush()?;
                            player.load(&url).await?;
                        }
                    }
                    KeyCode::Char('p') => {
                        if let Some(track) = queue.previous() {
                            let (name, artists) = (track.name.clone(), track.artists.join(", "));
                            let url = provider.playable_url(track).await?;
                            print!(
                                "\r[{}/{}] {} - {}                    ",
                                queue.position() + 1,
                                queue.len(),
                                name,
                                artists
                            );
                            io::stdout().flush()?;
                            player.load(&url).await?;
                        }
                    }
                    KeyCode::Char('s') => {
                        queue.toggle_shuffle();
                    }
                    _ => {}
                }
            }
        }

        // Check for mpv events (track ended)
        if let Some(event) = player.try_recv_event() {
            if MpvPlayer::is_track_finished(&event) {
                // Auto-advance to next track
                if let Some(track) = queue.next() {
                    let (name, artists) = (track.name.clone(), track.artists.join(", "));
                    let url = provider.playable_url(track).await?;
                    print!(
                        "\r[{}/{}] {} - {}                    ",
                        queue.position() + 1,
                        queue.len(),
                        name,
                        artists
                    );
                    io::stdout().flush()?;
                    player.load(&url).await?;
                } else {
                    println!("\nPlaylist finished");
                    break;
                }
            }
        }
    }

    disable_raw_mode()?;
    player.quit().await?;

    Ok(())
}
