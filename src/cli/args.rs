use crate::provider::ProviderKind;
use clap::{Parser, Subcommand};

/// grit - Git-like version control for playlists
///
/// Track changes, sync across platforms, and play music from
/// Spotify and YouTube with a unified terminal interface.
#[derive(Parser, Debug)]
#[command(name = "grit")]
#[command(version)]
#[command(about = "Git-like version control for playlists")]
#[command(long_about = "grit - Version control for your music\n\n\
                  Track playlist changes, sync across Spotify and YouTube,\n\
                  and play music with a beautiful TUI.\n\n\
                  EXAMPLES:\n  \
                    grit auth spotify\n  \
                    grit init https://open.spotify.com/playlist/...\n  \
                    grit search \"lofi beats\"\n  \
                    grit add <track-id>\n  \
                    grit commit -m \"add chill vibes\"\n  \
                    grit push\n  \
                    grit play -l <playlist-id>")]
pub struct Cli {
    #[arg(
        short,
        long,
        global = true,
        help = "Override provider (spotify/youtube)"
    )]
    pub provider: Option<ProviderKind>,

    #[arg(short = 'l', long, global = true, help = "Playlist ID to operate on")]
    pub playlist: Option<String>,

    #[arg(
        short,
        long,
        global = true,
        default_value_t = false,
        help = "Enable verbose output"
    )]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialize tracking for a playlist (like 'git init')
    #[command(visible_alias = "i")]
    Init {
        #[arg(
            help = "Playlist URL or ID\n                       Example: https://open.spotify.com/playlist/37i9..."
        )]
        playlist: String,
        #[arg(
            short,
            long,
            help = "Provider (auto-detected from URL if not specified, defaults to Spotify)"
        )]
        provider: Option<ProviderKind>,
    },

    /// Pull latest changes from remote (like 'git pull')
    Pull,

    /// Show sync status (like 'git status')
    #[command(visible_alias = "st")]
    Status {
        #[arg(short = 'l', long, help = "Playlist ID or use --playlist")]
        playlist: Option<String>,
    },

    /// Show commit history (like 'git log')
    Log,

    /// Apply a playlist state from file
    Apply {
        #[arg(help = "Path to the YAML file")]
        file: String,
    },

    /// Start playback with TUI player
    #[command(visible_alias = "p")]
    Play {
        #[arg(short = 'l', long, help = "Playlist ID to play")]
        playlist: Option<String>,
        #[arg(short, long, help = "Start with shuffle enabled")]
        shuffle: bool,
    },

    /// Authenticate with Spotify or YouTube
    Auth {
        #[arg(help = "Provider: 'spotify' or 'youtube'")]
        provider: ProviderKind,
    },

    /// Search for tracks to add
    #[command(visible_alias = "s")]
    Search {
        #[arg(help = "Search query (e.g., \"lofi beats\")")]
        query: String,
    },

    /// Stage a track for addition (like 'git add')
    #[command(visible_alias = "a")]
    Add {
        #[arg(help = "Track ID from search results")]
        track_id: String,
    },

    /// Stage a track for removal (like 'git rm')
    #[command(visible_alias = "rm")]
    Remove {
        #[arg(help = "Track ID to remove")]
        track_id: String,
    },

    /// Stage a track to be moved
    #[command(visible_alias = "mv")]
    Move {
        #[arg(help = "Track ID to move")]
        track_id: String,
        #[arg(help = "New position (0-based index)")]
        new_index: usize,
    },

    /// Commit staged changes (like 'git commit')
    #[command(visible_alias = "c")]
    Commit {
        #[arg(short, long, help = "Commit message")]
        message: String,
    },

    /// Push local changes to remote (like 'git push')
    Push {
        #[arg(short = 'l', long, help = "Playlist ID to push")]
        playlist: Option<String>,
    },

    /// Show differences between versions (like 'git diff')
    #[command(visible_alias = "d")]
    Diff {
        #[arg(long, help = "Show only staged changes")]
        staged: bool,
        #[arg(long, help = "Show only remote changes")]
        remote: bool,
    },

    /// Clear staged changes (like 'git reset')
    Reset {
        #[arg(short = 'l', long, help = "Playlist ID")]
        playlist: Option<String>,
    },

    /// List tracks in local playlist
    #[command(visible_alias = "ls")]
    List {
        #[arg(short = 'l', long, help = "Playlist ID")]
        playlist: Option<String>,
    },

    /// Search within local playlist tracks
    Find {
        #[arg(help = "Search query")]
        query: String,
        #[arg(short = 'l', long, help = "Playlist ID")]
        playlist: Option<String>,
    },

    /// Delete credentials for a provider
    Logout {
        #[arg(help = "Provider: 'spotify' or 'youtube'")]
        provider: ProviderKind,
    },

    /// Show authenticated user info
    Whoami {
        #[arg(help = "Provider: 'spotify' or 'youtube'")]
        provider: ProviderKind,
    },

    /// List all tracked playlists
    Playlists {
        #[arg(help = "Optional search query to filter")]
        query: Option<String>,
    },

    /// Revert playlist to a previous commit
    Revert {
        #[arg(help = "Commit hash (defaults to previous commit)")]
        hash: Option<String>,
        #[arg(short = 'l', long, help = "Playlist ID")]
        playlist: Option<String>,
    },
}
