use crate::playback::events::RepeatMode;
use crate::provider::Track;

pub enum PlayerBackend {
    Mpv,
    Spotify,
}

pub struct App {
    pub playlist_name: String,
    pub tracks: Vec<Track>,
    pub current_index: usize,
    pub selected_index: usize, // For playlist navigation
    pub is_paused: bool,
    pub shuffle: bool,
    pub repeat_mode: RepeatMode,
    pub position_secs: f64,
    pub duration_secs: f64,
    pub backend: PlayerBackend,
    pub error: Option<String>,
    pub loading: bool,
}

impl App {
    pub fn new(playlist_name: String, tracks: Vec<Track>, backend: PlayerBackend) -> Self {
        let duration = tracks.first().map(|t| t.duration_ms as f64 / 1000.0).unwrap_or(0.0);
        Self {
            playlist_name,
            tracks,
            current_index: 0,
            selected_index: 0,
            is_paused: false,
            shuffle: false,
            repeat_mode: RepeatMode::None,
            position_secs: 0.0,
            duration_secs: duration,
            backend,
            error: None,
            loading: false,
        }
    }

    pub fn current_track(&self) -> Option<&Track> {
        self.tracks.get(self.current_index)
    }

    pub fn next_track(&self) -> Option<&Track> {
        let next_idx = self.current_index + 1;
        if next_idx < self.tracks.len() {
            self.tracks.get(next_idx)
        } else if self.repeat_mode == RepeatMode::All {
            // Wrap around to first track
            self.tracks.first()
        } else {
            None
        }
    }

    pub fn progress(&self) -> f64 {
        if self.duration_secs > 0.0 {
            (self.position_secs / self.duration_secs).min(1.0)
        } else {
            0.0
        }
    }

    pub fn format_time(secs: f64) -> String {
        let mins = (secs / 60.0) as u64;
        let secs = (secs % 60.0) as u64;
        format!("{}:{:02}", mins, secs)
    }

    pub fn set_error(&mut self, msg: String) {
        self.error = Some(msg);
    }

    pub fn clear_error(&mut self) {
        self.error = None;
    }

    pub fn select_next(&mut self) {
        if self.selected_index < self.tracks.len().saturating_sub(1) {
            self.selected_index += 1;
        }
    }

    pub fn select_prev(&mut self) {
        self.selected_index = self.selected_index.saturating_sub(1);
    }

    pub fn selected_track(&self) -> Option<&Track> {
        self.tracks.get(self.selected_index)
    }

    pub fn cycle_repeat(&mut self) {
        self.repeat_mode = match self.repeat_mode {
            RepeatMode::None => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::None,
        };
    }
}
