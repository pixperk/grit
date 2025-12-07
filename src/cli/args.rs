use clap::{Parser, Subcommand};
use crate::provider::ProviderKind;

/// plr - Git-like version control for your playlists
///
/// Track changes, sync across devices, and play music from
/// Spotify and YouTube with a unified terminal interface.
#[derive(Parser, Debug)]
#[command(name = "plr")]
#[command(version, about, long_about = None)]
pub struct Cli {
    #[arg(short, long, global = true)]
    pub provider : Option<ProviderKind>,

    #[arg(short = 'l', long, global = true)]
    pub playlist: Option<String>,

    #[arg(short, long, global = true, default_value_t = false)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialize tracking for a playlist
    Init {
        /// Playlist ID to initialize
        playlist_id: String,
    },
    /// Pull latest changes from remote
    Pull,
    /// Push local changes to remote
    Push,
    /// Show sync status
    Status,
    /// Show diff between local and remote
    Diff,
    /// Show change history
    Log,
    /// Apply a playlist state from file
    Apply {
        /// Path to the YAML file
        file: String,
    },
    /// Start playback
    Play,
    /// Authenticate with a provider
    Auth {
        /// Provider to authenticate
        provider: ProviderKind,
    },
}