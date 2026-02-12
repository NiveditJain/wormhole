use async_trait::async_trait;
use futures_util::StreamExt;
use http::HeaderMap;
use reqwest::Client;
use secrecy::{ExposeSecret, SecretString};
use tracing::debug;

use crate::error::ProviderError;
use crate::provider::builder::http_client;
use crate::provider::{Provider, SseByteStream};
use crate::types::ProviderKind;

const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2023-06-01";

pub struct AnthropicProvider {
    client: Client,
    api_key: SecretString,
    base_url: String,
}

impl AnthropicProvider {
    pub fn new(api_key: SecretString, base_url: Option<String>) -> Self {
        let client = http_client();

        Self {
            client,
            api_key,
            base_url: base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
        }
    }

    fn build_request(
        &self,
        path: &str,
        body: serde_json::Value,
        headers: &HeaderMap,
    ) -> reqwest::RequestBuilder {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self
            .client
            .post(&url)
            .header("x-api-key", self.api_key.expose_secret())
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json");

        // Forward anthropic-beta header if present
        if let Some(beta) = headers.get("anthropic-beta") {
            req = req.header("anthropic-beta", beta);
        }

        req.json(&body)
    }

    async fn send_json(
        &self,
        path: &str,
        body: serde_json::Value,
        headers: &HeaderMap,
    ) -> Result<serde_json::Value, ProviderError> {
        let response = self
            .build_request(path, body, headers)
            .send()
            .await
            .map_err(|e| ProviderError::Connection {
                provider: ProviderKind::Anthropic,
                source: e,
            })?;

        let status = response.status().as_u16();
        if status >= 400 {
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::Upstream {
                provider: ProviderKind::Anthropic,
                status,
                message: format!("HTTP {}", status),
                body: Some(body),
            });
        }

        response.json().await.map_err(|e| ProviderError::Connection {
            provider: ProviderKind::Anthropic,
            source: e,
        })
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Anthropic
    }

    async fn messages_stream(
        &self,
        body: serde_json::Value,
        headers: &HeaderMap,
    ) -> Result<SseByteStream, ProviderError> {
        debug!("Anthropic messages_stream request");

        let response = self
            .build_request("/v1/messages", body, headers)
            .send()
            .await
            .map_err(|e| ProviderError::Connection {
                provider: ProviderKind::Anthropic,
                source: e,
            })?;

        let status = response.status().as_u16();
        if status >= 400 {
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::Upstream {
                provider: ProviderKind::Anthropic,
                status,
                message: format!("HTTP {}", status),
                body: Some(body),
            });
        }

        // Stream bytes directly through - zero transformation needed
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
        headers: &HeaderMap,
    ) -> Result<serde_json::Value, ProviderError> {
        debug!("Anthropic messages request (non-streaming)");
        self.send_json("/v1/messages", body, headers).await
    }

    async fn count_tokens(
        &self,
        body: serde_json::Value,
        headers: &HeaderMap,
    ) -> Result<serde_json::Value, ProviderError> {
        debug!("Anthropic count_tokens request");
        self.send_json("/v1/messages/count_tokens", body, headers)
            .await
    }

    async fn list_models(&self) -> Result<serde_json::Value, ProviderError> {
        debug!("Anthropic list_models request");
        let url = format!("{}/v1/models", self.base_url);
        let response = self
            .client
            .get(&url)
            .header("x-api-key", self.api_key.expose_secret())
            .header("anthropic-version", ANTHROPIC_VERSION)
            .send()
            .await
            .map_err(|e| ProviderError::Connection {
                provider: ProviderKind::Anthropic,
                source: e,
            })?;

        let status = response.status().as_u16();
        if status >= 400 {
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::Upstream {
                provider: ProviderKind::Anthropic,
                status,
                message: format!("HTTP {}", status),
                body: Some(body),
            });
        }

        response.json().await.map_err(|e| ProviderError::Connection {
            provider: ProviderKind::Anthropic,
            source: e,
        })
    }
}
