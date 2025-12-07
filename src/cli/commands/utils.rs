use std::path::Path;

use anyhow::{Context, Result};

use crate::{
    provider::{Provider, ProviderKind, SpotifyProvider, YoutubeProvider},
    state::credentials,
};

pub fn create_provider(provider_kind: ProviderKind, plr_dir: &Path) -> Result<Box<dyn Provider>> {
    let token = credentials::load(plr_dir, provider_kind)?
        .context("No credentials found. Please run 'plr auth <provider>' first.")?;

    let provider: Box<dyn Provider> = match provider_kind {
        ProviderKind::Spotify => {
            let client_id =
                std::env::var("SPOTIFY_CLIENT_ID").context("SPOTIFY_CLIENT_ID not set")?;
            let client_secret =
                std::env::var("SPOTIFY_CLIENT_SECRET").context("SPOTIFY_CLIENT_SECRET not set")?;

            Box::new(SpotifyProvider::new(client_id, client_secret).with_token(&token, plr_dir))
        }
        ProviderKind::Youtube => {
            let client_id =
                std::env::var("YOUTUBE_CLIENT_ID").context("YOUTUBE_CLIENT_ID not set")?;
            let client_secret =
                std::env::var("YOUTUBE_CLIENT_SECRET").context("YOUTUBE_CLIENT_SECRET not set")?;

            Box::new(YoutubeProvider::new(client_id, client_secret).with_token(&token, plr_dir))
        }
    };
    Ok(provider)
}
