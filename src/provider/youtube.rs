use crate::provider::{
    DiffPatch, OAuthToken, PlaylistSnapshot, Provider, ProviderKind, Track, TrackChange,
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use tokio::sync::Mutex;

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const API_BASE: &str = "https://www.googleapis.com/youtube/v3";

pub struct YoutubeProvider {
    client_id: String,
    client_secret: String,
    token: Mutex<Option<OAuthToken>>,
    plr_dir: Option<std::path::PathBuf>,
    http: reqwest::Client,
}

#[derive(Deserialize)]
struct YoutubeTokenResponse {
    access_token: String,
    token_type: String,
    expires_in: u64,
    refresh_token: Option<String>,
    scope: Option<String>,
}

#[derive(Deserialize)]
struct YoutubePlaylistResponse {
    items: Vec<YoutubePlaylist>,
}

#[derive(Deserialize)]
struct YoutubePlaylist {
    id: String,
    snippet: YoutubePlaylistSnippet,
    #[serde(rename = "contentDetails")]
    content_details: Option<YoutubeContentDetails>,
}

#[derive(Deserialize)]
struct YoutubePlaylistSnippet {
    title: String,
    description: Option<String>,
}

#[derive(Deserialize)]
struct YoutubeContentDetails {
    #[serde(rename = "itemCount")]
    item_count: Option<u64>,
}

#[derive(Deserialize)]
struct YoutubePlaylistItemsResponse {
    items: Vec<YoutubePlaylistItem>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct YoutubePlaylistItem {
    snippet: YoutubeItemSnippet,
    #[serde(rename = "contentDetails")]
    content_details: YoutubeItemContentDetails,
}

#[derive(Deserialize)]
struct YoutubeItemSnippet {
    title: String,
    #[serde(rename = "videoOwnerChannelTitle")]
    video_owner_channel_title: Option<String>,
}

#[derive(Deserialize)]
struct YoutubeItemContentDetails {
    #[serde(rename = "videoId")]
    video_id: String,
}

#[derive(Deserialize)]
struct YoutubeVideoResponse {
    items: Vec<YoutubeVideo>,
}

#[derive(Deserialize)]
struct YoutubeVideo {
    id: String,
    #[serde(rename = "contentDetails")]
    content_details: YoutubeVideoContentDetails,
}

#[derive(Deserialize)]
struct YoutubeVideoContentDetails {
    duration: String,
}

impl YoutubeTokenResponse {
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

impl YoutubeProvider {
    pub fn new(client_id: String, client_secret: String) -> Self {
        Self {
            client_id,
            client_secret,
            token: Mutex::new(None),
            plr_dir: None,
            http: reqwest::Client::new(),
        }
    }

    pub fn with_token(mut self, token: &OAuthToken, plr_dir: &std::path::Path) -> Self {
        *self.token.blocking_lock() = Some(token.clone());
        self.plr_dir = Some(plr_dir.to_path_buf());
        self
    }

    fn is_token_expired(token: &OAuthToken) -> bool {
        if let Some(expires_at) = token.expires_at {
            use std::time::{SystemTime, UNIX_EPOCH};
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            return now >= expires_at.saturating_sub(60);
        }
        false
    }

    async fn get_token(&self) -> Result<String> {
        let token_guard = self.token.lock().await;
        let current_token = token_guard
            .as_ref()
            .context("Not authenticated with YouTube")?
            .clone();
        drop(token_guard);

        if Self::is_token_expired(&current_token) {
            println!("Token expired, refreshing...");
            let new_token = self.refresh_token(&current_token).await?;

            if let Some(plr_dir) = &self.plr_dir {
                use crate::state::credentials;
                credentials::save(plr_dir, ProviderKind::Youtube, &new_token)?;
            }

            *self.token.lock().await = Some(new_token.clone());
            Ok(new_token.access_token)
        } else {
            Ok(current_token.access_token)
        }
    }

    async fn token_request(&self, params: &[(&str, &str)]) -> Result<YoutubeTokenResponse> {
        let response = self
            .http
            .post(TOKEN_URL)
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

    async fn api_get<T: serde::de::DeserializeOwned>(&self, url: &str, token: &str) -> Result<T> {
        let response = self
            .http
            .get(url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .context("Failed to send API request")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("YouTube API error {}: {}", status, error_text);
        }

        response
            .json()
            .await
            .context("Failed to parse API response")
    }

    fn parse_iso8601_duration(duration: &str) -> u64 {
        // Parse ISO 8601 duration format (PT1H2M3S) to milliseconds
        let duration = duration.trim_start_matches("PT");
        let mut total_ms = 0u64;
        let mut num = String::new();

        for ch in duration.chars() {
            if ch.is_ascii_digit() {
                num.push(ch);
            } else {
                if let Ok(value) = num.parse::<u64>() {
                    total_ms += match ch {
                        'H' => value * 3600 * 1000,
                        'M' => value * 60 * 1000,
                        'S' => value * 1000,
                        _ => 0,
                    };
                }
                num.clear();
            }
        }

        total_ms
    }
}

#[async_trait]
impl Provider for YoutubeProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Youtube
    }

    fn oauth_url(&self, redirect_uri: &str, state: &str) -> String {
        let scopes = [
            "https://www.googleapis.com/auth/youtube.readonly",
            "https://www.googleapis.com/auth/youtube",
        ]
        .join(" ");

        format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&access_type=offline&prompt=consent",
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
            ("client_id", &self.client_id),
            ("client_secret", &self.client_secret),
        ];

        self.token_request(&params)
            .await
            .map(|r| r.into_oauth_token())
    }

    async fn refresh_token(&self, token: &OAuthToken) -> Result<OAuthToken> {
        let refresh = token
            .refresh_token
            .as_ref()
            .context("No refresh token available")?;

        let params = [
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh.as_str()),
            ("client_id", &self.client_id),
            ("client_secret", &self.client_secret),
        ];

        let mut new_token = self.token_request(&params).await?.into_oauth_token();

        if new_token.refresh_token.is_none() {
            new_token.refresh_token = token.refresh_token.clone();
        }

        Ok(new_token)
    }

    async fn fetch(&self, playlist_id: &str) -> Result<PlaylistSnapshot> {
        let token = self.get_token().await?;

        let playlist_url = format!(
            "{}/playlists?part=snippet,contentDetails&id={}&key={}",
            API_BASE, playlist_id, self.client_id
        );

        let playlist_resp: YoutubePlaylistResponse =
            self.api_get(&playlist_url, &token).await?;

        let playlist = playlist_resp
            .items
            .into_iter()
            .next()
            .context("Playlist not found")?;

        let mut all_tracks = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let mut items_url = format!(
                "{}/playlistItems?part=snippet,contentDetails&playlistId={}&maxResults=50",
                API_BASE, playlist_id
            );

            if let Some(token) = &page_token {
                items_url.push_str(&format!("&pageToken={}", token));
            }

            let items_resp: YoutubePlaylistItemsResponse =
                self.api_get(&items_url, &token).await?;

            let video_ids: Vec<String> = items_resp
                .items
                .iter()
                .map(|item| item.content_details.video_id.clone())
                .collect();

            if !video_ids.is_empty() {
                let videos_url = format!(
                    "{}/videos?part=contentDetails&id={}",
                    API_BASE,
                    video_ids.join(",")
                );

                let videos_resp: YoutubeVideoResponse =
                    self.api_get(&videos_url, &token).await?;

                for (item, video) in items_resp.items.iter().zip(videos_resp.items.iter()) {
                    let duration_ms = Self::parse_iso8601_duration(&video.content_details.duration);
                    let artist = item
                        .snippet
                        .video_owner_channel_title
                        .clone()
                        .unwrap_or_else(|| "Unknown".to_string());

                    all_tracks.push(Track {
                        id: item.content_details.video_id.clone(),
                        name: item.snippet.title.clone(),
                        artists: vec![artist],
                        duration_ms,
                        provider: ProviderKind::Youtube,
                        metadata: None,
                    });
                }
            }

            page_token = items_resp.next_page_token;
            if page_token.is_none() {
                break;
            }
        }

        Ok(PlaylistSnapshot {
            id: playlist.id.clone(),
            name: playlist.snippet.title,
            description: playlist.snippet.description,
            tracks: all_tracks,
            provider: ProviderKind::Youtube,
            snapshot_hash: format!("yt-{}", playlist.id),
            metadata: None,
        })
    }

    async fn apply(&self, _playlist_id: &str, _patch: &DiffPatch) -> Result<()> {
        anyhow::bail!("YouTube playlist modification not yet implemented")
    }

    async fn playable_url(&self, track: &Track) -> Result<String> {
        Ok(format!("https://www.youtube.com/watch?v={}", track.id))
    }

    async fn search_by_query(&self, query: &str) -> Result<Vec<Track>> {
        let token = self.get_token().await?;
        let url = format!(
            "{}/search?part=snippet&q={}&type=video&maxResults=10",
            API_BASE,
            urlencoding::encode(query)
        );

        #[derive(Deserialize)]
        struct SearchResponse {
            items: Vec<SearchItem>,
        }

        #[derive(Deserialize)]
        struct SearchItem {
            id: SearchId,
            snippet: YoutubeItemSnippet,
        }

        #[derive(Deserialize)]
        struct SearchId {
            #[serde(rename = "videoId")]
            video_id: String,
        }

        let resp: SearchResponse = self.api_get(&url, &token).await?;

        let video_ids: Vec<String> = resp.items.iter().map(|item| item.id.video_id.clone()).collect();

        if video_ids.is_empty() {
            return Ok(Vec::new());
        }

        let videos_url = format!(
            "{}/videos?part=contentDetails&id={}",
            API_BASE,
            video_ids.join(",")
        );

        let videos_resp: YoutubeVideoResponse = self.api_get(&videos_url, &token).await?;

        let tracks = resp
            .items
            .iter()
            .zip(videos_resp.items.iter())
            .map(|(item, video)| {
                let duration_ms = Self::parse_iso8601_duration(&video.content_details.duration);
                let artist = item
                    .snippet
                    .video_owner_channel_title
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string());

                Track {
                    id: item.id.video_id.clone(),
                    name: item.snippet.title.clone(),
                    artists: vec![artist],
                    duration_ms,
                    provider: ProviderKind::Youtube,
                    metadata: None,
                }
            })
            .collect();

        Ok(tracks)
    }
}