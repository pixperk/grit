use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::provider::{OAuthToken, ProviderKind};
use crate::state::credentials;

const API_BASE: &str = "https://api.spotify.com/v1";
const TOKEN_URL: &str = "https://accounts.spotify.com/api/token";

/// Spotify Connect playback controller
/// Controls playback on any Spotify Connect device (librespot, phone, desktop app)
pub struct SpotifyPlayer {
    http: reqwest::Client,
    token: Mutex<OAuthToken>,
    client_id: String,
    client_secret: String,
    grit_dir: PathBuf,
    device_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DevicesResponse {
    devices: Vec<Device>,
}

#[derive(Debug, Deserialize)]
struct Device {
    id: Option<String>,
    name: String,
    is_active: bool,
}

#[derive(Debug, Serialize)]
struct PlayRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    uris: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    offset: Option<PlayOffset>,
}

#[derive(Debug, Serialize)]
struct PlayOffset {
    position: usize,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    token_type: String,
    expires_in: u64,
    refresh_token: Option<String>,
    scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CurrentlyPlaying {
    item: Option<PlayingItem>,
    #[allow(dead_code)]
    is_playing: bool,
}

#[derive(Debug, Deserialize)]
struct PlayingItem {
    name: String,
    artists: Vec<PlayingArtist>,
}

#[derive(Debug, Deserialize)]
struct PlayingArtist {
    name: String,
}

#[derive(Debug, Deserialize)]
struct SpotifyError {
    error: SpotifyErrorDetails,
}

#[derive(Debug, Deserialize)]
struct SpotifyErrorDetails {
    #[allow(dead_code)]
    status: u16,
    message: String,
}

/// Parse Spotify API error response into a clean message
fn parse_spotify_error(text: &str) -> String {
    if let Ok(err) = serde_json::from_str::<SpotifyError>(text) {
        err.error.message
    } else {
        text.trim().to_string()
    }
}

impl SpotifyPlayer {
    pub fn new(
        token: OAuthToken,
        client_id: String,
        client_secret: String,
        grit_dir: &Path,
    ) -> Self {
        Self {
            http: reqwest::Client::new(),
            token: Mutex::new(token),
            client_id,
            client_secret,
            grit_dir: grit_dir.to_path_buf(),
            device_id: None,
        }
    }

    /// Check if token is expired (with 60 second buffer)
    fn is_token_expired(token: &OAuthToken) -> bool {
        if let Some(expires_at) = token.expires_at {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            return now >= expires_at.saturating_sub(60);
        }
        false
    }

    /// Get access token, refreshing if expired
    async fn get_token(&self) -> Result<String> {
        let current_token = self.token.lock().await.clone();

        if Self::is_token_expired(&current_token) {
            let new_token = self.refresh_token(&current_token).await?;

            // Save refreshed token
            credentials::save(&self.grit_dir, ProviderKind::Spotify, &new_token)?;

            *self.token.lock().await = new_token.clone();
            Ok(new_token.access_token)
        } else {
            Ok(current_token.access_token)
        }
    }

    /// Refresh the OAuth token
    async fn refresh_token(&self, token: &OAuthToken) -> Result<OAuthToken> {
        let refresh = token
            .refresh_token
            .as_ref()
            .context("No refresh token available")?;

        use base64::Engine;
        let credentials = format!("{}:{}", self.client_id, self.client_secret);
        let basic_auth = base64::engine::general_purpose::STANDARD.encode(credentials);

        let params = [
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh.as_str()),
        ];

        let response = self
            .http
            .post(TOKEN_URL)
            .header("Authorization", format!("Basic {}", basic_auth))
            .form(&params)
            .send()
            .await
            .context("Failed to refresh token")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            bail!("Token refresh failed: {}", error_text);
        }

        let resp: TokenResponse = response.json().await?;

        let expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + resp.expires_in;

        Ok(OAuthToken {
            access_token: resp.access_token,
            refresh_token: resp.refresh_token.or(token.refresh_token.clone()),
            expires_at: Some(expires_at),
            token_type: resp.token_type,
            scope: resp.scope,
        })
    }

    /// Get available Spotify Connect devices
    pub async fn get_devices(&self) -> Result<Vec<(String, String, bool)>> {
        let token = self.get_token().await?;

        let response = self
            .http
            .get(format!("{}/me/player/devices", API_BASE))
            .bearer_auth(&token)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            bail!("Failed to get devices ({}): {}", status, text);
        }

        let resp: DevicesResponse = response.json().await?;

        Ok(resp
            .devices
            .into_iter()
            .filter_map(|d| d.id.map(|id| (id, d.name, d.is_active)))
            .collect())
    }

    /// Select a device for playback
    pub async fn select_device(&mut self) -> Result<()> {
        let devices = self.get_devices().await?;

        if devices.is_empty() {
            bail!(
                "No Spotify devices found.\n\n\
                 Start one of these:\n  \
                 - Spotify desktop app\n  \
                 - Spotify mobile app\n  \
                 - librespot: librespot -n 'grit' -b 320\n"
            );
        }

        // Prefer active device, otherwise first one
        let device = devices
            .iter()
            .find(|(_, _, active)| *active)
            .or(devices.first())
            .unwrap();

        println!("Using Spotify device: {}", device.1);
        self.device_id = Some(device.0.clone());
        Ok(())
    }

    /// Start playback with a list of track URIs
    pub async fn play(&self, uris: Vec<String>, offset: usize) -> Result<()> {
        let token = self.get_token().await?;
        let device_id = self.device_id.as_ref().context("No device selected")?;

        let body = PlayRequest {
            uris: Some(uris),
            offset: Some(PlayOffset { position: offset }),
        };

        let resp = self
            .http
            .put(format!(
                "{}/me/player/play?device_id={}",
                API_BASE, device_id
            ))
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            bail!("{}", parse_spotify_error(&text));
        }

        Ok(())
    }

    /// Pause playback
    pub async fn pause(&self) -> Result<()> {
        let token = self.get_token().await?;
        let device_id = self.device_id.as_ref().context("No device selected")?;

        let resp = self
            .http
            .put(format!(
                "{}/me/player/pause?device_id={}",
                API_BASE, device_id
            ))
            .bearer_auth(&token)
            .header("Content-Length", "0")
            .send()
            .await?;

        // 403 = already paused, ignore
        if !resp.status().is_success() && resp.status().as_u16() != 403 {
            let text = resp.text().await.unwrap_or_default();
            bail!("{}", parse_spotify_error(&text));
        }
        Ok(())
    }

    /// Resume playback
    pub async fn resume(&self) -> Result<()> {
        let token = self.get_token().await?;
        let device_id = self.device_id.as_ref().context("No device selected")?;

        let resp = self
            .http
            .put(format!(
                "{}/me/player/play?device_id={}",
                API_BASE, device_id
            ))
            .bearer_auth(&token)
            .header("Content-Length", "0")
            .send()
            .await?;

        if !resp.status().is_success() && resp.status().as_u16() != 403 {
            let text = resp.text().await.unwrap_or_default();
            bail!("{}", parse_spotify_error(&text));
        }
        Ok(())
    }

    /// Skip to next track
    pub async fn next(&self) -> Result<()> {
        let token = self.get_token().await?;
        let device_id = self.device_id.as_ref().context("No device selected")?;

        let resp = self
            .http
            .post(format!(
                "{}/me/player/next?device_id={}",
                API_BASE, device_id
            ))
            .bearer_auth(&token)
            .header("Content-Length", "0")
            .send()
            .await?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            bail!("{}", parse_spotify_error(&text));
        }
        Ok(())
    }

    /// Skip to previous track
    pub async fn previous(&self) -> Result<()> {
        let token = self.get_token().await?;
        let device_id = self.device_id.as_ref().context("No device selected")?;

        let resp = self
            .http
            .post(format!(
                "{}/me/player/previous?device_id={}",
                API_BASE, device_id
            ))
            .bearer_auth(&token)
            .header("Content-Length", "0")
            .send()
            .await?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            bail!("{}", parse_spotify_error(&text));
        }
        Ok(())
    }

    /// Seek to position in seconds
    pub async fn seek(&self, position_secs: u64) -> Result<()> {
        let token = self.get_token().await?;
        let device_id = self.device_id.as_ref().context("No device selected")?;
        let position_ms = position_secs * 1000;

        let resp = self
            .http
            .put(format!(
                "{}/me/player/seek?device_id={}&position_ms={}",
                API_BASE, device_id, position_ms
            ))
            .bearer_auth(&token)
            .header("Content-Length", "0")
            .send()
            .await?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            bail!("{}", parse_spotify_error(&text));
        }
        Ok(())
    }

    /// Toggle shuffle
    pub async fn set_shuffle(&self, state: bool) -> Result<()> {
        let token = self.get_token().await?;
        let device_id = self.device_id.as_ref().context("No device selected")?;

        let resp = self
            .http
            .put(format!(
                "{}/me/player/shuffle?device_id={}&state={}",
                API_BASE, device_id, state
            ))
            .bearer_auth(&token)
            .header("Content-Length", "0")
            .send()
            .await?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            bail!("{}", parse_spotify_error(&text));
        }
        Ok(())
    }

    /// Set repeat mode
    pub async fn set_repeat(&self, mode: crate::playback::events::RepeatMode) -> Result<()> {
        let token = self.get_token().await?;
        let device_id = self.device_id.as_ref().context("No device selected")?;

        let state = match mode {
            crate::playback::events::RepeatMode::None => "off",
            crate::playback::events::RepeatMode::All => "context",
            crate::playback::events::RepeatMode::One => "track",
        };

        let resp = self
            .http
            .put(format!(
                "{}/me/player/repeat?device_id={}&state={}",
                API_BASE, device_id, state
            ))
            .bearer_auth(&token)
            .header("Content-Length", "0")
            .send()
            .await?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            bail!("{}", parse_spotify_error(&text));
        }
        Ok(())
    }

    /// Get currently playing track info
    pub async fn get_currently_playing(&self) -> Result<Option<(String, String)>> {
        let token = self.get_token().await?;

        let resp = self
            .http
            .get(format!("{}/me/player/currently-playing", API_BASE))
            .bearer_auth(&token)
            .send()
            .await?;

        // 204 = nothing playing
        if resp.status().as_u16() == 204 {
            return Ok(None);
        }

        if !resp.status().is_success() {
            return Ok(None);
        }

        let playing: CurrentlyPlaying = resp.json().await?;

        if let Some(item) = playing.item {
            let artists = item
                .artists
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            Ok(Some((item.name, artists)))
        } else {
            Ok(None)
        }
    }
}
