pub mod spotify;
mod traits;
mod types;
pub mod youtube;

pub use spotify::SpotifyProvider;
pub use traits::Provider;
pub use types::*;
pub use youtube::YoutubeProvider;
