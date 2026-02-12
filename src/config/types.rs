use crate::types::ProviderKind;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct WormholeConfig {
    #[serde(default)]
    pub proxy: ProxyConfig,

    #[serde(default)]
    pub failover: FailoverConfig,

    #[serde(default)]
    pub providers: ProviderConfigs,

    #[serde(default)]
    pub session: SessionConfig,
}

impl Default for WormholeConfig {
    fn default() -> Self {
        Self {
            proxy: ProxyConfig::default(),
            failover: FailoverConfig::default(),
            providers: ProviderConfigs::default(),
            session: SessionConfig::default(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ProxyConfig {
    #[serde(default = "default_host")]
    pub host: String,

    #[serde(default)]
    pub port: u16,

    #[serde(default)]
    pub default_provider: Option<ProviderKind>,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: 0,
            default_provider: None,
        }
    }
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct FailoverConfig {
    #[serde(default = "default_max_retries")]
    pub max_retries: usize,

    #[serde(default = "default_retry_on")]
    pub retry_on: Vec<u16>,

    #[serde(default = "default_backoff_base_ms")]
    pub backoff_base_ms: u64,

    #[serde(default)]
    pub fallback_order: Vec<ProviderKind>,
}

impl Default for FailoverConfig {
    fn default() -> Self {
        Self {
            max_retries: default_max_retries(),
            retry_on: default_retry_on(),
            backoff_base_ms: default_backoff_base_ms(),
            fallback_order: Vec::new(),
        }
    }
}

fn default_max_retries() -> usize {
    2
}
fn default_retry_on() -> Vec<u16> {
    vec![429, 500, 502, 503, 529]
}
fn default_backoff_base_ms() -> u64 {
    500
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ProviderConfigs {
    #[serde(default)]
    pub anthropic: Option<AnthropicConfig>,

    #[serde(default)]
    pub bedrock: Option<BedrockConfig>,

    #[serde(default)]
    pub vertex: Option<VertexConfig>,

    #[serde(default)]
    pub foundry: Option<FoundryConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnthropicConfig {
    pub api_key: Option<String>,
    pub api_key_env: Option<String>,
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BedrockConfig {
    pub region: Option<String>,
    pub profile: Option<String>,
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    pub use_global_endpoint: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VertexConfig {
    pub project_id: Option<String>,
    pub region: Option<String>,
    pub credentials_file: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FoundryConfig {
    pub resource: Option<String>,
    pub api_key: Option<String>,
    pub api_key_env: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SessionConfig {
    #[serde(default = "default_auto_cleanup_days")]
    pub auto_cleanup_days: u32,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            auto_cleanup_days: default_auto_cleanup_days(),
        }
    }
}

fn default_auto_cleanup_days() -> u32 {
    30
}

#[derive(Debug, Clone)]
pub enum ProviderCredentials {
    Anthropic {
        api_key: SecretString,
        base_url: Option<String>,
    },
    Bedrock {
        region: String,
        credentials: aws_credential_types::Credentials,
    },
    Vertex {
        project_id: String,
        region: String,
        token: SecretString,
    },
    Foundry {
        resource: String,
        api_key: SecretString,
    },
}

impl WormholeConfig {
    pub fn config_dir() -> PathBuf {
        dirs_path()
    }

    pub fn config_file_path() -> PathBuf {
        dirs_path().join("config.toml")
    }

    pub fn sessions_dir() -> PathBuf {
        dirs_path().join("sessions")
    }
}

fn dirs_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".wormhole")
}
