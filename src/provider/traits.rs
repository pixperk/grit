use async_trait::async_trait;
use crate::provider::{ PlaylistSnapshot, DiffPatch, Track, OAuthToken};



#[async_trait]
pub trait Provider : Send + Sync {
    async fn fetch(&self, playlist_id : &str) -> anyhow::Result<PlaylistSnapshot>;
    async fn apply(&self, playlist_id : &str, patch : &DiffPatch) -> anyhow::Result<()>;
    async fn playable_url(&self, track : &Track ) -> anyhow::Result<String>;

    // OAuth related methods
    fn oauth_url(&self, redirect_uri : &str, state : &str) -> String;
    async fn exchange_code(&self, code : &str, redirect_uri : &str) -> anyhow::Result<OAuthToken>;

    //fetch_single_track
    async fn search_by_query(&self, query : &str) -> anyhow::Result<Vec<Track>>;
}