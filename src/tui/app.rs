use crate::playback::events::RepeatMode;
use crate::playback::Lyrics;
use crate::provider::Track;

pub enum PlayerBackend {
    Mpv,
    Spotify,
}

pub struct App {
    pub playlist_name: String,
    pub tracks: Vec<Track>,
    pub current_index: usize,
    pub selected_index: usize,
    pub is_paused: bool,
    pub shuffle: bool,
    pub repeat_mode: RepeatMode,
    pub position_secs: f64,
    pub duration_secs: f64,
    pub backend: PlayerBackend,
    pub error: Option<String>,
    pub loading: bool,
    pub seek_position: Option<f64>,
    pub search_query: Option<String>,
    pub search_matches: Vec<usize>,
    pub search_match_index: usize,
    pub lyrics: Option<Lyrics>,
    pub show_lyrics: bool,
    pub lyrics_loading: bool,
    pub lyrics_scroll: usize,
    pub lyrics_auto_scroll: bool,
    pub search_blocked: bool,
}

impl App {
    pub fn new(playlist_name: String, tracks: Vec<Track>, backend: PlayerBackend) -> Self {
        let duration = tracks
            .first()
            .map(|t| t.duration_ms as f64 / 1000.0)
            .unwrap_or(0.0);
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
            seek_position: None,
            search_query: None,
            search_matches: Vec::new(),
            search_match_index: 0,
            lyrics: None,
            show_lyrics: false,
            lyrics_loading: false,
            lyrics_scroll: 0,
            lyrics_auto_scroll: true,
            search_blocked: false,
        }
    }

    pub fn toggle_lyrics(&mut self) {
        self.show_lyrics = !self.show_lyrics;
    }

    pub fn lyrics_scroll_up(&mut self) {
        self.lyrics_scroll = self.lyrics_scroll.saturating_sub(1);
        self.lyrics_auto_scroll = false;
    }

    pub fn lyrics_scroll_down(&mut self, max_lines: usize) {
        if self.lyrics_scroll < max_lines.saturating_sub(1) {
            self.lyrics_scroll += 1;
        }
        self.lyrics_auto_scroll = false;
    }

    pub fn lyrics_toggle_auto_scroll(&mut self) {
        self.lyrics_auto_scroll = !self.lyrics_auto_scroll;
    }

    pub fn lyrics_line_count(&self) -> usize {
        self.lyrics
            .as_ref()
            .map(|l| {
                if l.lines.is_empty() {
                    l.plain.as_ref().map(|p| p.lines().count()).unwrap_or(0)
                } else {
                    l.lines.len()
                }
            })
            .unwrap_or(0)
    }

    pub fn reset_lyrics_scroll(&mut self) {
        self.lyrics_scroll = 0;
        self.lyrics_auto_scroll = true;
    }

    pub fn current_lyric_index(&self) -> Option<usize> {
        self.lyrics.as_ref()?.current_line_index(self.position_secs)
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

    #[allow(dead_code)]
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

    pub fn start_seeking(&mut self) {
        self.seek_position = Some(self.position_secs);
    }

    pub fn cancel_seeking(&mut self) {
        self.seek_position = None;
    }

    pub fn seek_forward(&mut self, secs: f64) {
        if let Some(ref mut pos) = self.seek_position {
            *pos = (*pos + secs).min(self.duration_secs);
        }
    }

    pub fn seek_backward(&mut self, secs: f64) {
        if let Some(ref mut pos) = self.seek_position {
            *pos = (*pos - secs).max(0.0);
        }
    }

    pub fn get_seek_position(&self) -> Option<f64> {
        self.seek_position
    }

    pub fn is_seeking(&self) -> bool {
        self.seek_position.is_some()
    }

    pub fn seek_progress(&self) -> f64 {
        if let Some(pos) = self.seek_position {
            if self.duration_secs > 0.0 {
                return (pos / self.duration_secs).min(1.0);
            }
        }
        self.progress()
    }

    pub fn start_search(&mut self) {
        self.search_query = Some(String::new());
        self.search_matches.clear();
        self.search_match_index = 0;
    }

    pub fn cancel_search(&mut self) {
        self.search_query = None;
        self.search_matches.clear();
        self.search_match_index = 0;
    }

    pub fn push_search_char(&mut self, c: char) {
        if let Some(ref mut query) = self.search_query {
            query.push(c);
            self.update_search_matches();
        }
    }

    pub fn pop_search_char(&mut self) {
        if let Some(ref mut query) = self.search_query {
            query.pop();
            self.update_search_matches();
        }
    }

    fn update_search_matches(&mut self) {
        self.search_matches.clear();
        if let Some(ref query) = self.search_query {
            if !query.is_empty() {
                let query_lower = query.to_lowercase();
                for (i, track) in self.tracks.iter().enumerate() {
                    if track.name.to_lowercase().contains(&query_lower)
                        || track
                            .artists
                            .iter()
                            .any(|a| a.to_lowercase().contains(&query_lower))
                    {
                        self.search_matches.push(i);
                    }
                }
                if !self.search_matches.is_empty() {
                    self.search_match_index = 0;
                    self.selected_index = self.search_matches[0];
                }
            }
        }
    }

    pub fn next_search_match(&mut self) {
        if !self.search_matches.is_empty() {
            self.search_match_index = (self.search_match_index + 1) % self.search_matches.len();
            self.selected_index = self.search_matches[self.search_match_index];
        }
    }

    pub fn prev_search_match(&mut self) {
        if !self.search_matches.is_empty() {
            self.search_match_index = if self.search_match_index == 0 {
                self.search_matches.len() - 1
            } else {
                self.search_match_index - 1
            };
            self.selected_index = self.search_matches[self.search_match_index];
        }
    }

    pub fn is_searching(&self) -> bool {
        self.search_query.is_some()
    }

    pub fn is_search_match(&self, index: usize) -> bool {
        self.search_matches.contains(&index)
    }
}
