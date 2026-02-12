#![allow(dead_code)]

use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use wormhole::config::types::WormholeConfig;
use wormhole::server::state::AppState;

/// Create an AppState with default config for testing.
pub fn test_app_state() -> Arc<AppState> {
    let config = WormholeConfig::default();
    let shutdown = CancellationToken::new();
    Arc::new(AppState::new(config, shutdown))
}

/// Create a test AppState with a specific provider as default.
pub fn test_app_state_with_provider(
    provider: Arc<dyn wormhole::provider::Provider>,
) -> Arc<AppState> {
    let config = WormholeConfig::default();
    let shutdown = CancellationToken::new();
    Arc::new(AppState::new(config, shutdown).with_default_provider(provider))
}

/// Sample SSE data chunks for testing
pub fn sample_sse_message_start() -> &'static str {
    "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_01\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-sonnet-4-5-20250929\",\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":1}}}\n\n"
}

pub fn sample_sse_content_block_delta() -> &'static str {
    "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n"
}

pub fn sample_sse_message_stop() -> &'static str {
    "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
}

/// Build a full SSE stream payload
pub fn sample_sse_full_response() -> String {
    format!(
        "{}{}{}",
        sample_sse_message_start(),
        sample_sse_content_block_delta(),
        sample_sse_message_stop()
    )
}

/// Sample non-streaming response
pub fn sample_messages_response() -> serde_json::Value {
    serde_json::json!({
        "id": "msg_01",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "Hello!"}],
        "model": "claude-sonnet-4-5-20250929",
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": {"input_tokens": 10, "output_tokens": 5}
    })
}

/// Sample streaming request body
pub fn sample_streaming_request() -> serde_json::Value {
    serde_json::json!({
        "model": "claude-sonnet-4-5-20250929",
        "max_tokens": 1024,
        "stream": true,
        "messages": [{"role": "user", "content": "Hello"}]
    })
}

/// Sample non-streaming request body
pub fn sample_nonstreaming_request() -> serde_json::Value {
    serde_json::json!({
        "model": "claude-sonnet-4-5-20250929",
        "max_tokens": 1024,
        "stream": false,
        "messages": [{"role": "user", "content": "Hello"}]
    })
}
