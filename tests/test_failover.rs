use std::sync::Arc;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use wormhole::provider::anthropic::AnthropicProvider;
use wormhole::provider::failover::FailoverProvider;
use wormhole::provider::Provider;

mod helpers;

#[tokio::test]
async fn test_failover_on_429() {
    // Primary server returns 429
    let primary_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(429).set_body_json(serde_json::json!({
            "type": "error",
            "error": {"type": "rate_limit_error", "message": "Rate limited"}
        })))
        .mount(&primary_server)
        .await;

    // Fallback server returns success
    let fallback_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(helpers::sample_messages_response()),
        )
        .mount(&fallback_server)
        .await;

    let primary: Arc<dyn Provider> = Arc::new(AnthropicProvider::new(
        secrecy::SecretString::from("test-key".to_string()),
        Some(primary_server.uri()),
    ));

    let fallback: Arc<dyn Provider> = Arc::new(AnthropicProvider::new(
        secrecy::SecretString::from("test-key".to_string()),
        Some(fallback_server.uri()),
    ));

    let failover = FailoverProvider::new(
        primary,
        vec![fallback],
        2,
        vec![429, 500, 502, 503, 529],
        10, // Short backoff for tests
    );

    let body = helpers::sample_nonstreaming_request();
    let headers = http::HeaderMap::new();

    let result = failover.messages(body, &headers).await.unwrap();
    assert_eq!(result["type"], "message");
}

#[tokio::test]
async fn test_failover_on_500() {
    let primary_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .mount(&primary_server)
        .await;

    let fallback_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(helpers::sample_messages_response()),
        )
        .mount(&fallback_server)
        .await;

    let primary: Arc<dyn Provider> = Arc::new(AnthropicProvider::new(
        secrecy::SecretString::from("test-key".to_string()),
        Some(primary_server.uri()),
    ));

    let fallback: Arc<dyn Provider> = Arc::new(AnthropicProvider::new(
        secrecy::SecretString::from("test-key".to_string()),
        Some(fallback_server.uri()),
    ));

    let failover = FailoverProvider::new(primary, vec![fallback], 2, vec![429, 500], 10);

    let body = helpers::sample_nonstreaming_request();
    let headers = http::HeaderMap::new();

    let result = failover.messages(body, &headers).await.unwrap();
    assert_eq!(result["type"], "message");
}

#[tokio::test]
async fn test_no_failover_on_400() {
    // 400 is not retryable - should fail immediately
    let primary_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "type": "error",
            "error": {"type": "invalid_request_error", "message": "Bad request"}
        })))
        .mount(&primary_server)
        .await;

    let fallback_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(helpers::sample_messages_response()),
        )
        .mount(&fallback_server)
        .await;

    let primary: Arc<dyn Provider> = Arc::new(AnthropicProvider::new(
        secrecy::SecretString::from("test-key".to_string()),
        Some(primary_server.uri()),
    ));

    let fallback: Arc<dyn Provider> = Arc::new(AnthropicProvider::new(
        secrecy::SecretString::from("test-key".to_string()),
        Some(fallback_server.uri()),
    ));

    let failover = FailoverProvider::new(primary, vec![fallback], 2, vec![429, 500], 10);

    let body = helpers::sample_nonstreaming_request();
    let headers = http::HeaderMap::new();

    let err = failover.messages(body, &headers).await.unwrap_err();
    assert_eq!(err.status_code(), 400);
}

#[tokio::test]
async fn test_failover_all_providers_fail() {
    let primary_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Error"))
        .mount(&primary_server)
        .await;

    let fallback_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Error"))
        .mount(&fallback_server)
        .await;

    let primary: Arc<dyn Provider> = Arc::new(AnthropicProvider::new(
        secrecy::SecretString::from("test-key".to_string()),
        Some(primary_server.uri()),
    ));

    let fallback: Arc<dyn Provider> = Arc::new(AnthropicProvider::new(
        secrecy::SecretString::from("test-key".to_string()),
        Some(fallback_server.uri()),
    ));

    let failover = FailoverProvider::new(primary, vec![fallback], 2, vec![500], 10);

    let body = helpers::sample_nonstreaming_request();
    let headers = http::HeaderMap::new();

    let err = failover.messages(body, &headers).await.unwrap_err();
    assert_eq!(err.status_code(), 500);
}

#[tokio::test]
async fn test_failover_streaming() {
    // Primary returns 429, fallback streams successfully
    let primary_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(429).set_body_string("Rate limited"))
        .mount(&primary_server)
        .await;

    let fallback_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(helpers::sample_sse_full_response()),
        )
        .mount(&fallback_server)
        .await;

    let primary: Arc<dyn Provider> = Arc::new(AnthropicProvider::new(
        secrecy::SecretString::from("test-key".to_string()),
        Some(primary_server.uri()),
    ));

    let fallback: Arc<dyn Provider> = Arc::new(AnthropicProvider::new(
        secrecy::SecretString::from("test-key".to_string()),
        Some(fallback_server.uri()),
    ));

    let failover = FailoverProvider::new(primary, vec![fallback], 2, vec![429], 10);

    let body = helpers::sample_streaming_request();
    let headers = http::HeaderMap::new();

    let stream = failover.messages_stream(body, &headers).await.unwrap();

    use futures_util::StreamExt;
    let mut all_bytes = Vec::new();
    let mut stream = stream;
    while let Some(result) = stream.next().await {
        let bytes = result.unwrap();
        all_bytes.extend_from_slice(&bytes);
    }

    let response_text = String::from_utf8(all_bytes).unwrap();
    assert!(response_text.contains("event: message_start"));
}
