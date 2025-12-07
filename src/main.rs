mod cli;
mod provider;
mod state;

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
        _ => {
            println!("{:?}", cli.command);
        }
    }

    Ok(())
}
