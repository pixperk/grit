use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
pub enum ProviderKind {
    Spotify,
    Youtube,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    pub name: String,
    pub artists: Vec<String>,
    pub duration_ms: u64,
    pub provider: ProviderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistSnapshot {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub tracks: Vec<Track>,
    pub provider: ProviderKind,
    pub snapshot_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TrackChange {
    Added {
        track: Track,
        index: usize,
    },
    Removed {
        track: Track,
        index: usize,
    },
    Moved {
        track: Track,
        from: usize,
        to: usize,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiffPatch {
    pub changes: Vec<TrackChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<u64>, // unix timestamp
    pub token_type: String,
    pub scope: Option<String>,
}
