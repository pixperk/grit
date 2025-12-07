use crate::provider::ProviderKind;
use clap::{Parser, Subcommand};

/// plr - Git-like version control for your playlists
///
/// Track changes, sync across devices, and play music from
/// Spotify and YouTube with a unified terminal interface.
#[derive(Parser, Debug)]
#[command(name = "plr")]
#[command(version, about, long_about = None)]
pub struct Cli {
    #[arg(short, long, global = true)]
    pub provider: Option<ProviderKind>,

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
        /// Playlist URL or ID (e.g., https://open.spotify.com/playlist/37i9... or 37i9...)
        playlist: String,
        /// Provider (defaults to Spotify)
        #[arg(short, long)]
        provider: Option<ProviderKind>,
    },
    /// Pull latest changes from remote
    Pull,
    /// Push local changes to remote
    Push,
    /// Show sync status
    Status {
        /// Playlist ID or use --playlist
        #[arg(short = 'l', long)]
        playlist: Option<String>,
    },
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
    Search {
        /// Search query
        query: String,
    },
    Add {
        /// Track ID to add
        track_id: String,
    },
    Remove {
        /// Track ID to remove
        track_id: String,
    },
    Move {
        /// Track ID to move
        track_id: String,
        /// New position index
        new_index: usize,
    },
    Commit {
        /// Commit message
        #[arg(short, long)]
        message: String,
    },

    Diff {
        /// Show only staged changes
        #[arg(long)]
        staged: bool,
        /// Show only remote changes
        #[arg(long)]
        remote: bool,
    },

    /// Clear staged changes
    Reset {
        /// Playlist ID or use --playlist
        #[arg(short = 'l', long)]
        playlist: Option<String>,
    },
    /// List tracks in local playlist
    List {
        /// Playlist ID or use --playlist
        #[arg(short = 'l', long)]
        playlist: Option<String>,
    },
    /// Search within local playlist tracks
    Find {
        /// Search query
        query: String,
        /// Playlist ID or use --playlist
        #[arg(short = 'l', long)]
        playlist: Option<String>,
    },
    /// Delete credentials for a provider
    Logout {
        /// Provider to logout from
        provider: ProviderKind,
    },
    /// Show authenticated user info
    Whoami {
        /// Provider to check
        provider: ProviderKind,
    },
}
