use async_trait::async_trait;
use futures_util::StreamExt;
use http::HeaderMap;
use reqwest::Client;
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use tracing::debug;

use crate::error::ProviderError;
use crate::provider::builder::http_client;
use crate::provider::model_map::translate_model_id;
use crate::provider::{Provider, SseByteStream};
use crate::types::ProviderKind;

const VERTEX_ANTHROPIC_VERSION: &str = "vertex-2023-10-16";

pub struct VertexProvider {
    client: Client,
    token: SecretString,
    /// Pre-computed base URL
    base_url: String,
}

impl VertexProvider {
    pub fn new(project_id: String, region: String, token: SecretString) -> Self {
        let client = http_client();

        let base_url = format!(
            "https://{}-aiplatform.googleapis.com/v1/projects/{}/locations/{}/publishers/anthropic",
            region, project_id, region
        );

        Self {
            client,
            token,
            base_url,
        }
    }

    fn transform_body(&self, mut body: serde_json::Value) -> (String, serde_json::Value) {
        let obj = body.as_object_mut().expect("body must be an object");

        // Extract and translate model ID
        let model = obj
            .remove("model")
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "claude-sonnet-4-5-20250929".to_string());

        let vertex_model = translate_model_id(ProviderKind::Vertex, &model).into_owned();

        // Remove stream field (endpoint determines streaming)
        obj.remove("stream");

        // Add anthropic_version
        obj.insert(
            "anthropic_version".to_string(),
            json!(VERTEX_ANTHROPIC_VERSION),
        );

        (vertex_model, body)
    }

    fn build_request(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> reqwest::RequestBuilder {
        self.client
            .post(url)
            .header(
                "Authorization",
                format!("Bearer {}", self.token.expose_secret()),
            )
            .header("content-type", "application/json")
            .json(body)
    }
}

#[async_trait]
impl Provider for VertexProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Vertex
    }

    async fn messages_stream(
        &self,
        body: serde_json::Value,
        _headers: &HeaderMap,
    ) -> Result<SseByteStream, ProviderError> {
        debug!("Vertex messages_stream request");

        let (model_id, transformed_body) = self.transform_body(body);
        let url = format!(
            "{}/models/{}:streamRawPredict",
            self.base_url,
            model_id
        );

        let response = self
            .build_request(&url, &transformed_body)
            .send()
            .await
            .map_err(|e| ProviderError::Connection {
                provider: ProviderKind::Vertex,
                source: e,
            })?;

        let status = response.status().as_u16();
        if status >= 400 {
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::Upstream {
                provider: ProviderKind::Vertex,
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
        debug!("Vertex messages request (non-streaming)");

        let (model_id, transformed_body) = self.transform_body(body);
        let url = format!(
            "{}/models/{}:rawPredict",
            self.base_url,
            model_id
        );

        let response = self
            .build_request(&url, &transformed_body)
            .send()
            .await
            .map_err(|e| ProviderError::Connection {
                provider: ProviderKind::Vertex,
                source: e,
            })?;

        let status = response.status().as_u16();
        if status >= 400 {
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::Upstream {
                provider: ProviderKind::Vertex,
                status,
                message: format!("HTTP {}", status),
                body: Some(body),
            });
        }

        response.json().await.map_err(|e| ProviderError::Connection {
            provider: ProviderKind::Vertex,
            source: e,
        })
    }

    async fn count_tokens(
        &self,
        body: serde_json::Value,
        _headers: &HeaderMap,
    ) -> Result<serde_json::Value, ProviderError> {
        debug!("Vertex count_tokens request");

        let (model_id, transformed_body) = self.transform_body(body);
        let url = format!(
            "{}/models/{}:rawPredict",
            self.base_url,
            model_id
        );

        // Vertex supports count_tokens through rawPredict with the right body
        let count_body = transformed_body;
        // The count_tokens request structure is sent as-is through rawPredict

        let response = self
            .build_request(&url, &count_body)
            .send()
            .await
            .map_err(|e| ProviderError::Connection {
                provider: ProviderKind::Vertex,
                source: e,
            })?;

        let status = response.status().as_u16();
        if status >= 400 {
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::Upstream {
                provider: ProviderKind::Vertex,
                status,
                message: format!("HTTP {}", status),
                body: Some(body),
            });
        }

        response.json().await.map_err(|e| ProviderError::Connection {
            provider: ProviderKind::Vertex,
            source: e,
        })
    }

    async fn list_models(&self) -> Result<serde_json::Value, ProviderError> {
        debug!("Vertex list_models request");

        Ok(json!({
            "data": [
                {"id": "claude-opus-4-6", "display_name": "Claude Opus 4.6", "provider": "vertex"},
                {"id": "claude-sonnet-4-5-20250929", "display_name": "Claude Sonnet 4.5", "provider": "vertex"},
                {"id": "claude-haiku-4-5-20251001", "display_name": "Claude Haiku 4.5", "provider": "vertex"},
                {"id": "claude-3-5-sonnet-20241022", "display_name": "Claude 3.5 Sonnet", "provider": "vertex"},
                {"id": "claude-3-5-haiku-20241022", "display_name": "Claude 3.5 Haiku", "provider": "vertex"},
            ],
            "has_more": false,
        }))
    }
}
