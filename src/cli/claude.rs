use anyhow::{bail, Context, Result};
use console::style;
use dialoguer::FuzzySelect;
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

use crate::cli::ClaudeArgs;
use crate::config::credentials::resolve_credentials;
use crate::config::types::ProviderCredentials;
use crate::config::WormholeConfig;
use crate::provider::anthropic::AnthropicProvider;
use crate::provider::bedrock::BedrockProvider;
use crate::provider::failover::FailoverProvider;
use crate::provider::foundry::FoundryProvider;
use crate::provider::vertex::VertexProvider;
use crate::provider::Provider;
use crate::server::state::AppState;
use crate::session::{SessionState, SessionStore};
use crate::types::ProviderKind;

pub async fn run(args: ClaudeArgs, config: WormholeConfig) -> Result<()> {
    let session_store = SessionStore::new()?;

    // Clean up old sessions
    let cleanup_days = config.session.auto_cleanup_days;
    if let Ok(cleaned) = session_store.cleanup_old(cleanup_days as i64) {
        if cleaned > 0 {
            debug!("Cleaned up {} old sessions", cleaned);
        }
    }

    // Resolve provider
    let (provider_kind, resumed_session) = resolve_provider_and_session(&args, &config, &session_store)?;

    // Resolve credentials
    let credentials = resolve_credentials(provider_kind, &config.providers)
        .await
        .context(format!(
            "Failed to resolve credentials for {}",
            provider_kind.display_name()
        ))?;

    // Build provider
    let provider: Arc<dyn Provider> = build_provider(provider_kind, credentials)?;

    // Wrap with failover if configured
    let provider = maybe_wrap_failover(provider, &config).await;

    // Create or resume session
    let session = if let Some(s) = resumed_session {
        session_store.touch(&s.id).ok();
        s
    } else {
        let session = SessionState::new(
            provider_kind,
            args.model.clone(),
            None,
            config.failover.fallback_order.clone(),
        );
        session_store.create(&session)?;
        session
    };

    // Start proxy server
    let shutdown = CancellationToken::new();
    let state = Arc::new(
        AppState::new(config.clone(), shutdown.clone()).with_default_provider(provider),
    );

    let host = "127.0.0.1";
    let port = args.port.unwrap_or(0);
    let (_host, actual_port) = crate::server::start_server(state, host, port, shutdown.clone())
        .await
        .context("Failed to start proxy server")?;

    // Print banner
    print_banner(provider_kind, &session, actual_port, args.model.as_deref());

    // Spawn Claude Code
    let base_url = format!("http://127.0.0.1:{}", actual_port);
    let exit_code = spawn_claude(&base_url, &args.claude_args).await?;

    // Save session and shut down
    session_store.touch(&session.id).ok();
    shutdown.cancel();

    // Give the server a moment to finish in-flight requests
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    debug!("Session {} saved, proxy shut down", session.short_id());

    if exit_code != 0 {
        std::process::exit(exit_code);
    }

    Ok(())
}

fn resolve_provider_and_session(
    args: &ClaudeArgs,
    config: &WormholeConfig,
    store: &SessionStore,
) -> Result<(ProviderKind, Option<SessionState>)> {
    // If --resume is specified, try to resume a session
    if let Some(ref resume_arg) = args.resume {
        let session = match resume_arg {
            Some(id) => store.get(id).map_err(|e| anyhow::anyhow!("{}", e))?,
            None => store
                .most_recent()?
                .ok_or_else(|| anyhow::anyhow!("No sessions to resume"))?,
        };

        println!(
            "{} Resuming session {} ({})",
            style("↻").cyan().bold(),
            style(session.short_id()).yellow(),
            session.provider.display_name()
        );

        return Ok((session.provider, Some(session)));
    }

    // If --provider is specified, use it
    if let Some(provider) = args.provider {
        return Ok((provider, None));
    }

    // Try default provider from config
    if let Some(default) = config.proxy.default_provider {
        return Ok((default, None));
    }

    // Interactive selection
    let kinds = ProviderKind::all();
    let names: Vec<&str> = kinds.iter().map(|p| p.display_name()).collect();

    let selection = FuzzySelect::new()
        .with_prompt("Select provider")
        .items(&names)
        .default(0)
        .interact()
        .context("Provider selection cancelled")?;

    Ok((kinds[selection], None))
}

fn build_provider(
    kind: ProviderKind,
    credentials: ProviderCredentials,
) -> Result<Arc<dyn Provider>> {
    match (kind, credentials) {
        (ProviderKind::Anthropic, ProviderCredentials::Anthropic { api_key, base_url }) => {
            Ok(Arc::new(AnthropicProvider::new(api_key, base_url)))
        }
        (ProviderKind::Bedrock, ProviderCredentials::Bedrock { region, credentials }) => {
            Ok(Arc::new(BedrockProvider::new(credentials, region)))
        }
        (ProviderKind::Vertex, ProviderCredentials::Vertex { project_id, region, token }) => {
            Ok(Arc::new(VertexProvider::new(project_id, region, token)))
        }
        (ProviderKind::Foundry, ProviderCredentials::Foundry { resource, api_key }) => {
            Ok(Arc::new(FoundryProvider::new(resource, api_key)))
        }
        _ => bail!("Credential type mismatch"),
    }
}

async fn maybe_wrap_failover(
    primary: Arc<dyn Provider>,
    config: &WormholeConfig,
) -> Arc<dyn Provider> {
    if config.failover.fallback_order.is_empty() {
        return primary;
    }

    let mut fallbacks = Vec::new();
    for kind in &config.failover.fallback_order {
        if *kind == primary.kind() {
            continue; // Skip if same as primary
        }
        match resolve_credentials(*kind, &config.providers).await {
            Ok(creds) => match build_provider(*kind, creds) {
                Ok(provider) => fallbacks.push(provider),
                Err(e) => {
                    tracing::warn!("Skipping fallback {}: {}", kind, e);
                }
            },
            Err(e) => {
                tracing::warn!("Skipping fallback {} (no credentials): {}", kind, e);
            }
        }
    }

    if fallbacks.is_empty() {
        return primary;
    }

    Arc::new(FailoverProvider::new(
        primary,
        fallbacks,
        config.failover.max_retries,
        config.failover.retry_on.clone(),
        config.failover.backoff_base_ms,
    ))
}

fn print_banner(
    provider: ProviderKind,
    session: &SessionState,
    port: u16,
    model: Option<&str>,
) {
    let divider = style("─".repeat(50)).dim();
    println!("{}", divider);
    println!(
        "  {} {}",
        style("Provider:").bold(),
        style(provider.display_name()).cyan()
    );
    if let Some(model) = model.or(session.model.as_deref()) {
        println!("  {} {}", style("Model:").bold(), style(model).cyan());
    }
    println!(
        "  {} {}",
        style("Session:").bold(),
        style(session.short_id()).yellow()
    );
    println!(
        "  {} http://127.0.0.1:{}",
        style("Proxy:").bold(),
        port
    );
    println!("{}", divider);
    println!();
}

async fn spawn_claude(base_url: &str, extra_args: &[String]) -> Result<i32> {
    info!("Spawning claude with ANTHROPIC_BASE_URL={}", base_url);

    let mut cmd = Command::new("claude");
    cmd.env("ANTHROPIC_BASE_URL", base_url)
        .env("ANTHROPIC_API_KEY", "wormhole-proxy")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    for arg in extra_args {
        cmd.arg(arg);
    }

    let status = cmd
        .status()
        .await
        .context("Failed to spawn claude. Is Claude Code installed?")?;

    Ok(status.code().unwrap_or(1))
}
