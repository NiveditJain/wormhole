use anyhow::Result;
use console::style;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::cli::ServeArgs;
use crate::config::credentials::resolve_credentials;
use crate::config::WormholeConfig;
use crate::provider::builder::build_provider;
use crate::server::state::AppState;

pub async fn run(args: ServeArgs, config: WormholeConfig) -> Result<()> {
    let shutdown = CancellationToken::new();

    // Build default provider if configured
    let default_provider = if let Some(default_kind) = config.proxy.default_provider {
        match resolve_credentials(default_kind, &config.providers).await {
            Ok(creds) => {
                let provider = build_provider(default_kind, creds)?;
                Some(provider)
            }
            Err(e) => {
                tracing::warn!(
                    "Default provider {} not available: {}",
                    default_kind,
                    e
                );
                None
            }
        }
    } else {
        None
    };

    let mut state = AppState::new(config.clone(), shutdown.clone());
    if let Some(provider) = default_provider {
        state = state.with_default_provider(provider);
    }
    let state = Arc::new(state);

    let (host, port) = crate::server::start_server(
        state,
        &args.host,
        args.port,
        shutdown.clone(),
    )
    .await?;

    // Print daemon banner
    let divider = style("─".repeat(50)).dim();
    println!("{}", divider);
    println!(
        "  {} Wormhole daemon running",
        style("●").green().bold()
    );
    println!(
        "  {} http://{}:{}",
        style("Listening:").bold(),
        host,
        port
    );
    if let Some(default_kind) = config.proxy.default_provider {
        println!(
            "  {} {}",
            style("Default provider:").bold(),
            style(default_kind.display_name()).cyan()
        );
    } else {
        println!(
            "  {} {}",
            style("Default provider:").bold(),
            style("none (create sessions via API)").dim()
        );
    }
    println!("{}", divider);
    println!();
    println!(
        "  Create sessions: POST http://{}:{}/v1/sessions",
        host, port
    );
    println!(
        "  Connect Claude:  ANTHROPIC_BASE_URL=http://{}:{} claude",
        host, port
    );
    println!();

    // Wait for shutdown signal
    info!("Press Ctrl+C to stop");
    tokio::signal::ctrl_c().await?;

    println!();
    println!("{} Shutting down...", style("●").yellow().bold());
    shutdown.cancel();

    // Give in-flight requests time to complete
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    println!("{} Goodbye!", style("●").green().bold());
    Ok(())
}

