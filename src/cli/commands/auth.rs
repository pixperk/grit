use crate::provider::{Provider, ProviderKind, SpotifyProvider, YoutubeProvider};
use crate::state::credentials;
use anyhow::{Context, Result};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::Path;

const REDIRECT_URI: &str = "http://127.0.0.1:8888/callback";

/// Run the authentication flow for the given provider.
pub async fn run(provider: ProviderKind, plr_dir: &Path) -> Result<()> {
    match provider {
        ProviderKind::Spotify => auth_spotify(plr_dir).await,
        ProviderKind::Youtube => auth_youtube(plr_dir).await,
    }
}

async fn auth_spotify(plr_dir: &Path) -> Result<()> {
    let client_id =
        std::env::var("SPOTIFY_CLIENT_ID").context("Set SPOTIFY_CLIENT_ID environment variable")?;
    let client_secret = std::env::var("SPOTIFY_CLIENT_SECRET")
        .context("Set SPOTIFY_CLIENT_SECRET environment variable")?;

    let provider = SpotifyProvider::new(client_id, client_secret);

    let state = format!("{:016x}", rand::random::<u64>());
    let auth_url = provider.oauth_url(REDIRECT_URI, &state);

    println!("Opening browser for Spotify authorization...\n");
    println!("If it doesn't open, visit:\n{}\n", auth_url);

    let _ = open::that(auth_url.clone());

    let code = wait_for_callback(&state)?;

    println!("Exchanging code for token...");
    let token = provider.exchange_code(&code, REDIRECT_URI).await?;

    credentials::save(plr_dir, ProviderKind::Spotify, &token)?;

    println!("\nSuccessfully authenticated with Spotify!");
    println!(
        "  Token saved to {:?}",
        plr_dir.join("credentials/spotify.json")
    );

    Ok(())
}

async fn auth_youtube(plr_dir: &Path) -> Result<()> {
    let client_id =
        std::env::var("YOUTUBE_CLIENT_ID").context("Set YOUTUBE_CLIENT_ID environment variable")?;
    let client_secret = std::env::var("YOUTUBE_CLIENT_SECRET")
        .context("Set YOUTUBE_CLIENT_SECRET environment variable")?;

    let provider = YoutubeProvider::new(client_id, client_secret);

    let state = format!("{:016x}", rand::random::<u64>());
    let auth_url = provider.oauth_url(REDIRECT_URI, &state);

    println!("Opening browser for YouTube authorization...\n");
    println!("If it doesn't open, visit:\n{}\n", auth_url);

    let _ = open::that(auth_url.clone());

    let code = wait_for_callback(&state)?;

    println!("Exchanging code for token...");
    let token = provider.exchange_code(&code, REDIRECT_URI).await?;

    credentials::save(plr_dir, ProviderKind::Youtube, &token)?;

    println!("\nSuccessfully authenticated with YouTube!");
    println!(
        "  Token saved to {:?}",
        plr_dir.join("credentials/youtube.json")
    );

    Ok(())
}

fn wait_for_callback(expected_state: &str) -> Result<String> {
    let listener = TcpListener::bind("127.0.0.1:8888")
        .context("Failed to bind to port 8888. Is another instance running?")?;

    println!("Waiting for callback...");

    for stream in listener.incoming() {
        let mut stream = stream?;
        let mut reader = BufReader::new(&stream);
        let mut request_line = String::new();
        reader.read_line(&mut request_line)?;

        // Parse GET /callback?code=xxx&state=yyy HTTP/1.1
        if let Some(path) = request_line.split_whitespace().nth(1) {
            if path.starts_with("/callback?") {
                let query = path.trim_start_matches("/callback?");
                let params: std::collections::HashMap<_, _> =
                    query.split('&').filter_map(|p| p.split_once('=')).collect();

                if params.get("state") != Some(&expected_state) {
                    send_response(&mut stream, "400", "State mismatch - possible CSRF")?;
                    continue;
                }

                if let Some(&code) = params.get("code") {
                    send_response(
                        &mut stream,
                        "200",
                        "<html><body><h1>Success!</h1><p>You can close this tab.</p></body></html>",
                    )?;
                    return Ok(code.to_string());
                }

                if let Some(&error) = params.get("error") {
                    send_response(&mut stream, "400", &format!("Auth failed: {}", error))?;
                    anyhow::bail!("Authorization denied: {}", error);
                }
            }
        }

        send_response(&mut stream, "404", "Not Found")?;
    }

    anyhow::bail!("No valid callback received")
}

fn send_response(stream: &mut impl Write, status: &str, body: &str) -> Result<()> {
    let response = format!(
        "HTTP/1.1 {} OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        body.len(),
        body
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}

pub async fn logout(provider: ProviderKind, plr_dir: &Path) -> Result<()> {
    // Check if credentials exist
    let token = credentials::load(plr_dir, provider)?;

    if token.is_none() {
        println!("Not logged in to {:?}", provider);
        return Ok(());
    }

    // Delete credentials
    credentials::delete(plr_dir, provider)?;

    println!("Logged out from {:?}", provider);
    println!("Run 'plr auth {:?}' to login again", provider);

    Ok(())
}

pub async fn whoami(provider: ProviderKind, plr_dir: &Path) -> Result<()> {
    let token = credentials::load(plr_dir, provider)?
        .context("Not authenticated. Run 'plr auth <provider>' first")?;

    match provider {
        ProviderKind::Spotify => {
            println!("Logged in to Spotify");
            println!("Token type: {}", token.token_type);
            if let Some(scope) = &token.scope {
                println!("Scopes: {}", scope);
            }
            if let Some(expires_at) = token.expires_at {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                if now < expires_at {
                    let remaining = expires_at - now;
                    println!("Token expires in: {}s", remaining);
                } else {
                    println!("Token expired (will auto-refresh on next use)");
                }
            }
        }
        ProviderKind::Youtube => {
            println!("Logged in to YouTube");
            println!("Token type: {}", token.token_type);
            if let Some(scope) = &token.scope {
                println!("Scopes: {}", scope);
            }
            if let Some(expires_at) = token.expires_at {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                if now < expires_at {
                    let remaining = expires_at - now;
                    println!("Token expires in: {}s", remaining);
                } else {
                    println!("Token expired (will auto-refresh on next use)");
                }
            }
        }
    }

    Ok(())
}
