use anyhow::{Context, Result};
use console::style;
use dialoguer::{Input, Password, Select};
use std::fs;
use std::os::unix::fs::PermissionsExt;

use crate::config::{load_config, WormholeConfig};
use crate::types::ProviderKind;

pub fn run_init() -> Result<()> {
    let config_dir = WormholeConfig::config_dir();
    let config_path = WormholeConfig::config_file_path();
    let sessions_dir = WormholeConfig::sessions_dir();

    if config_path.exists() {
        println!(
            "{} Config file already exists at {}",
            style("!").yellow().bold(),
            config_path.display()
        );

        let overwrite = Select::new()
            .with_prompt("Overwrite?")
            .items(&["No", "Yes"])
            .default(0)
            .interact()?;

        if overwrite == 0 {
            println!("Keeping existing config.");
            return Ok(());
        }
    }

    // Create directories
    fs::create_dir_all(&config_dir).context("Failed to create config directory")?;
    fs::create_dir_all(&sessions_dir).context("Failed to create sessions directory")?;

    // Select default provider
    let provider_names: Vec<&str> = ProviderKind::all().iter().map(|p| p.display_name()).collect();
    let provider_idx = Select::new()
        .with_prompt("Default provider")
        .items(&provider_names)
        .default(0)
        .interact()?;
    let default_provider = ProviderKind::all()[provider_idx];

    let config_content = format!(
        r#"[proxy]
host = "127.0.0.1"
port = 0
default_provider = "{default_provider}"

[failover]
max_retries = 2
retry_on = [429, 500, 502, 503, 529]
backoff_base_ms = 500
fallback_order = []

[session]
auto_cleanup_days = 30

# Uncomment and configure the providers you want to use:

# [providers.anthropic]
# api_key = "sk-ant-..."
# api_key_env = "ANTHROPIC_API_KEY"

# [providers.bedrock]
# region = "us-east-1"
# profile = "default"

# [providers.vertex]
# project_id = "my-gcp-project"
# region = "us-east5"

# [providers.foundry]
# resource = "my-azure-resource"
# api_key_env = "AZURE_FOUNDRY_API_KEY"
"#
    );

    fs::write(&config_path, &config_content).context("Failed to write config file")?;

    // Set permissions to 600
    let mut perms = fs::metadata(&config_path)?.permissions();
    perms.set_mode(0o600);
    fs::set_permissions(&config_path, perms)?;

    println!(
        "{} Config created at {}",
        style("✓").green().bold(),
        config_path.display()
    );
    println!(
        "  Run {} to set up provider credentials.",
        style("wormhole config set-credentials <provider>").cyan()
    );

    Ok(())
}

pub fn run_show() -> Result<()> {
    let config = load_config();
    let config_path = WormholeConfig::config_file_path();

    println!(
        "{} Wormhole Configuration",
        style("●").cyan().bold()
    );
    println!(
        "  Config file: {}",
        if config_path.exists() {
            style(config_path.display().to_string()).green()
        } else {
            style("not found (using defaults)".to_string()).yellow()
        }
    );
    println!();

    println!("{}", style("[proxy]").bold());
    println!("  host = \"{}\"", config.proxy.host);
    println!("  port = {}", config.proxy.port);
    println!(
        "  default_provider = {}",
        config
            .proxy
            .default_provider
            .map(|p| format!("\"{}\"", p))
            .unwrap_or_else(|| "none".to_string())
    );
    println!();

    println!("{}", style("[failover]").bold());
    println!("  max_retries = {}", config.failover.max_retries);
    println!("  retry_on = {:?}", config.failover.retry_on);
    println!("  backoff_base_ms = {}", config.failover.backoff_base_ms);
    println!(
        "  fallback_order = {:?}",
        config
            .failover
            .fallback_order
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
    );
    println!();

    println!("{}", style("[providers]").bold());

    if let Some(ref a) = config.providers.anthropic {
        println!(
            "  anthropic: api_key={}",
            if a.api_key.is_some() {
                style("****").green().to_string()
            } else if a.api_key_env.is_some() {
                style(format!("env:{}", a.api_key_env.as_ref().unwrap())).yellow().to_string()
            } else {
                style("not set").red().to_string()
            }
        );
    } else {
        println!("  anthropic: {}", style("not configured").dim());
    }

    if let Some(ref b) = config.providers.bedrock {
        println!(
            "  bedrock: region={}, profile={}",
            b.region.as_deref().unwrap_or("default"),
            b.profile.as_deref().unwrap_or("default")
        );
    } else {
        println!("  bedrock: {}", style("not configured").dim());
    }

    if let Some(ref v) = config.providers.vertex {
        println!(
            "  vertex: project={}, region={}",
            v.project_id.as_deref().unwrap_or("not set"),
            v.region.as_deref().unwrap_or("us-east5")
        );
    } else {
        println!("  vertex: {}", style("not configured").dim());
    }

    if let Some(ref f) = config.providers.foundry {
        println!(
            "  foundry: resource={}, api_key={}",
            f.resource.as_deref().unwrap_or("not set"),
            if f.api_key.is_some() {
                style("****").green().to_string()
            } else if f.api_key_env.is_some() {
                style(format!("env:{}", f.api_key_env.as_ref().unwrap())).yellow().to_string()
            } else {
                style("not set").red().to_string()
            }
        );
    } else {
        println!("  foundry: {}", style("not configured").dim());
    }

    // Check config file permissions
    if config_path.exists() {
        if let Ok(meta) = fs::metadata(&config_path) {
            let mode = meta.permissions().mode();
            if mode & 0o077 != 0 {
                println!();
                println!(
                    "{} Config file is readable by others (mode {:o}). Run: chmod 600 {}",
                    style("⚠").yellow().bold(),
                    mode & 0o777,
                    config_path.display()
                );
            }
        }
    }

    Ok(())
}

pub fn run_set_credentials(provider: ProviderKind, api_key: Option<String>) -> Result<()> {
    gather_and_save_credentials(provider, api_key)
}

/// Interactively gather credentials for the given provider and save them to the
/// config file. This is used by both `wormhole config set-credentials` and the
/// inline "add provider" flow in `wormhole claude`.
pub fn gather_and_save_credentials(provider: ProviderKind, api_key: Option<String>) -> Result<()> {
    let config_path = WormholeConfig::config_file_path();
    let config_dir = WormholeConfig::config_dir();

    fs::create_dir_all(&config_dir)?;

    // Read existing config or start fresh
    let mut config_str = if config_path.exists() {
        fs::read_to_string(&config_path)?
    } else {
        String::new()
    };

    let section = match provider {
        ProviderKind::Anthropic => {
            let key = if let Some(key) = api_key {
                key
            } else {
                Password::new()
                    .with_prompt("Anthropic API key")
                    .interact()?
            };
            format!(
                "\n[providers.anthropic]\napi_key = \"{}\"\n",
                key
            )
        }
        ProviderKind::Bedrock => {
            let region: String = Input::new()
                .with_prompt("AWS region")
                .default("us-east-1".to_string())
                .interact_text()?;
            let profile: String = Input::new()
                .with_prompt("AWS profile")
                .default("default".to_string())
                .interact_text()?;
            format!(
                "\n[providers.bedrock]\nregion = \"{}\"\nprofile = \"{}\"\n",
                region, profile
            )
        }
        ProviderKind::Vertex => {
            let project_id: String = Input::new()
                .with_prompt("GCP project ID")
                .interact_text()?;
            let region: String = Input::new()
                .with_prompt("GCP region")
                .default("us-east5".to_string())
                .interact_text()?;
            format!(
                "\n[providers.vertex]\nproject_id = \"{}\"\nregion = \"{}\"\n",
                project_id, region
            )
        }
        ProviderKind::Foundry => {
            let resource: String = Input::new()
                .with_prompt("Azure Foundry resource name")
                .interact_text()?;
            let key = if let Some(key) = api_key {
                key
            } else {
                Password::new()
                    .with_prompt("Azure Foundry API key")
                    .interact()?
            };
            format!(
                "\n[providers.foundry]\nresource = \"{}\"\napi_key = \"{}\"\n",
                resource, key
            )
        }
    };

    // Remove existing section for this provider if present
    let section_header = format!("[providers.{}]", provider);
    if let Some(start) = config_str.find(&section_header) {
        // Find next section or end of file
        let rest = &config_str[start + section_header.len()..];
        let end = rest
            .find("\n[")
            .map(|i| start + section_header.len() + i)
            .unwrap_or(config_str.len());
        config_str.replace_range(start..end, "");
    }

    config_str.push_str(&section);

    fs::write(&config_path, &config_str)?;

    // Set permissions to 600
    let mut perms = fs::metadata(&config_path)?.permissions();
    perms.set_mode(0o600);
    fs::set_permissions(&config_path, perms)?;

    println!(
        "{} Credentials saved for {}",
        style("✓").green().bold(),
        style(provider.display_name()).cyan()
    );

    Ok(())
}
