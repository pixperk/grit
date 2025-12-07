use crate::provider::{DiffPatch, OAuthToken, PlaylistSnapshot, Provider, ProviderKind, Track};
use serde::Deserialize;
use anyhow::{Context, Result};
use async_trait::async_trait;

const AUTH_URL: &str = "https://accounts.spotify.com/authorize";
const TOKEN_URL: &str = "https://accounts.spotify.com/api/token";
const API_BASE: &str = "https://api.spotify.com/v1";

pub struct SpotifyProvider {
    client_id: String,
    client_secret: String,
    http: reqwest::Client,
}

#[derive(Deserialize)]
struct SpotifyTokenResponse {
    access_token: String,
    token_type: String,
    expires_in: u64,
    refresh_token: Option<String>,
    scope: Option<String>,
}

impl SpotifyTokenResponse {
    fn into_oauth_token(self) -> OAuthToken {
        use std::time::{SystemTime, UNIX_EPOCH};

        let expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + self.expires_in;

        OAuthToken {
            access_token: self.access_token,
            refresh_token: self.refresh_token,
            expires_at: Some(expires_at),
            token_type: self.token_type,
            scope: self.scope,
        }
    }
}

impl SpotifyProvider {
    pub fn new(client_id: String, client_secret: String) -> Self {
        Self {
            client_id,
            client_secret,
            http: reqwest::Client::new(),
        }
    }

    fn basic_auth_header(&self) -> String {
        use base64::Engine;
        let credentials = format!("{}:{}", self.client_id, self.client_secret);
        base64::engine::general_purpose::STANDARD.encode(credentials)
    }

    async fn token_request(&self, params: &[(&str, &str)]) -> Result<SpotifyTokenResponse> {
        let response = self.http
            .post(TOKEN_URL)
            .header("Authorization", format!("Basic {}", self.basic_auth_header()))
            .form(params)
            .send()
            .await
            .context("Failed to send token request")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Token request failed: {}", error_text);
        }

        response
            .json()
            .await
            .context("Failed to parse token response")
    }
}

#[async_trait]
impl Provider for SpotifyProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Spotify
    }

    fn oauth_url(&self, redirect_uri: &str, state: &str) -> String {
        let scopes = [
            "playlist-read-private",
            "playlist-read-collaborative",
            "playlist-modify-public",
            "playlist-modify-private",
        ]
        .join(" ");

        format!(
            "{}?client_id={}&response_type=code&redirect_uri={}&scope={}&state={}",
            AUTH_URL,
            urlencoding::encode(&self.client_id),
            urlencoding::encode(redirect_uri),
            urlencoding::encode(&scopes),
            urlencoding::encode(state),
        )
    }

    async fn exchange_code(&self, code: &str, redirect_uri: &str) -> Result<OAuthToken> {
        let params = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
        ];

        self.token_request(&params)
            .await
            .map(|r| r.into_oauth_token())
    }

    async fn refresh_token(&self, token: &OAuthToken) -> Result<OAuthToken> {
        let refresh = token.refresh_token.as_ref()
            .context("No refresh token available")?;

        let params = [
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh.as_str()),
        ];

        let mut new_token = self.token_request(&params)
            .await?
            .into_oauth_token();

        // Spotify doesn't always return a new refresh_token
        if new_token.refresh_token.is_none() {
            new_token.refresh_token = token.refresh_token.clone();
        }

        Ok(new_token)
    }

    async fn fetch(&self, _playlist_id: &str) -> Result<PlaylistSnapshot> {
        todo!("Implement fetch")
    }

    async fn apply(&self, _playlist_id: &str, _patch: &DiffPatch) -> Result<()> {
        todo!("Implement apply")
    }

    async fn playable_url(&self, track: &Track) -> Result<String> {
        // Spotify URI format for librespot
        Ok(format!("spotify:track:{}", track.id))
    }

    async fn search_by_query(&self, _query: &str) -> Result<Vec<Track>> {
        todo!("Implement search")
    }

}
