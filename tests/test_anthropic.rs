use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use wormhole::provider::anthropic::AnthropicProvider;
use wormhole::provider::Provider;
use wormhole::types::ProviderKind;

mod helpers;

#[tokio::test]
async fn test_anthropic_provider_kind() {
    let provider = AnthropicProvider::new(
        secrecy::SecretString::from("test-key".to_string()),
        None,
    );
    assert_eq!(provider.kind(), ProviderKind::Anthropic);
}

#[tokio::test]
async fn test_anthropic_nonstreaming_messages() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "test-key"))
        .and(header("anthropic-version", "2023-06-01"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(helpers::sample_messages_response()),
        )
        .mount(&mock_server)
        .await;

    let provider = AnthropicProvider::new(
        secrecy::SecretString::from("test-key".to_string()),
        Some(mock_server.uri()),
    );

    let body = helpers::sample_nonstreaming_request();
    let headers = http::HeaderMap::new();

    let result = provider.messages(body, &headers).await.unwrap();
    assert_eq!(result["type"], "message");
    assert_eq!(result["role"], "assistant");
}

#[tokio::test]
async fn test_anthropic_streaming_messages() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(helpers::sample_sse_full_response()),
        )
        .mount(&mock_server)
        .await;

    let provider = AnthropicProvider::new(
        secrecy::SecretString::from("test-key".to_string()),
        Some(mock_server.uri()),
    );

    let body = helpers::sample_streaming_request();
    let headers = http::HeaderMap::new();

    let stream = provider.messages_stream(body, &headers).await.unwrap();

    // Collect all bytes from the stream
    use futures_util::StreamExt;
    let mut all_bytes = Vec::new();
    let mut stream = stream;
    while let Some(result) = stream.next().await {
        let bytes = result.unwrap();
        all_bytes.extend_from_slice(&bytes);
    }

    let response_text = String::from_utf8(all_bytes).unwrap();
    assert!(response_text.contains("event: message_start"));
    assert!(response_text.contains("event: content_block_delta"));
    assert!(response_text.contains("event: message_stop"));
}

#[tokio::test]
async fn test_anthropic_error_propagation() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(429).set_body_json(serde_json::json!({
            "type": "error",
            "error": {"type": "rate_limit_error", "message": "Rate limited"}
        })))
        .mount(&mock_server)
        .await;

    let provider = AnthropicProvider::new(
        secrecy::SecretString::from("test-key".to_string()),
        Some(mock_server.uri()),
    );

    let body = helpers::sample_nonstreaming_request();
    let headers = http::HeaderMap::new();

    let err = provider.messages(body, &headers).await.unwrap_err();
    assert_eq!(err.status_code(), 429);
}

#[tokio::test]
async fn test_anthropic_beta_header_forwarding() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("anthropic-beta", "max-tokens-3-5-sonnet-2024-07-15"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(helpers::sample_messages_response()),
        )
        .mount(&mock_server)
        .await;

    let provider = AnthropicProvider::new(
        secrecy::SecretString::from("test-key".to_string()),
        Some(mock_server.uri()),
    );

    let body = helpers::sample_nonstreaming_request();
    let mut headers = http::HeaderMap::new();
    headers.insert(
        "anthropic-beta",
        "max-tokens-3-5-sonnet-2024-07-15".parse().unwrap(),
    );

    let result = provider.messages(body, &headers).await.unwrap();
    assert_eq!(result["type"], "message");
}

#[tokio::test]
async fn test_anthropic_list_models() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{"id": "claude-sonnet-4-5-20250929"}],
            "has_more": false,
        })))
        .mount(&mock_server)
        .await;

    let provider = AnthropicProvider::new(
        secrecy::SecretString::from("test-key".to_string()),
        Some(mock_server.uri()),
    );

    let result = provider.list_models().await.unwrap();
    assert!(result["data"].is_array());
}

#[tokio::test]
async fn test_anthropic_count_tokens() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/messages/count_tokens"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "input_tokens": 42,
        })))
        .mount(&mock_server)
        .await;

    let provider = AnthropicProvider::new(
        secrecy::SecretString::from("test-key".to_string()),
        Some(mock_server.uri()),
    );

    let body = serde_json::json!({
        "model": "claude-sonnet-4-5-20250929",
        "messages": [{"role": "user", "content": "Hello"}]
    });
    let headers = http::HeaderMap::new();

    let result = provider.count_tokens(body, &headers).await.unwrap();
    assert_eq!(result["input_tokens"], 42);
}
