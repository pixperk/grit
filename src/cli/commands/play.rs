use anyhow::{bail, Context, Result};
use crossterm::event::{KeyCode, KeyModifiers};
use std::path::Path;

use crate::playback::{fetch_audio_url, LyricsFetcher, MpvPlayer, Queue, SpotifyPlayer};
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
        ProviderKind::Spotify => play_spotify(&snap, shuffle, grit_dir, &snapshot_path).await,
        ProviderKind::Youtube => play_mpv(&snap, shuffle, grit_dir, &snapshot_path).await,
    }
}

async fn play_spotify(
    snap: &crate::provider::PlaylistSnapshot,
    shuffle: bool,
    grit_dir: &Path,
    snapshot_path: &Path,
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
    let mut last_update = std::time::Instant::now();
    let mut last_modified = std::fs::metadata(snapshot_path)
        .and_then(|m| m.modified())
        .ok();

    let mut lyrics_fetcher = LyricsFetcher::new();

    loop {
        if let Some(lyrics) = lyrics_fetcher.try_recv() {
            app.lyrics = Some(lyrics);
            app.lyrics_loading = false;
        }

        tui.draw(&app)?;
        poll_counter = poll_counter.wrapping_add(1);

        if !app.is_paused {
            let now = std::time::Instant::now();
            let elapsed = now.duration_since(last_update).as_secs_f64();
            last_update = now;
            app.position_secs = (app.position_secs + elapsed).min(app.duration_secs);

            let should_poll = poll_counter.is_multiple_of(30)
                || (app.position_secs >= app.duration_secs && app.duration_secs > 0.0);

            if should_poll {
                use crate::playback::events::RepeatMode;

                if let Ok(Some((name, _))) = player.get_currently_playing().await {
                    if app.current_track().map(|t| &t.name) != Some(&name) {
                        if let Some(idx) = app.tracks.iter().position(|t| t.name == name) {
                            if app.repeat_mode == RepeatMode::One {
                                let current_idx = app.current_index;
                                let uris: Vec<String> = app
                                    .tracks
                                    .iter()
                                    .map(|t| format!("spotify:track:{}", t.id))
                                    .collect();
                                let _ = player.play(uris, current_idx).await;
                                app.position_secs = 0.0;
                            } else {
                                app.current_index = idx;
                                app.position_secs = 0.0;
                                app.duration_secs = app.tracks[idx].duration_ms as f64 / 1000.0;
                                // Clear lyrics for new track
                                app.lyrics = None;
                            }
                        }
                    }
                } else if app.repeat_mode == RepeatMode::All
                    && app.current_index == app.tracks.len() - 1
                {
                    let uris: Vec<String> = app
                        .tracks
                        .iter()
                        .map(|t| format!("spotify:track:{}", t.id))
                        .collect();
                    let _ = player.play(uris, 0).await;
                    app.current_index = 0;
                    app.position_secs = 0.0;
                    app.duration_secs = app.tracks[0].duration_ms as f64 / 1000.0;
                }
            }
        }

        if poll_counter.is_multiple_of(50) {
            let current_modified = std::fs::metadata(snapshot_path)
                .and_then(|m| m.modified())
                .ok();
            if current_modified != last_modified {
                if let Ok(new_snap) = snapshot::load(snapshot_path) {
                    app.tracks = new_snap.tracks;
                    last_modified = current_modified;
                }
            }
        }

        if let Some(key) = tui.poll_key()? {
            if app.is_searching() {
                match (key.code, key.modifiers) {
                    (KeyCode::Esc, _) => app.cancel_search(),
                    (KeyCode::Enter, _) => {
                        let idx = app.selected_index;
                        app.cancel_search();
                        if idx != app.current_index && idx < app.tracks.len() {
                            let uris: Vec<String> = app
                                .tracks
                                .iter()
                                .map(|t| format!("spotify:track:{}", t.id))
                                .collect();
                            if let Err(e) = player.play(uris, idx).await {
                                app.set_error(e.to_string());
                            } else {
                                app.current_index = idx;
                                app.position_secs = 0.0;
                                app.duration_secs = app.tracks[idx].duration_ms as f64 / 1000.0;
                                app.lyrics = None;
                                app.reset_lyrics_scroll();
                            }
                        }
                    }
                    (KeyCode::Char('n'), m) if m.contains(KeyModifiers::CONTROL) => {
                        app.next_search_match()
                    }
                    (KeyCode::Char('p'), m) if m.contains(KeyModifiers::CONTROL) => {
                        app.prev_search_match()
                    }
                    (KeyCode::Up, _) => app.select_prev(),
                    (KeyCode::Down, _) => app.select_next(),
                    (KeyCode::Backspace, _) => app.pop_search_char(),
                    (KeyCode::Char(c), _) => app.push_search_char(c),
                    _ => {}
                }
                continue;
            }

            if app.is_seeking() {
                match key.code {
                    KeyCode::Esc => app.cancel_seeking(),
                    KeyCode::Enter => {
                        if let Some(secs) = app.get_seek_position() {
                            if let Err(e) = player.seek(secs as u64).await {
                                app.set_error(e.to_string());
                            } else {
                                app.position_secs = secs;
                            }
                        }
                        app.cancel_seeking();
                    }
                    KeyCode::Left => app.seek_backward(5.0),
                    KeyCode::Right => app.seek_forward(5.0),
                    _ => {}
                }
                continue;
            }

            match key.code {
                KeyCode::Char('/') if app.show_lyrics => {
                    app.search_blocked = true;
                }
                _ => {
                    app.search_blocked = false;
                    app.clear_error();
                }
            }
            match key.code {
                KeyCode::Char('q') => break,
                KeyCode::Char('/') => {
                    if !app.show_lyrics {
                        app.start_search();
                    }
                }
                KeyCode::Char('g') => app.start_seeking(),
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
                                app.lyrics = None;
                                app.reset_lyrics_scroll();
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
                                app.lyrics = None;
                                app.reset_lyrics_scroll();
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
                KeyCode::Char('r') => {
                    app.cycle_repeat();
                    if let Err(e) = player.set_repeat(app.repeat_mode).await {
                        app.set_error(e.to_string());
                    }
                }
                KeyCode::Char('l') => {
                    app.toggle_lyrics();
                }
                KeyCode::Char('a') if app.show_lyrics => {
                    app.lyrics_toggle_auto_scroll();
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
                    if app.show_lyrics {
                        app.lyrics_scroll_up();
                    } else {
                        app.select_prev();
                    }
                }
                KeyCode::Down => {
                    if app.show_lyrics {
                        let max_lines = app.lyrics_line_count();
                        app.lyrics_scroll_down(max_lines);
                    } else {
                        app.select_next();
                    }
                }
                KeyCode::Enter => {
                    let idx = app.selected_index;
                    if idx != app.current_index && idx < app.tracks.len() {
                        let uris: Vec<String> = app
                            .tracks
                            .iter()
                            .map(|t| format!("spotify:track:{}", t.id))
                            .collect();
                        if let Err(e) = player.play(uris, idx).await {
                            app.set_error(e.to_string());
                        } else {
                            app.current_index = idx;
                            app.position_secs = 0.0;
                            app.duration_secs = app.tracks[idx].duration_ms as f64 / 1000.0;
                            app.lyrics = None;
                        }
                    }
                }
                _ => {}
            }
        }

        if app.show_lyrics && app.lyrics.is_none() && !app.lyrics_loading {
            if let Some(track) = app.current_track() {
                let artist = track.artists.first().map(|s| s.as_str()).unwrap_or("");
                let duration = track.duration_ms / 1000;
                lyrics_fetcher.fetch_for_track(&track.id, &track.name, artist, duration);
                app.lyrics_loading = true;
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
    snapshot_path: &Path,
) -> Result<()> {
    use crate::cli::commands::utils::create_provider;

    let provider = create_provider(snap.provider, grit_dir)?;
    let mut queue = Queue::new(snap.tracks.clone());

    if shuffle {
        queue.toggle_shuffle();
    }

    let mut player = MpvPlayer::spawn().await?;
    player.observe_eof_reached().await?;

    let mut app = App::new(snap.name.clone(), snap.tracks.clone(), PlayerBackend::Mpv);
    app.shuffle = shuffle;
    app.loading = true;
    let mut skip_position = 0u8;
    let mut last_seek = std::time::Instant::now();
    let mut last_modified = std::fs::metadata(snapshot_path)
        .and_then(|m| m.modified())
        .ok();
    let mut file_check_counter = 0u8;

    let mut tui = Tui::new()?;
    tui.draw(&app)?;

    let mut lyrics_fetcher = LyricsFetcher::new();

    if let Some(track) = queue.current_track().cloned() {
        let yt_url = provider.playable_url(&track).await?;
        match fetch_audio_url(&yt_url).await {
            Ok(audio_url) => {
                if let Err(e) = player.load(&audio_url).await {
                    app.set_error(format!("Failed to load: {}", e));
                }
            }
            Err(e) => {
                app.set_error(format!("Failed to load: {}", e));
            }
        }
        app.duration_secs = track.duration_ms as f64 / 1000.0;
        if let Some(idx) = app.tracks.iter().position(|t| t.id == track.id) {
            app.current_index = idx;
        }
        skip_position = 5;
    }
    app.loading = false;

    loop {
        if let Some(lyrics) = lyrics_fetcher.try_recv() {
            app.lyrics = Some(lyrics);
            app.lyrics_loading = false;
        }

        tui.draw(&app)?;

        if !app.is_paused && skip_position == 0 {
            if let Ok(Some(pos)) = player.get_position().await {
                app.position_secs = pos.min(app.duration_secs);
            }
        } else {
            skip_position = skip_position.saturating_sub(1);
        }

        file_check_counter = file_check_counter.wrapping_add(1);
        if file_check_counter.is_multiple_of(100) {
            let current_modified = std::fs::metadata(snapshot_path)
                .and_then(|m| m.modified())
                .ok();
            if current_modified != last_modified {
                if let Ok(new_snap) = snapshot::load(snapshot_path) {
                    app.tracks = new_snap.tracks.clone();
                    queue = Queue::new(new_snap.tracks);
                    last_modified = current_modified;
                }
            }
        }

        if let Some(key) = tui.poll_key()? {
            if app.is_searching() {
                match (key.code, key.modifiers) {
                    (KeyCode::Esc, _) => app.cancel_search(),
                    (KeyCode::Enter, _) => {
                        let idx = app.selected_index;
                        app.cancel_search();
                        if idx != app.current_index && idx < app.tracks.len() {
                            if let Some(track) = app.tracks.get(idx).cloned() {
                                app.loading = true;
                                app.current_index = idx;
                                app.position_secs = 0.0;
                                app.duration_secs = track.duration_ms as f64 / 1000.0;
                                app.lyrics = None;
                                app.lyrics_loading = false;
                                app.reset_lyrics_scroll();
                                lyrics_fetcher.reset();
                                queue.jump_to(idx);
                                tui.draw(&app)?;
                                match provider.playable_url(&track).await {
                                    Ok(yt_url) => match fetch_audio_url(&yt_url).await {
                                        Ok(audio_url) => {
                                            while player.try_recv_event().is_some() {}
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
                    (KeyCode::Char('n'), m) if m.contains(KeyModifiers::CONTROL) => {
                        app.next_search_match()
                    }
                    (KeyCode::Char('p'), m) if m.contains(KeyModifiers::CONTROL) => {
                        app.prev_search_match()
                    }
                    (KeyCode::Up, _) => app.select_prev(),
                    (KeyCode::Down, _) => app.select_next(),
                    (KeyCode::Backspace, _) => app.pop_search_char(),
                    (KeyCode::Char(c), _) => app.push_search_char(c),
                    _ => {}
                }
                continue;
            }

            if app.is_seeking() {
                match key.code {
                    KeyCode::Esc => app.cancel_seeking(),
                    KeyCode::Enter => {
                        if let Some(secs) = app.get_seek_position() {
                            if let Err(e) = player.seek_absolute(secs).await {
                                app.set_error(e.to_string());
                            } else {
                                app.position_secs = secs;
                                skip_position = 3;
                            }
                        }
                        app.cancel_seeking();
                    }
                    KeyCode::Left => app.seek_backward(5.0),
                    KeyCode::Right => app.seek_forward(5.0),
                    _ => {}
                }
                continue;
            }

            match key.code {
                KeyCode::Char('/') if app.show_lyrics => {
                    app.search_blocked = true;
                }
                _ => {
                    app.search_blocked = false;
                    app.clear_error();
                }
            }
            match key.code {
                KeyCode::Char('q') => break,
                KeyCode::Char('/') => {
                    if !app.show_lyrics {
                        app.start_search();
                    }
                }
                KeyCode::Char('g') => app.start_seeking(),
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
                    use crate::playback::events::RepeatMode;

                    let track = match queue.next() {
                        Some(track) => Some(track.clone()),
                        None if app.repeat_mode == RepeatMode::All => {
                            queue.jump_to(0);
                            queue.current_track().cloned()
                        }
                        None => None,
                    };

                    if let Some(track) = track {
                        app.loading = true;
                        if let Some(idx) = app.tracks.iter().position(|t| t.id == track.id) {
                            app.current_index = idx;
                        }
                        app.position_secs = 0.0;
                        app.duration_secs = track.duration_ms as f64 / 1000.0;
                        app.lyrics = None;
                        app.lyrics_loading = false;
                        app.reset_lyrics_scroll();
                        lyrics_fetcher.reset();
                        tui.draw(&app)?;
                        match provider.playable_url(&track).await {
                            Ok(yt_url) => match fetch_audio_url(&yt_url).await {
                                Ok(audio_url) => {
                                    while player.try_recv_event().is_some() {}
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
                        if let Some(idx) = app.tracks.iter().position(|t| t.id == track.id) {
                            app.current_index = idx;
                        }
                        app.position_secs = 0.0;
                        app.duration_secs = track.duration_ms as f64 / 1000.0;
                        app.lyrics = None;
                        app.lyrics_loading = false;
                        app.reset_lyrics_scroll();
                        lyrics_fetcher.reset();
                        tui.draw(&app)?;
                        match provider.playable_url(&track).await {
                            Ok(yt_url) => match fetch_audio_url(&yt_url).await {
                                Ok(audio_url) => {
                                    while player.try_recv_event().is_some() {}
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
                KeyCode::Char('r') => {
                    app.cycle_repeat();
                }
                KeyCode::Left => {
                    let now = std::time::Instant::now();
                    if now.duration_since(last_seek).as_millis() >= 150 {
                        if let Err(e) = player.seek(-5).await {
                            app.set_error(e.to_string());
                        } else {
                            app.position_secs = (app.position_secs - 5.0).max(0.0);
                            skip_position = 3;
                            last_seek = now;
                        }
                    }
                }
                KeyCode::Right => {
                    let now = std::time::Instant::now();
                    if now.duration_since(last_seek).as_millis() >= 150 {
                        if let Err(e) = player.seek(5).await {
                            app.set_error(e.to_string());
                        } else {
                            app.position_secs = (app.position_secs + 5.0).min(app.duration_secs);
                            skip_position = 3;
                            last_seek = now;
                        }
                    }
                }
                KeyCode::Char('l') => {
                    app.toggle_lyrics();
                }
                KeyCode::Char('a') if app.show_lyrics => {
                    app.lyrics_toggle_auto_scroll();
                }
                KeyCode::Up => {
                    if app.show_lyrics {
                        app.lyrics_scroll_up();
                    } else {
                        app.select_prev();
                    }
                }
                KeyCode::Down => {
                    if app.show_lyrics {
                        let max_lines = app.lyrics_line_count();
                        app.lyrics_scroll_down(max_lines);
                    } else {
                        app.select_next();
                    }
                }
                KeyCode::Enter => {
                    let idx = app.selected_index;
                    if idx != app.current_index && idx < app.tracks.len() {
                        if let Some(track) = app.tracks.get(idx).cloned() {
                            app.loading = true;
                            app.current_index = idx;
                            app.position_secs = 0.0;
                            app.duration_secs = track.duration_ms as f64 / 1000.0;
                            app.lyrics = None;
                            app.lyrics_loading = false;
                            app.reset_lyrics_scroll();
                            lyrics_fetcher.reset();
                            queue.jump_to(idx);
                            tui.draw(&app)?;
                            match provider.playable_url(&track).await {
                                Ok(yt_url) => match fetch_audio_url(&yt_url).await {
                                    Ok(audio_url) => {
                                        while player.try_recv_event().is_some() {}
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

        if app.show_lyrics && app.lyrics.is_none() && !app.lyrics_loading {
            if let Some(track) = app.current_track() {
                let duration = track.duration_ms / 1000;
                lyrics_fetcher.fetch_for_yt(&track.id, &track.name, duration);
                app.lyrics_loading = true;
            }
        }

        while let Some(event) = player.try_recv_event() {
            if MpvPlayer::is_track_finished(&event) {
                use crate::playback::events::RepeatMode;

                let track = if app.repeat_mode == RepeatMode::One {
                    queue.current_track().cloned()
                } else {
                    match queue.next() {
                        Some(track) => Some(track.clone()),
                        None if app.repeat_mode == RepeatMode::All => {
                            queue.jump_to(0);
                            queue.current_track().cloned()
                        }
                        None => None,
                    }
                };

                if let Some(track) = track {
                    app.loading = true;
                    if let Some(idx) = app.tracks.iter().position(|t| t.id == track.id) {
                        app.current_index = idx;
                    }
                    app.position_secs = 0.0;
                    app.duration_secs = track.duration_ms as f64 / 1000.0;
                    app.lyrics = None;
                    app.lyrics_loading = false;
                    app.reset_lyrics_scroll();
                    lyrics_fetcher.reset();
                    tui.draw(&app)?;

                    if let Ok(yt_url) = provider.playable_url(&track).await {
                        match fetch_audio_url(&yt_url).await {
                            Ok(audio_url) => {
                                while player.try_recv_event().is_some() {}
                                if let Err(e) = player.load(&audio_url).await {
                                    app.set_error(e.to_string());
                                }
                            }
                            Err(e) => app.set_error(e.to_string()),
                        }
                    } else {
                        app.set_error("Failed to get playable URL".to_string());
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
