pub mod claude;
pub mod config_cmd;
pub mod serve;
pub mod sessions;

use clap::{Parser, Subcommand};

use crate::types::ProviderKind;

#[derive(Parser, Debug)]
#[command(name = "wormhole", about = "Multi-cloud provider proxy for Claude Code")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Enable verbose logging
    #[arg(long, global = true)]
    pub verbose: bool,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Start proxy and spawn Claude Code (wrapper mode)
    Claude(ClaudeArgs),

    /// Run a long-lived proxy server (daemon mode)
    Serve(ServeArgs),

    /// Manage sessions
    Sessions {
        #[command(subcommand)]
        command: SessionsCommand,
    },

    /// Manage configuration
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(clap::Args, Debug)]
pub struct ClaudeArgs {
    /// Provider to use
    #[arg(long)]
    pub provider: Option<ProviderKind>,

    /// Model to use
    #[arg(long)]
    pub model: Option<String>,

    /// Resume a previous session (optionally by ID prefix)
    #[arg(long)]
    pub resume: Option<Option<String>>,

    /// Port for the local proxy
    #[arg(long)]
    pub port: Option<u16>,

    /// Additional arguments to pass to claude CLI
    #[arg(last = true)]
    pub claude_args: Vec<String>,
}

#[derive(clap::Args, Debug)]
pub struct ServeArgs {
    /// Port to listen on
    #[arg(long, default_value = "8080")]
    pub port: u16,

    /// Host to bind to
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,
}

#[derive(Subcommand, Debug)]
pub enum SessionsCommand {
    /// List all sessions
    List,

    /// Delete a session
    Delete {
        /// Session ID (or prefix)
        id: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigCommand {
    /// Initialize a new configuration file
    Init,

    /// Show current configuration
    Show,

    /// Set credentials for a provider
    SetCredentials {
        /// Provider to configure
        provider: ProviderKind,

        /// API key (for providers that use one)
        #[arg(long)]
        api_key: Option<String>,
    },
}
