use anyhow::{bail, Context, Result};
use console::style;
use dialoguer::{FuzzySelect, Select};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

use crate::cli::config_cmd::gather_and_save_credentials;
use crate::cli::ClaudeArgs;
use crate::config::credentials::resolve_credentials;
use crate::config::types::ProviderCredentials;
use crate::config::{load_config, WormholeConfig};
use crate::provider::anthropic::AnthropicProvider;
use crate::provider::bedrock::BedrockProvider;
use crate::provider::failover::FailoverProvider;
use crate::provider::foundry::FoundryProvider;
use crate::provider::model_map::available_models;
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

    // Resolve provider and model (and whether we're resuming a previous session)
    let (provider_kind, resumed_session, selected_model) =
        resolve_provider_and_session(&args, &config, &session_store)?;

    // Use explicitly provided --model, or the interactively selected model
    let model = args.model.clone().or(selected_model);

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
    print_banner(provider_kind, actual_port, model.as_deref());

    // Spawn Claude Code, passing --resume if we're resuming a previous session
    let base_url = format!("http://127.0.0.1:{}", actual_port);
    let resume_id = resumed_session.as_ref().map(|s| s.id.as_str());
    let exit_code = spawn_claude(&base_url, &args.claude_args, resume_id).await?;

    // After Claude exits, capture its session ID and save wormhole session metadata
    if let Some(claude_session_id) = find_claude_session_id() {
        if resumed_session
            .as_ref()
            .map_or(true, |s| s.id != claude_session_id)
        {
            // New session — save provider/model metadata keyed by Claude's session ID
            let session = SessionState::with_id(
                claude_session_id,
                provider_kind,
                model.clone(),
                None,
                config.failover.fallback_order.clone(),
            );
            session_store.create(&session)?;
            debug!("Session {} saved", session.short_id());
        } else {
            // Resumed session — just update last_active
            session_store.touch(&claude_session_id).ok();
            debug!("Session {} touched", &claude_session_id[..8]);
        }
    } else {
        debug!("Could not detect Claude session ID");
    }

    shutdown.cancel();

    // Give the server a moment to finish in-flight requests
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    if exit_code != 0 {
        std::process::exit(exit_code);
    }

    Ok(())
}

fn resolve_provider_and_session(
    args: &ClaudeArgs,
    config: &WormholeConfig,
    store: &SessionStore,
) -> Result<(ProviderKind, Option<SessionState>, Option<String>)> {
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

        return Ok((session.provider, Some(session), None));
    }

    // If --provider and --model are both specified, skip all interactive selection
    if let Some(provider) = args.provider {
        if let Some(ref model) = args.model {
            return Ok((provider, None, Some(model.clone())));
        }
        // --provider without --model: skip provider picker, show model picker
        let model = interactive_model_select(provider)?;
        return Ok((provider, None, model));
    }

    // Try default provider from config
    if let Some(default) = config.proxy.default_provider {
        if let Some(ref model) = args.model {
            return Ok((default, None, Some(model.clone())));
        }
        let model = interactive_model_select(default)?;
        return Ok((default, None, model));
    }

    // Full interactive flow: provider → model
    let (provider, model) = interactive_provider_and_model_select(config)?;
    Ok((provider, None, model))
}

/// Check whether a provider has credentials configured (in config or env).
fn is_provider_configured(kind: ProviderKind, config: &WormholeConfig) -> bool {
    match kind {
        ProviderKind::Anthropic => {
            config
                .providers
                .anthropic
                .as_ref()
                .map_or(false, |c| c.api_key.is_some() || c.api_key_env.is_some())
                || std::env::var("ANTHROPIC_API_KEY").is_ok()
        }
        ProviderKind::Bedrock => {
            config.providers.bedrock.is_some()
                || std::env::var("AWS_REGION").is_ok()
                || std::env::var("AWS_DEFAULT_REGION").is_ok()
                || std::path::Path::new(&format!(
                    "{}/.aws/credentials",
                    std::env::var("HOME").unwrap_or_default()
                ))
                .exists()
        }
        ProviderKind::Vertex => {
            config
                .providers
                .vertex
                .as_ref()
                .map_or(false, |c| c.project_id.is_some())
                || std::env::var("GOOGLE_CLOUD_PROJECT").is_ok()
        }
        ProviderKind::Foundry => {
            config
                .providers
                .foundry
                .as_ref()
                .map_or(false, |c| c.api_key.is_some() || c.api_key_env.is_some())
                || std::env::var("AZURE_FOUNDRY_API_KEY").is_ok()
        }
    }
}

/// Full interactive flow: select a provider, then select a model.
fn interactive_provider_and_model_select(
    config: &WormholeConfig,
) -> Result<(ProviderKind, Option<String>)> {
    let provider = interactive_provider_select(config)?;
    let model = interactive_model_select(provider)?;
    Ok((provider, model))
}

/// Show an interactive provider picker with configured/unconfigured status
/// and an "Configure a new provider..." option.
fn interactive_provider_select(config: &WormholeConfig) -> Result<ProviderKind> {
    let mut current_config = config.clone();

    loop {
        let kinds = ProviderKind::all();

        // Build display items
        let mut items: Vec<String> = Vec::new();
        let mut configured_kinds: Vec<Option<ProviderKind>> = Vec::new();

        for &kind in kinds {
            let configured = is_provider_configured(kind, &current_config);
            let label = if configured {
                format!("{} {}", style("✓").green().bold(), kind.display_name())
            } else {
                format!(
                    "  {} {}",
                    kind.display_name(),
                    style("(not configured)").dim()
                )
            };
            items.push(label);
            configured_kinds.push(Some(kind));
        }

        // Separator
        items.push(format!("{}", style("──────────────────────────────").dim()));
        configured_kinds.push(None); // separator marker

        // "Configure a new provider..." option
        items.push(format!(
            "{} Configure a new provider...",
            style("+").cyan().bold()
        ));
        configured_kinds.push(None); // "add new" marker

        let separator_idx = kinds.len();
        let add_new_idx = kinds.len() + 1;

        let selection = Select::new()
            .with_prompt("Select provider")
            .items(&items)
            .default(0)
            .interact()
            .context("Provider selection cancelled")?;

        // Separator is not a valid selection — re-prompt
        if selection == separator_idx {
            continue;
        }

        // "Configure a new provider..." selected
        if selection == add_new_idx {
            let provider_names: Vec<&str> = kinds.iter().map(|p| p.display_name()).collect();
            let provider_idx = Select::new()
                .with_prompt("Which provider to configure")
                .items(&provider_names)
                .default(0)
                .interact()
                .context("Provider type selection cancelled")?;

            let chosen = kinds[provider_idx];
            println!();
            gather_and_save_credentials(chosen, None)?;
            println!();

            // Reload config after saving credentials
            current_config = load_config();
            continue;
        }

        // A provider was selected
        let chosen = kinds[selection];
        let configured = is_provider_configured(chosen, &current_config);

        if !configured {
            // Run inline credential setup
            println!();
            println!(
                "{} {} is not configured. Let's set it up.",
                style("!").yellow().bold(),
                chosen.display_name()
            );
            println!();
            gather_and_save_credentials(chosen, None)?;
            println!();

            // Reload config and restart selection
            current_config = load_config();
            continue;
        }

        return Ok(chosen);
    }
}

/// Show an interactive model picker for the given provider using FuzzySelect.
fn interactive_model_select(provider: ProviderKind) -> Result<Option<String>> {
    let models = available_models(provider);

    if models.is_empty() {
        return Ok(None);
    }

    let display_names: Vec<&str> = models.iter().map(|(_, name)| *name).collect();

    let selection = FuzzySelect::new()
        .with_prompt(format!("Select model ({})", provider.display_name()))
        .items(&display_names)
        .default(0)
        .interact()
        .context("Model selection cancelled")?;

    Ok(Some(models[selection].0.to_string()))
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

fn print_banner(provider: ProviderKind, port: u16, model: Option<&str>) {
    let divider = style("─".repeat(50)).dim();
    println!("{}", divider);
    println!(
        "  {} {}",
        style("Provider:").bold(),
        style(provider.display_name()).cyan()
    );
    if let Some(model) = model {
        println!("  {} {}", style("Model:").bold(), style(model).cyan());
    }
    println!(
        "  {} http://127.0.0.1:{}",
        style("Proxy:").bold(),
        port
    );
    println!("{}", divider);
    println!();
}

async fn spawn_claude(
    base_url: &str,
    extra_args: &[String],
    resume_session_id: Option<&str>,
) -> Result<i32> {
    info!("Spawning claude with ANTHROPIC_BASE_URL={}", base_url);

    let mut cmd = Command::new("claude");
    cmd.env("ANTHROPIC_BASE_URL", base_url)
        .env("ANTHROPIC_API_KEY", "wormhole-proxy")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    // Forward --resume to Claude Code so it resumes its own session
    if let Some(session_id) = resume_session_id {
        cmd.arg("--resume").arg(session_id);
    }

    for arg in extra_args {
        cmd.arg(arg);
    }

    let status = cmd
        .status()
        .await
        .context("Failed to spawn claude. Is Claude Code installed?")?;

    Ok(status.code().unwrap_or(1))
}

/// Scan Claude Code's session directory for the most recently modified session
/// file and return its ID (the filename stem, a UUID).
///
/// Claude stores sessions at `~/.claude/projects/<encoded-cwd>/<session-id>.jsonl`.
fn find_claude_session_id() -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let cwd = std::env::current_dir().ok()?;

    // Claude encodes the cwd by replacing '/' with '-'
    let encoded_cwd = cwd.to_string_lossy().replace('/', "-");
    let sessions_dir = PathBuf::from(&home)
        .join(".claude")
        .join("projects")
        .join(&encoded_cwd);

    let mut newest: Option<(String, std::time::SystemTime)> = None;

    for entry in std::fs::read_dir(&sessions_dir).ok()? {
        let entry = entry.ok()?;
        let path = entry.path();

        // Only consider top-level .jsonl files (not subagent files in subdirs)
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }

        if let Ok(meta) = entry.metadata() {
            if let Ok(modified) = meta.modified() {
                let name = path.file_stem()?.to_string_lossy().to_string();
                if newest.as_ref().map_or(true, |(_, t)| modified > *t) {
                    newest = Some((name, modified));
                }
            }
        }
    }

    let (id, _) = newest?;
    debug!("Detected Claude session ID: {}", id);
    Some(id)
}
