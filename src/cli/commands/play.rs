use anyhow::{bail, Context, Result};
use crossterm::event::KeyCode;
use std::path::Path;

use crate::playback::{fetch_audio_url, MpvPlayer, Queue, SpotifyPlayer};
use crate::provider::ProviderKind;
use crate::state::{credentials, snapshot};
use crate::tui::{App, PlayerBackend, Tui};

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

    match snap.provider {
        ProviderKind::Spotify => play_spotify(&snap, shuffle, grit_dir).await,
        ProviderKind::Youtube => play_mpv(&snap, shuffle, grit_dir).await,
    }
}

async fn play_spotify(
    snap: &crate::provider::PlaylistSnapshot,
    shuffle: bool,
    grit_dir: &Path,
) -> Result<()> {
    let token = credentials::load(grit_dir, ProviderKind::Spotify)?
        .context("No Spotify credentials. Run 'grit auth spotify' first.")?;

    let client_id = std::env::var("SPOTIFY_CLIENT_ID").context("SPOTIFY_CLIENT_ID not set")?;
    let client_secret =
        std::env::var("SPOTIFY_CLIENT_SECRET").context("SPOTIFY_CLIENT_SECRET not set")?;

    let mut player = SpotifyPlayer::new(token, client_id, client_secret, grit_dir);
    player.select_device().await?;

    let uris: Vec<String> = snap
        .tracks
        .iter()
        .map(|t| format!("spotify:track:{}", t.id))
        .collect();

    player.set_shuffle(shuffle).await?;
    player.play(uris, 0).await?;

    let mut app = App::new(
        snap.name.clone(),
        snap.tracks.clone(),
        PlayerBackend::Spotify,
    );
    app.shuffle = shuffle;

    let mut tui = Tui::new()?;
    let mut poll_counter = 0u8;

    loop {
        tui.draw(&app)?;

        if !app.is_paused {
            app.position_secs = (app.position_secs + 0.1).min(app.duration_secs);

            // Poll Spotify every ~3 seconds OR when track should have ended
            poll_counter = poll_counter.wrapping_add(1);
            let should_poll = poll_counter % 30 == 0
                || (app.position_secs >= app.duration_secs && app.duration_secs > 0.0);

            if should_poll {
                if let Ok(Some((name, _))) = player.get_currently_playing().await {
                    if app.current_track().map(|t| &t.name) != Some(&name) {
                        if let Some(idx) = app.tracks.iter().position(|t| t.name == name) {
                            app.current_index = idx;
                            app.position_secs = 0.0;
                            app.duration_secs = app.tracks[idx].duration_ms as f64 / 1000.0;
                        }
                    }
                }
            }
        }

        if let Some(key) = tui.poll_key()? {
            app.clear_error();
            match key {
                KeyCode::Char('q') => break,
                KeyCode::Char(' ') => {
                    app.is_paused = !app.is_paused;
                    let res = if app.is_paused {
                        player.pause().await
                    } else {
                        player.resume().await
                    };
                    if let Err(e) = res {
                        app.set_error(e.to_string());
                    }
                }
                KeyCode::Char('n') => {
                    if let Err(e) = player.next().await {
                        app.set_error(e.to_string());
                    } else {
                        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                        if let Ok(Some((name, _))) = player.get_currently_playing().await {
                            if let Some(idx) = app.tracks.iter().position(|t| t.name == name) {
                                app.current_index = idx;
                                app.position_secs = 0.0;
                                app.duration_secs = app.tracks[idx].duration_ms as f64 / 1000.0;
                            }
                        }
                    }
                }
                KeyCode::Char('p') => {
                    if let Err(e) = player.previous().await {
                        app.set_error(e.to_string());
                    } else {
                        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                        if let Ok(Some((name, _))) = player.get_currently_playing().await {
                            if let Some(idx) = app.tracks.iter().position(|t| t.name == name) {
                                app.current_index = idx;
                                app.position_secs = 0.0;
                                app.duration_secs = app.tracks[idx].duration_ms as f64 / 1000.0;
                            }
                        }
                    }
                }
                KeyCode::Char('s') => {
                    app.shuffle = !app.shuffle;
                    if let Err(e) = player.set_shuffle(app.shuffle).await {
                        app.set_error(e.to_string());
                    }
                }
                KeyCode::Left => {
                    let new_pos = (app.position_secs - 5.0).max(0.0);
                    if let Err(e) = player.seek(new_pos as u64).await {
                        app.set_error(e.to_string());
                    } else {
                        app.position_secs = new_pos;
                    }
                }
                KeyCode::Right => {
                    let new_pos = app.position_secs + 5.0;
                    if new_pos < app.duration_secs {
                        if let Err(e) = player.seek(new_pos as u64).await {
                            app.set_error(e.to_string());
                        } else {
                            app.position_secs = new_pos;
                        }
                    }
                }
                KeyCode::Up => {
                    app.select_prev();
                }
                KeyCode::Down => {
                    app.select_next();
                }
                KeyCode::Enter => {
                    let idx = app.selected_index;
                    if idx != app.current_index && idx < app.tracks.len() {
                        // Jump to selected track by replaying context with offset
                        let uris: Vec<String> = app.tracks.iter()
                            .map(|t| format!("spotify:track:{}", t.id))
                            .collect();
                        if let Err(e) = player.play(uris, idx).await {
                            app.set_error(e.to_string());
                        } else {
                            app.current_index = idx;
                            app.position_secs = 0.0;
                            app.duration_secs = app.tracks[idx].duration_ms as f64 / 1000.0;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    tui.restore()?;
    let _ = player.pause().await;
    Ok(())
}

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
    }

    let mut player = MpvPlayer::spawn().await?;

    let mut app = App::new(snap.name.clone(), snap.tracks.clone(), PlayerBackend::Mpv);
    app.shuffle = shuffle;
    app.loading = true;
    let mut skip_position = 0u8; // Skip position queries after track change

    let mut tui = Tui::new()?;
    tui.draw(&app)?;

    if let Some(track) = queue.current_track().cloned() {
        let yt_url = provider.playable_url(&track).await?;
        match fetch_audio_url(&yt_url).await {
            Ok(audio_url) => {
                player.load(&audio_url).await?;
            }
            Err(e) => {
                app.set_error(format!("Failed to load: {}", e));
            }
        }
        app.duration_secs = track.duration_ms as f64 / 1000.0;
        // Find actual index in tracks list
        if let Some(idx) = app.tracks.iter().position(|t| t.id == track.id) {
            app.current_index = idx;
        }
        skip_position = 5; // Skip first few position queries
    }
    app.loading = false;

    loop {
        tui.draw(&app)?;

        if !app.is_paused && skip_position == 0 {
            if let Ok(Some(pos)) = player.get_position().await {
                app.position_secs = pos.min(app.duration_secs);
            }
        } else if skip_position > 0 {
            skip_position -= 1;
        }

        if let Some(key) = tui.poll_key()? {
            app.clear_error();
            match key {
                KeyCode::Char('q') => break,
                KeyCode::Char(' ') => {
                    app.is_paused = !app.is_paused;
                    let res = if app.is_paused {
                        player.pause().await
                    } else {
                        player.resume().await
                    };
                    if let Err(e) = res {
                        app.set_error(e.to_string());
                    }
                }
                KeyCode::Char('n') => {
                    if let Some(track) = queue.next().cloned() {
                        app.loading = true;
                        // Find actual index in tracks list and update immediately
                        if let Some(idx) = app.tracks.iter().position(|t| t.id == track.id) {
                            app.current_index = idx;
                        }
                        app.position_secs = 0.0;
                        app.duration_secs = track.duration_ms as f64 / 1000.0;
                        tui.draw(&app)?;
                        match provider.playable_url(&track).await {
                            Ok(yt_url) => match fetch_audio_url(&yt_url).await {
                                Ok(audio_url) => {
                                    if let Err(e) = player.load(&audio_url).await {
                                        app.set_error(e.to_string());
                                    }
                                }
                                Err(e) => app.set_error(e.to_string()),
                            },
                            Err(e) => app.set_error(e.to_string()),
                        }
                        app.loading = false;
                        skip_position = 5;
                    }
                }
                KeyCode::Char('p') => {
                    if let Some(track) = queue.previous().cloned() {
                        app.loading = true;
                        // Find actual index in tracks list and update immediately
                        if let Some(idx) = app.tracks.iter().position(|t| t.id == track.id) {
                            app.current_index = idx;
                        }
                        app.position_secs = 0.0;
                        app.duration_secs = track.duration_ms as f64 / 1000.0;
                        tui.draw(&app)?;
                        match provider.playable_url(&track).await {
                            Ok(yt_url) => match fetch_audio_url(&yt_url).await {
                                Ok(audio_url) => {
                                    if let Err(e) = player.load(&audio_url).await {
                                        app.set_error(e.to_string());
                                    }
                                }
                                Err(e) => app.set_error(e.to_string()),
                            },
                            Err(e) => app.set_error(e.to_string()),
                        }
                        app.loading = false;
                        skip_position = 5;
                    }
                }
                KeyCode::Char('s') => {
                    queue.toggle_shuffle();
                    app.shuffle = !app.shuffle;
                }
                KeyCode::Left => {
                    let _ = player.seek(-5).await;
                }
                KeyCode::Right => {
                    let _ = player.seek(5).await;
                }
                KeyCode::Up => {
                    app.select_prev();
                }
                KeyCode::Down => {
                    app.select_next();
                }
                KeyCode::Enter => {
                    let idx = app.selected_index;
                    if idx != app.current_index && idx < app.tracks.len() {
                        if let Some(track) = app.tracks.get(idx).cloned() {
                            app.loading = true;
                            app.current_index = idx;
                            app.position_secs = 0.0;
                            app.duration_secs = track.duration_ms as f64 / 1000.0;
                            queue.jump_to(idx);
                            tui.draw(&app)?;
                            match provider.playable_url(&track).await {
                                Ok(yt_url) => match fetch_audio_url(&yt_url).await {
                                    Ok(audio_url) => {
                                        if let Err(e) = player.load(&audio_url).await {
                                            app.set_error(e.to_string());
                                        }
                                    }
                                    Err(e) => app.set_error(e.to_string()),
                                },
                                Err(e) => app.set_error(e.to_string()),
                            }
                            app.loading = false;
                            skip_position = 5;
                        }
                    }
                }
                _ => {}
            }
        }

        // Check for track end and auto-advance
        while let Some(event) = player.try_recv_event() {
            if MpvPlayer::is_track_finished(&event) {
                if let Some(track) = queue.next().cloned() {
                    app.loading = true;
                    // Find actual index in tracks list and update immediately
                    if let Some(idx) = app.tracks.iter().position(|t| t.id == track.id) {
                        app.current_index = idx;
                    }
                    app.position_secs = 0.0;
                    app.duration_secs = track.duration_ms as f64 / 1000.0;
                    tui.draw(&app)?;
                    if let Ok(yt_url) = provider.playable_url(&track).await {
                        match fetch_audio_url(&yt_url).await {
                            Ok(audio_url) => {
                                let _ = player.load(&audio_url).await;
                            }
                            Err(e) => app.set_error(e.to_string()),
                        }
                    }
                    app.loading = false;
                    skip_position = 5;
                    tui.draw(&app)?;
                }
            }
        }
    }

    tui.restore()?;
    player.quit().await?;
    Ok(())
}
