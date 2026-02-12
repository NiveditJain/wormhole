use async_trait::async_trait;
use futures_util::StreamExt;
use http::HeaderMap;
use reqwest::Client;
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use tracing::debug;

use crate::error::ProviderError;
use crate::provider::{Provider, SseByteStream};
use crate::types::ProviderKind;

pub struct FoundryProvider {
    client: Client,
    #[allow(dead_code)]
    resource: String,
    api_key: SecretString,
    base_url: String,
}

impl FoundryProvider {
    pub fn new(resource: String, api_key: SecretString) -> Self {
        let client = Client::builder()
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .pool_max_idle_per_host(32)
            .tcp_nodelay(true)
            .tcp_keepalive(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client");

        let base_url = format!(
            "https://{}.services.ai.azure.com/anthropic/v1",
            resource
        );

        Self {
            client,
            resource,
            api_key,
            base_url,
        }
    }

    fn build_request(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> reqwest::RequestBuilder {
        let url = format!("{}{}", self.base_url, path);
        self.client
            .post(&url)
            .header("api-key", self.api_key.expose_secret())
            .header("content-type", "application/json")
            .json(body)
    }

    async fn send_json(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, ProviderError> {
        let response = self
            .build_request(path, body)
            .send()
            .await
            .map_err(|e| ProviderError::Connection {
                provider: ProviderKind::Foundry,
                source: e,
            })?;

        let status = response.status().as_u16();
        if status >= 400 {
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::Upstream {
                provider: ProviderKind::Foundry,
                status,
                message: format!("HTTP {}", status),
                body: Some(body),
            });
        }

        response.json().await.map_err(|e| ProviderError::Connection {
            provider: ProviderKind::Foundry,
            source: e,
        })
    }
}

#[async_trait]
impl Provider for FoundryProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Foundry
    }

    async fn messages_stream(
        &self,
        body: serde_json::Value,
        _headers: &HeaderMap,
    ) -> Result<SseByteStream, ProviderError> {
        debug!("Foundry messages_stream request");

        let response = self
            .build_request("/messages", &body)
            .send()
            .await
            .map_err(|e| ProviderError::Connection {
                provider: ProviderKind::Foundry,
                source: e,
            })?;

        let status = response.status().as_u16();
        if status >= 400 {
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::Upstream {
                provider: ProviderKind::Foundry,
                status,
                message: format!("HTTP {}", status),
                body: Some(body),
            });
        }

        // SSE passthrough - same format as Anthropic
        let stream = response.bytes_stream().map(|result| {
            result.map_err(|e| {
                ProviderError::StreamDecode(format!("Stream read error: {}", e))
            })
        });

        Ok(Box::pin(stream))
    }

    async fn messages(
        &self,
        body: serde_json::Value,
        _headers: &HeaderMap,
    ) -> Result<serde_json::Value, ProviderError> {
        debug!("Foundry messages request (non-streaming)");
        self.send_json("/messages", &body).await
    }

    async fn count_tokens(
        &self,
        body: serde_json::Value,
        _headers: &HeaderMap,
    ) -> Result<serde_json::Value, ProviderError> {
        debug!("Foundry count_tokens request");
        self.send_json("/messages/count_tokens", &body).await
    }

    async fn list_models(&self) -> Result<serde_json::Value, ProviderError> {
        debug!("Foundry list_models request");

        // Foundry doesn't support /v1/models - return hardcoded list
        Ok(json!({
            "data": [
                {"id": "claude-opus-4-6", "display_name": "Claude Opus 4.6", "provider": "foundry"},
                {"id": "claude-sonnet-4-5-20250929", "display_name": "Claude Sonnet 4.5", "provider": "foundry"},
                {"id": "claude-haiku-4-5-20251001", "display_name": "Claude Haiku 4.5", "provider": "foundry"},
            ],
            "has_more": false,
        }))
    }
}
