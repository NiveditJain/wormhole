use anyhow::{bail, Context, Result};
use aws_credential_types::provider::ProvideCredentials;
use secrecy::SecretString;
use tracing::debug;

use crate::config::types::*;
use crate::types::ProviderKind;

pub async fn resolve_credentials(
    provider: ProviderKind,
    config: &ProviderConfigs,
) -> Result<ProviderCredentials> {
    match provider {
        ProviderKind::Anthropic => resolve_anthropic(config.anthropic.as_ref()).await,
        ProviderKind::Bedrock => resolve_bedrock(config.bedrock.as_ref()).await,
        ProviderKind::Vertex => resolve_vertex(config.vertex.as_ref()).await,
        ProviderKind::Foundry => resolve_foundry(config.foundry.as_ref()).await,
    }
}

async fn resolve_anthropic(config: Option<&AnthropicConfig>) -> Result<ProviderCredentials> {
    // 1. Try config file api_key
    if let Some(cfg) = config {
        if let Some(ref key) = cfg.api_key {
            debug!("Using Anthropic API key from config file");
            return Ok(ProviderCredentials::Anthropic {
                api_key: SecretString::from(key.clone()),
                base_url: cfg.base_url.clone(),
            });
        }
        // 2. Try env var reference
        if let Some(ref env_name) = cfg.api_key_env {
            if let Ok(key) = std::env::var(env_name) {
                debug!("Using Anthropic API key from env var {}", env_name);
                return Ok(ProviderCredentials::Anthropic {
                    api_key: SecretString::from(key),
                    base_url: cfg.base_url.clone(),
                });
            }
        }
    }

    // 3. Try standard env var
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        debug!("Using Anthropic API key from ANTHROPIC_API_KEY env var");
        return Ok(ProviderCredentials::Anthropic {
            api_key: SecretString::from(key),
            base_url: config.and_then(|c| c.base_url.clone()),
        });
    }

    bail!(
        "No Anthropic API key found. Tried:\n\
         1. config file providers.anthropic.api_key\n\
         2. config file providers.anthropic.api_key_env reference\n\
         3. ANTHROPIC_API_KEY environment variable\n\
         \n\
         Run `wormhole config set-credentials anthropic` to configure."
    )
}

async fn resolve_bedrock(config: Option<&BedrockConfig>) -> Result<ProviderCredentials> {
    let region = config
        .and_then(|c| c.region.clone())
        .or_else(|| std::env::var("AWS_REGION").ok())
        .or_else(|| std::env::var("AWS_DEFAULT_REGION").ok())
        .unwrap_or_else(|| "us-east-1".to_string());

    let mut aws_config_loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_types::region::Region::new(region.clone()));

    if let Some(cfg) = config {
        if let Some(ref profile) = cfg.profile {
            aws_config_loader = aws_config_loader.profile_name(profile);
        }
    }

    let aws_cfg = aws_config_loader.load().await;
    let credentials_provider = aws_cfg
        .credentials_provider()
        .context("No AWS credentials found. Tried AWS config chain (env vars, ~/.aws/credentials, IAM role).\nRun `wormhole config set-credentials bedrock` to configure.")?;

    let credentials: aws_credential_types::Credentials = credentials_provider
        .provide_credentials()
        .await
        .context("Failed to resolve AWS credentials")?;

    debug!("Resolved AWS credentials for region {}", region);

    Ok(ProviderCredentials::Bedrock {
        region,
        credentials,
    })
}

async fn resolve_vertex(config: Option<&VertexConfig>) -> Result<ProviderCredentials> {
    let project_id = config
        .and_then(|c| c.project_id.clone())
        .or_else(|| std::env::var("GOOGLE_CLOUD_PROJECT").ok())
        .or_else(|| std::env::var("GCLOUD_PROJECT").ok())
        .context(
            "No GCP project ID found. Set providers.vertex.project_id in config \
             or GOOGLE_CLOUD_PROJECT env var.",
        )?;

    let region = config
        .and_then(|c| c.region.clone())
        .or_else(|| std::env::var("GOOGLE_CLOUD_REGION").ok())
        .unwrap_or_else(|| "us-east5".to_string());

    // Set credentials file if specified in config
    if let Some(cfg) = config {
        if let Some(ref creds_file) = cfg.credentials_file {
            std::env::set_var("GOOGLE_APPLICATION_CREDENTIALS", creds_file);
        }
    }

    let auth_manager = gcp_auth::provider().await.context(
        "Failed to initialize GCP authentication. Ensure Application Default Credentials are configured.\n\
         Run `gcloud auth application-default login` or set GOOGLE_APPLICATION_CREDENTIALS.",
    )?;

    let token = auth_manager
        .token(&["https://www.googleapis.com/auth/cloud-platform"])
        .await
        .context("Failed to get GCP OAuth token")?;

    let token_str = token
        .as_str()
        .to_string();

    debug!(
        "Resolved GCP credentials for project {} region {}",
        project_id, region
    );

    Ok(ProviderCredentials::Vertex {
        project_id,
        region,
        token: SecretString::from(token_str),
    })
}

async fn resolve_foundry(config: Option<&FoundryConfig>) -> Result<ProviderCredentials> {
    let resource = config
        .and_then(|c| c.resource.clone())
        .or_else(|| std::env::var("AZURE_FOUNDRY_RESOURCE").ok())
        .context(
            "No Azure Foundry resource name found. Set providers.foundry.resource in config \
             or AZURE_FOUNDRY_RESOURCE env var.",
        )?;

    // 1. Try config file api_key
    if let Some(cfg) = config {
        if let Some(ref key) = cfg.api_key {
            debug!("Using Azure Foundry API key from config file");
            return Ok(ProviderCredentials::Foundry {
                resource,
                api_key: SecretString::from(key.clone()),
            });
        }
        // 2. Try env var reference
        if let Some(ref env_name) = cfg.api_key_env {
            if let Ok(key) = std::env::var(env_name) {
                debug!("Using Azure Foundry API key from env var {}", env_name);
                return Ok(ProviderCredentials::Foundry {
                    resource,
                    api_key: SecretString::from(key),
                });
            }
        }
    }

    // 3. Try standard env var
    if let Ok(key) = std::env::var("AZURE_FOUNDRY_API_KEY") {
        debug!("Using Azure Foundry API key from AZURE_FOUNDRY_API_KEY env var");
        return Ok(ProviderCredentials::Foundry {
            resource,
            api_key: SecretString::from(key),
        });
    }

    bail!(
        "No Azure Foundry API key found. Tried:\n\
         1. config file providers.foundry.api_key\n\
         2. config file providers.foundry.api_key_env reference\n\
         3. AZURE_FOUNDRY_API_KEY environment variable\n\
         \n\
         Run `wormhole config set-credentials foundry` to configure."
    )
}
