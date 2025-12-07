use async_trait::async_trait;
use crate::provider::{ DiffPatch, OAuthToken, PlaylistSnapshot, ProviderKind, Track};



#[async_trait]
pub trait Provider: Send + Sync {
    /// Returns the provider type
    fn kind(&self) -> ProviderKind;

    /// Fetch playlist snapshot from remote
    async fn fetch(&self, playlist_id: &str) -> anyhow::Result<PlaylistSnapshot>;
    
    /// Apply changes to remote playlist
    async fn apply(&self, playlist_id: &str, patch: &DiffPatch) -> anyhow::Result<()>;
    
    /// Get playable URL for a track
    async fn playable_url(&self, track: &Track) -> anyhow::Result<String>;

    /// Search for tracks by query
    async fn search_by_query(&self, query: &str) -> anyhow::Result<Vec<Track>>;

    // OAuth
    /// Generate OAuth authorization URL
    fn oauth_url(&self, redirect_uri: &str, state: &str) -> String;
    
    /// Exchange authorization code for tokens
    async fn exchange_code(&self, code: &str, redirect_uri: &str) -> anyhow::Result<OAuthToken>;
    
    /// Refresh an expired token
    async fn refresh_token(&self, token: &OAuthToken) -> anyhow::Result<OAuthToken>;
}