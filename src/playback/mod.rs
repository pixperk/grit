pub mod events;
pub mod mpv;
pub mod queue;
pub mod spotify;

pub use mpv::{fetch_audio_url, MpvPlayer};
pub use queue::Queue;
pub use spotify::SpotifyPlayer;
