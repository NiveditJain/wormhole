use async_trait::async_trait;
use http::HeaderMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, warn};

use crate::error::ProviderError;
use crate::provider::{Provider, SseByteStream};
use crate::types::ProviderKind;

pub struct FailoverProvider {
    primary: Arc<dyn Provider>,
    fallbacks: Vec<Arc<dyn Provider>>,
    max_retries: usize,
    retryable_statuses: HashSet<u16>,
    backoff_base: Duration,
}

impl FailoverProvider {
    pub fn new(
        primary: Arc<dyn Provider>,
        fallbacks: Vec<Arc<dyn Provider>>,
        max_retries: usize,
        retryable_statuses: Vec<u16>,
        backoff_base_ms: u64,
    ) -> Self {
        Self {
            primary,
            fallbacks,
            max_retries,
            retryable_statuses: retryable_statuses.into_iter().collect(),
            backoff_base: Duration::from_millis(backoff_base_ms),
        }
    }

    #[inline]
    fn is_retryable(&self, err: &ProviderError) -> bool {
        match err {
            ProviderError::Upstream { status, .. } => self.retryable_statuses.contains(status),
            ProviderError::Connection { .. } => true,
            _ => false,
        }
    }
}

#[async_trait]
impl Provider for FailoverProvider {
    fn kind(&self) -> ProviderKind {
        self.primary.kind()
    }

    async fn messages_stream(
        &self,
        body: serde_json::Value,
        headers: &HeaderMap,
    ) -> Result<SseByteStream, ProviderError> {
        // Fast path: try primary without cloning body
        match self.primary.messages_stream(body.clone(), headers).await {
            Ok(stream) => return Ok(stream),
            Err(e) => {
                if !self.is_retryable(&e) || self.max_retries == 0 || self.fallbacks.is_empty() {
                    return Err(e);
                }
                warn!(
                    "Retryable error from {}: {} (will try fallbacks)",
                    self.primary.kind(),
                    e
                );
            }
        }

        // Slow path: iterate through fallbacks
        let mut last_error = None;
        let limit = self.max_retries.min(self.fallbacks.len());

        for (i, fallback) in self.fallbacks.iter().take(limit).enumerate() {
            let backoff = self.backoff_base * 2u32.pow(i as u32);
            warn!(
                "Failover attempt {} to {} (backoff {:?})",
                i + 1,
                fallback.kind(),
                backoff
            );
            tokio::time::sleep(backoff).await;

            match fallback.messages_stream(body.clone(), headers).await {
                Ok(stream) => {
                    debug!(
                        "Failover succeeded on attempt {} with {}",
                        i + 1,
                        fallback.kind()
                    );
                    return Ok(stream);
                }
                Err(e) => {
                    if self.is_retryable(&e) {
                        warn!("Retryable error from {}: {}", fallback.kind(), e);
                        last_error = Some(e);
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            ProviderError::Other(anyhow::anyhow!("All providers exhausted"))
        }))
    }

    async fn messages(
        &self,
        body: serde_json::Value,
        headers: &HeaderMap,
    ) -> Result<serde_json::Value, ProviderError> {
        // Fast path: try primary
        match self.primary.messages(body.clone(), headers).await {
            Ok(result) => return Ok(result),
            Err(e) => {
                if !self.is_retryable(&e) || self.max_retries == 0 || self.fallbacks.is_empty() {
                    return Err(e);
                }
                warn!(
                    "Retryable error from {}: {} (will try fallbacks)",
                    self.primary.kind(),
                    e
                );
            }
        }

        // Slow path: iterate through fallbacks
        let mut last_error = None;
        let limit = self.max_retries.min(self.fallbacks.len());

        for (i, fallback) in self.fallbacks.iter().take(limit).enumerate() {
            let backoff = self.backoff_base * 2u32.pow(i as u32);
            warn!(
                "Failover attempt {} to {} (backoff {:?})",
                i + 1,
                fallback.kind(),
                backoff
            );
            tokio::time::sleep(backoff).await;

            match fallback.messages(body.clone(), headers).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    if self.is_retryable(&e) {
                        warn!("Retryable error from {}: {}", fallback.kind(), e);
                        last_error = Some(e);
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            ProviderError::Other(anyhow::anyhow!("All providers exhausted"))
        }))
    }

    async fn count_tokens(
        &self,
        body: serde_json::Value,
        headers: &HeaderMap,
    ) -> Result<serde_json::Value, ProviderError> {
        self.primary.count_tokens(body, headers).await
    }

    async fn list_models(&self) -> Result<serde_json::Value, ProviderError> {
        self.primary.list_models().await
    }
}
