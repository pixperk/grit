mod cli;
mod playback;
mod provider;
mod state;
mod tui;
mod utils;

use clap::Parser;
use cli::{Cli, Commands};
use provider::ProviderKind;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env file if present (ignores if missing)
    let _ = dotenvy::dotenv();

    let cli = Cli::parse();
    let grit_dir = PathBuf::from(".grit");

    match cli.command {
        Commands::Auth { provider } => {
            cli::commands::auth::run(provider, &grit_dir).await?;
        }
        Commands::Init { playlist, provider } => {
            let provider = provider
                .or(cli.provider)
                .or_else(|| cli::commands::init::detect_provider(&playlist))
                .unwrap_or(ProviderKind::Spotify);
            cli::commands::init::run(provider, &playlist, &grit_dir).await?;
        }
        Commands::Search { query } => {
            cli::commands::staging::search(&query, cli.provider, &grit_dir).await?;
        }
        Commands::Add { track_id } => {
            cli::commands::staging::add(&track_id, cli.playlist.as_deref(), &grit_dir).await?;
        }
        Commands::Remove { track_id } => {
            cli::commands::staging::remove(&track_id, cli.playlist.as_deref(), &grit_dir).await?;
        }
        Commands::Move {
            track_id,
            new_index,
        } => {
            cli::commands::staging::move_track(
                &track_id,
                new_index,
                cli.playlist.as_deref(),
                &grit_dir,
            )
            .await?;
        }
        Commands::Status { playlist } => {
            cli::commands::staging::status(
                playlist.as_deref().or(cli.playlist.as_deref()),
                &grit_dir,
            )
            .await?;
        }
        Commands::Reset { playlist } => {
            cli::commands::staging::reset(
                playlist.as_deref().or(cli.playlist.as_deref()),
                &grit_dir,
            )
            .await?;
        }
        Commands::List { playlist } => {
            cli::commands::misc::list(playlist.as_deref().or(cli.playlist.as_deref()), &grit_dir)
                .await?;
        }
        Commands::Find { query, playlist } => {
            cli::commands::misc::find(
                &query,
                playlist.as_deref().or(cli.playlist.as_deref()),
                &grit_dir,
            )
            .await?;
        }
        Commands::Logout { provider } => {
            cli::commands::auth::logout(provider, &grit_dir).await?;
        }
        Commands::Whoami { provider } => {
            cli::commands::auth::whoami(provider, &grit_dir).await?;
        }
        Commands::Commit { message } => {
            cli::commands::staging::commit(&message, cli.playlist.as_deref(), &grit_dir).await?;
        }
        Commands::Push { playlist } => {
            cli::commands::vcs::push(playlist.as_deref().or(cli.playlist.as_deref()), &grit_dir)
                .await?;
        }
        Commands::Log => {
            cli::commands::vcs::log(cli.playlist.as_deref(), &grit_dir).await?;
        }
        Commands::Pull => {
            cli::commands::vcs::pull(cli.playlist.as_deref(), &grit_dir).await?;
        }
        Commands::Diff { staged, remote } => {
            cli::commands::vcs::diff_cmd(cli.playlist.as_deref(), &grit_dir, staged, remote)
                .await?;
        }
        Commands::Playlists { query } => {
            cli::commands::misc::playlists(query.as_deref(), &grit_dir).await?;
        }
        Commands::Revert { hash, playlist } => {
            cli::commands::vcs::revert(
                hash.as_deref(),
                playlist.as_deref().or(cli.playlist.as_deref()),
                &grit_dir,
            )
            .await?;
        }
        Commands::Apply { file } => {
            cli::commands::vcs::apply(&file, cli.playlist.as_deref(), &grit_dir).await?;
        }
        Commands::Play { playlist, shuffle } => {
            cli::commands::play::run(
                playlist.as_deref().or(cli.playlist.as_deref()),
                shuffle,
                &grit_dir,
            )
            .await?;
        }
    }

    Ok(())
}
