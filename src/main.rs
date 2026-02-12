mod auth;
mod cli;
mod config;
mod error;
mod provider;
mod server;
mod session;
mod types;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use cli::{Cli, Command, ConfigCommand, SessionsCommand};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let filter = if cli.verbose {
        EnvFilter::new("wormhole=debug,tower_http=debug")
    } else {
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("wormhole=info,tower_http=info"))
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    let config = config::load_config();

    match cli.command {
        Command::Claude(args) => {
            cli::claude::run(args, config).await?;
        }
        Command::Serve(args) => {
            cli::serve::run(args, config).await?;
        }
        Command::Sessions { command } => match command {
            SessionsCommand::List => {
                cli::sessions::run_list()?;
            }
            SessionsCommand::Delete { id } => {
                cli::sessions::run_delete(&id)?;
            }
        },
        Command::Config { command } => match command {
            ConfigCommand::Init => {
                cli::config_cmd::run_init()?;
            }
            ConfigCommand::Show => {
                cli::config_cmd::run_show()?;
            }
            ConfigCommand::SetCredentials { provider, api_key } => {
                cli::config_cmd::run_set_credentials(provider, api_key)?;
            }
        },
        Command::Add(args) => {
            cli::config_cmd::run_add(args)?;
        }
    }

    Ok(())
}
