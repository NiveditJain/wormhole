pub mod anthropic;
pub mod bedrock;
pub mod builder;
pub mod failover;
pub mod foundry;
pub mod model_map;
pub mod vertex;

use async_trait::async_trait;
use bytes::Bytes;
use futures_util::Stream;
use http::HeaderMap;
use std::pin::Pin;

use crate::error::ProviderError;
use crate::types::ProviderKind;

pub type SseByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, ProviderError>> + Send>>;

#[async_trait]
pub trait Provider: Send + Sync + 'static {
    fn kind(&self) -> ProviderKind;

    async fn messages_stream(
        &self,
        body: serde_json::Value,
        headers: &HeaderMap,
    ) -> Result<SseByteStream, ProviderError>;

    async fn messages(
        &self,
        body: serde_json::Value,
        headers: &HeaderMap,
    ) -> Result<serde_json::Value, ProviderError>;

    async fn count_tokens(
        &self,
        body: serde_json::Value,
        headers: &HeaderMap,
    ) -> Result<serde_json::Value, ProviderError>;

    async fn list_models(&self) -> Result<serde_json::Value, ProviderError>;
}
