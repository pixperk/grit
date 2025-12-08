mod cli;
mod provider;
mod state;
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
    let plr_dir = PathBuf::from(".plr");

    match cli.command {
        Commands::Auth { provider } => {
            cli::commands::auth::run(provider, &plr_dir).await?;
        }
        Commands::Init { playlist, provider } => {
            let provider = provider.or(cli.provider).unwrap_or(ProviderKind::Spotify);
            cli::commands::init::run(provider, &playlist, &plr_dir).await?;
        }
        Commands::Search { query } => {
            cli::commands::staging::search(&query, cli.provider, &plr_dir).await?;
        }
        Commands::Add { track_id } => {
            cli::commands::staging::add(&track_id, cli.playlist.as_deref(), &plr_dir).await?;
        }
        Commands::Remove { track_id } => {
            cli::commands::staging::remove(&track_id, cli.playlist.as_deref(), &plr_dir).await?;
        }
        Commands::Move {
            track_id,
            new_index,
        } => {
            cli::commands::staging::move_track(
                &track_id,
                new_index,
                cli.playlist.as_deref(),
                &plr_dir,
            )
            .await?;
        }
        Commands::Status { playlist } => {
            cli::commands::staging::status(
                playlist.as_deref().or(cli.playlist.as_deref()),
                &plr_dir,
            )
            .await?;
        }
        Commands::Reset { playlist } => {
            cli::commands::staging::reset(
                playlist.as_deref().or(cli.playlist.as_deref()),
                &plr_dir,
            )
            .await?;
        }
        Commands::List { playlist } => {
            cli::commands::misc::list(playlist.as_deref().or(cli.playlist.as_deref()), &plr_dir)
                .await?;
        }
        Commands::Find { query, playlist } => {
            cli::commands::misc::find(
                &query,
                playlist.as_deref().or(cli.playlist.as_deref()),
                &plr_dir,
            )
            .await?;
        }
        Commands::Logout { provider } => {
            cli::commands::auth::logout(provider, &plr_dir).await?;
        }
        Commands::Whoami { provider } => {
            cli::commands::auth::whoami(provider, &plr_dir).await?;
        }
        Commands::Commit { message } => {
            cli::commands::staging::commit(&message, cli.playlist.as_deref(), &plr_dir).await?;
        }
        Commands::Push { playlist } => {
            cli::commands::vcs::push(playlist.as_deref().or(cli.playlist.as_deref()), &plr_dir)
                .await?;
        }
        Commands::Log => {
            cli::commands::vcs::log(cli.playlist.as_deref(), &plr_dir).await?;
        }
        Commands::Pull => {
            cli::commands::vcs::pull(cli.playlist.as_deref(), &plr_dir).await?;
        }
        Commands::Diff { staged, remote } => {
            cli::commands::vcs::diff_cmd(cli.playlist.as_deref(), &plr_dir, staged, remote).await?;
        }
        Commands::Playlists { query } => {
            cli::commands::misc::playlists(query.as_deref(), &plr_dir).await?;
        }

        _ => {
            println!("{:?}", cli.command);
        }
    }

    Ok(())
}
