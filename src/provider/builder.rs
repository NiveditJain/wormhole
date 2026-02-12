use std::sync::Arc;

use reqwest::Client;

use crate::config::types::ProviderCredentials;
use crate::provider::anthropic::AnthropicProvider;
use crate::provider::bedrock::BedrockProvider;
use crate::provider::foundry::FoundryProvider;
use crate::provider::vertex::VertexProvider;
use crate::provider::Provider;
use crate::types::ProviderKind;

/// Build a provider instance from a kind and its resolved credentials.
pub fn build_provider(
    kind: ProviderKind,
    credentials: ProviderCredentials,
) -> anyhow::Result<Arc<dyn Provider>> {
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
        _ => anyhow::bail!("Credential type mismatch"),
    }
}

/// Shared HTTP client constructor used by all providers.
pub fn http_client() -> Client {
    Client::builder()
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .pool_max_idle_per_host(32)
        .tcp_nodelay(true)
        .tcp_keepalive(std::time::Duration::from_secs(30))
        .build()
        .expect("Failed to build HTTP client")
}
