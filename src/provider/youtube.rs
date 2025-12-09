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
    grit_dir: Option<std::path::PathBuf>,
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
}

#[derive(Deserialize)]
struct YoutubePlaylistSnippet {
    title: String,
    description: Option<String>,
}

#[derive(Deserialize)]
struct YoutubePlaylistItemsResponse {
    items: Vec<YoutubePlaylistItem>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct YoutubePlaylistItem {
    id: String,
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
    snippet: YoutubeVideoSnippet,
    #[serde(rename = "contentDetails")]
    content_details: YoutubeVideoContentDetails,
}

#[derive(Deserialize)]
struct YoutubeVideoSnippet {
    title: String,
    #[serde(rename = "channelTitle")]
    channel_title: Option<String>,
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
            grit_dir: None,
            http: reqwest::Client::new(),
        }
    }

    pub fn with_token(mut self, token: &OAuthToken, grit_dir: &std::path::Path) -> Self {
        self.token = Mutex::new(Some(token.clone()));
        self.grit_dir = Some(grit_dir.to_path_buf());
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

            if let Some(grit_dir) = &self.grit_dir {
                use crate::state::credentials;
                credentials::save(grit_dir, ProviderKind::Youtube, &new_token)?;
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

    async fn fetch_playlist_item_ids(
        &self,
        playlist_id: &str,
        token: &str,
    ) -> Result<Vec<(String, String)>> {
        let mut items = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let mut url = format!(
                "{}/playlistItems?part=snippet,contentDetails&playlistId={}&maxResults=50",
                API_BASE, playlist_id
            );

            if let Some(token_str) = &page_token {
                url.push_str(&format!("&pageToken={}", token_str));
            }

            let resp: YoutubePlaylistItemsResponse = self.api_get(&url, token).await?;

            for item in resp.items {
                items.push((item.id, item.content_details.video_id));
            }

            page_token = resp.next_page_token;
            if page_token.is_none() {
                break;
            }
        }

        Ok(items)
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
    fn oauth_url(&self, redirect_uri: &str, state: &str) -> String {
        let scopes = "https://www.googleapis.com/auth/youtube.force-ssl";

        format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&access_type=offline&prompt=consent",
            AUTH_URL,
            urlencoding::encode(&self.client_id),
            urlencoding::encode(redirect_uri),
            urlencoding::encode(scopes),
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

        let playlist_resp: YoutubePlaylistResponse = self.api_get(&playlist_url, &token).await?;

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

            let items_resp: YoutubePlaylistItemsResponse = self.api_get(&items_url, &token).await?;

            let video_ids: Vec<String> = items_resp
                .items
                .iter()
                .map(|item| item.content_details.video_id.clone())
                .collect();

            if !video_ids.is_empty() {
                let videos_url = format!(
                    "{}/videos?part=snippet,contentDetails&id={}",
                    API_BASE,
                    video_ids.join(",")
                );

                let videos_resp: YoutubeVideoResponse = self.api_get(&videos_url, &token).await?;

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

    async fn apply(
        &self,
        playlist_id: &str,
        patch: &DiffPatch,
        desired_state: &PlaylistSnapshot,
    ) -> Result<()> {
        let token = self.get_token().await?;

        // Step 1: Remove tracks that shouldn't be there
        let playlist_items = self.fetch_playlist_item_ids(playlist_id, &token).await?;

        for change in &patch.changes {
            if let TrackChange::Removed { track, .. } = change {
                if let Some((item_id, _)) = playlist_items.iter().find(|(_, vid)| vid == &track.id)
                {
                    let url = format!("{}/playlistItems?id={}", API_BASE, item_id);

                    self.http
                        .delete(&url)
                        .header("Authorization", format!("Bearer {}", token))
                        .send()
                        .await?
                        .error_for_status()?;
                }
            }
        }

        // Step 2: Add new tracks to the END (we'll reorder later)
        for change in &patch.changes {
            if let TrackChange::Added { track, .. } = change {
                let body = serde_json::json!({
                    "snippet": {
                        "playlistId": playlist_id,
                        "resourceId": {
                            "kind": "youtube#video",
                            "videoId": track.id
                        }
                        // No position - adds to end
                    }
                });

                self.http
                    .post(format!("{}/playlistItems?part=snippet", API_BASE))
                    .header("Authorization", format!("Bearer {}", token))
                    .json(&body)
                    .send()
                    .await?
                    .error_for_status()?;
            }
        }

        // Step 3: Reorder playlist to match desired state
        // Process from the beginning, moving each track to its correct position
        for (desired_idx, desired_track) in desired_state.tracks.iter().enumerate() {
            // Fetch current state to find where this track is now and get its item_id
            let current = self.fetch(playlist_id).await?;
            let playlist_items = self.fetch_playlist_item_ids(playlist_id, &token).await?;

            let current_idx = current.tracks.iter().position(|t| t.id == desired_track.id);

            if let Some(current_idx) = current_idx {
                if current_idx != desired_idx {
                    // Find the item_id for this track
                    if let Some((item_id, _)) = playlist_items
                        .iter()
                        .find(|(_, vid)| vid == &desired_track.id)
                    {
                        let body = serde_json::json!({
                            "id": item_id,
                            "snippet": {
                                "playlistId": playlist_id,
                                "resourceId": {
                                    "kind": "youtube#video",
                                    "videoId": desired_track.id
                                },
                                "position": desired_idx
                            }
                        });

                        self.http
                            .put(format!("{}/playlistItems?part=snippet", API_BASE))
                            .header("Authorization", format!("Bearer {}", token))
                            .json(&body)
                            .send()
                            .await?
                            .error_for_status()?;
                    }
                }
            }
        }

        Ok(())
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
        }

        #[derive(Deserialize)]
        struct SearchId {
            #[serde(rename = "videoId")]
            video_id: String,
        }

        let resp: SearchResponse = self.api_get(&url, &token).await?;

        let video_ids: Vec<String> = resp
            .items
            .iter()
            .map(|item| item.id.video_id.clone())
            .collect();

        if video_ids.is_empty() {
            return Ok(Vec::new());
        }

        let videos_url = format!(
            "{}/videos?part=snippet,contentDetails&id={}",
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
                let artist = video
                    .snippet
                    .channel_title
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string());

                Track {
                    id: item.id.video_id.clone(),
                    name: video.snippet.title.clone(),
                    artists: vec![artist],
                    duration_ms,
                    provider: ProviderKind::Youtube,
                    metadata: None,
                }
            })
            .collect();

        Ok(tracks)
    }

    async fn fetch_track(&self, track_id: &str) -> Result<Track> {
        let token = self.get_token().await?;
        let url = format!(
            "{}/videos?part=snippet,contentDetails&id={}",
            API_BASE, track_id
        );

        let resp: YoutubeVideoResponse = self.api_get(&url, &token).await?;

        let video = resp.items.into_iter().next().context("Track not found")?;

        let duration_ms = Self::parse_iso8601_duration(&video.content_details.duration);
        let artist = video
            .snippet
            .channel_title
            .unwrap_or_else(|| "Unknown".to_string());

        Ok(Track {
            id: track_id.to_string(),
            name: video.snippet.title,
            artists: vec![artist],
            duration_ms,
            provider: ProviderKind::Youtube,
            metadata: None,
        })
    }

    async fn can_modify_playlist(&self, playlist_id: &str) -> Result<bool> {
        let token = self.get_token().await?;
        let url = format!("{}/playlists?part=snippet&id={}", API_BASE, playlist_id);

        match self.api_get::<YoutubePlaylistResponse>(&url, &token).await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
}
