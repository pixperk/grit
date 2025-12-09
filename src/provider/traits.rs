use crate::provider::{DiffPatch, OAuthToken, PlaylistSnapshot, Track};
use async_trait::async_trait;

#[async_trait]
pub trait Provider: Send + Sync {
    /// Fetch playlist snapshot from remote
    async fn fetch(&self, playlist_id: &str) -> anyhow::Result<PlaylistSnapshot>;

    /// Apply changes to remote playlist to match desired state
    async fn apply(
        &self,
        playlist_id: &str,
        patch: &DiffPatch,
        desired_state: &PlaylistSnapshot,
    ) -> anyhow::Result<()>;

    /// Get playable URL for a track
    async fn playable_url(&self, track: &Track) -> anyhow::Result<String>;

    /// Fetch Tracks
    async fn fetch_track(&self, track_id: &str) -> anyhow::Result<Track>;
    async fn search_by_query(&self, query: &str) -> anyhow::Result<Vec<Track>>;

    // OAuth
    /// Generate OAuth authorization URL
    fn oauth_url(&self, redirect_uri: &str, state: &str) -> String;

    /// Exchange authorization code for tokens
    async fn exchange_code(&self, code: &str, redirect_uri: &str) -> anyhow::Result<OAuthToken>;

    /// Refresh an expired token
    async fn refresh_token(&self, token: &OAuthToken) -> anyhow::Result<OAuthToken>;

    /// Check if the authenticated user can modify the playlist
    async fn can_modify_playlist(&self, playlist_id: &str) -> anyhow::Result<bool>;
}
