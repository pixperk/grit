use crate::{playback::events::RepeatMode, provider::Track};
use rand::seq::SliceRandom;

pub struct Queue {
    tracks: Vec<Track>,
    current: usize,
    play_order: Vec<usize>,
    pub shuffle: bool,
    pub repeat: RepeatMode,
}

impl Queue {
    pub fn new(tracks: Vec<Track>) -> Self {
        let play_order: Vec<usize> = (0..tracks.len()).collect();
        Self {
            tracks,
            current: 0,
            play_order,
            shuffle: false,
            repeat: RepeatMode::None,
        }
    }

    pub fn current_track(&self) -> Option<&Track> {
        let track_idx = *self.play_order.get(self.current)?;
        self.tracks.get(track_idx)
    }

    pub fn next(&mut self) -> Option<&Track> {
        // RepeatMode::One - stay on same track
        if self.repeat == RepeatMode::One {
            return self.current_track();
        }
        // Try to advance
        if self.current + 1 < self.play_order.len() {
            self.current += 1;
            self.current_track()
        } else {
            // At the end
            match self.repeat {
                RepeatMode::All => {
                    self.current = 0; // Loop back
                    self.current_track()
                }
                RepeatMode::One => self.current_track(),
                RepeatMode::None => None, // Stop
            }
        }
    }

    pub fn previous(&mut self) -> Option<&Track> {
        if self.current > 0 {
            self.current -= 1;
        } else if self.repeat == RepeatMode::All {
            self.current = self.play_order.len().saturating_sub(1);
        }
        self.current_track()
    }

    pub fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;

        // Remember what track we're on
        let current_track_idx = self.play_order[self.current];

        if self.shuffle {
            // Shuffle the order
            let mut rng = rand::thread_rng();
            self.play_order.shuffle(&mut rng);
        } else {
            // Restore sequential order
            self.play_order = (0..self.tracks.len()).collect();
        }

        // Find where current track ended up
        self.current = self
            .play_order
            .iter()
            .position(|&i| i == current_track_idx)
            .unwrap_or(0);
    }

    pub fn jump_to(&mut self, index: usize) -> Option<&Track> {
        if index < self.play_order.len() {
            self.current = index;
            self.current_track()
        } else {
            None
        }
    }
}
