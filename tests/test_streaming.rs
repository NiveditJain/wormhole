use std::sync::Arc;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use wormhole::provider::anthropic::AnthropicProvider;
use wormhole::provider::Provider;
use wormhole::server;
use wormhole::server::state::AppState;
use wormhole::config::types::WormholeConfig;

mod helpers;

#[tokio::test]
async fn test_proxy_streaming_passthrough() {
    // Set up mock upstream
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

    // Create provider pointing to mock
    let provider: Arc<dyn Provider> = Arc::new(AnthropicProvider::new(
        secrecy::SecretString::from("test-key".to_string()),
        Some(mock_server.uri()),
    ));

    // Start wormhole proxy
    let shutdown = tokio_util::sync::CancellationToken::new();
    let state = Arc::new(
        AppState::new(WormholeConfig::default(), shutdown.clone())
            .with_default_provider(provider),
    );
    let (host, port) = server::start_server(state, "127.0.0.1", 0, shutdown.clone())
        .await
        .unwrap();

    // Make a streaming request through the proxy
    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{}:{}/v1/messages", host, port))
        .json(&helpers::sample_streaming_request())
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "text/event-stream"
    );

    let body = response.text().await.unwrap();
    assert!(body.contains("event: message_start"));
    assert!(body.contains("event: content_block_delta"));
    assert!(body.contains("Hello"));
    assert!(body.contains("event: message_stop"));

    shutdown.cancel();
}

#[tokio::test]
async fn test_proxy_nonstreaming_passthrough() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(helpers::sample_messages_response()),
        )
        .mount(&mock_server)
        .await;

    let provider: Arc<dyn Provider> = Arc::new(AnthropicProvider::new(
        secrecy::SecretString::from("test-key".to_string()),
        Some(mock_server.uri()),
    ));

    let shutdown = tokio_util::sync::CancellationToken::new();
    let state = Arc::new(
        AppState::new(WormholeConfig::default(), shutdown.clone())
            .with_default_provider(provider),
    );
    let (host, port) = server::start_server(state, "127.0.0.1", 0, shutdown.clone())
        .await
        .unwrap();

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{}:{}/v1/messages", host, port))
        .json(&helpers::sample_nonstreaming_request())
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let result: serde_json::Value = response.json().await.unwrap();
    assert_eq!(result["type"], "message");
    assert_eq!(result["content"][0]["text"], "Hello!");

    shutdown.cancel();
}

#[tokio::test]
async fn test_proxy_health_endpoint() {
    let shutdown = tokio_util::sync::CancellationToken::new();
    let state = Arc::new(AppState::new(WormholeConfig::default(), shutdown.clone()));
    let (host, port) = server::start_server(state, "127.0.0.1", 0, shutdown.clone())
        .await
        .unwrap();

    let client = reqwest::Client::new();
    let response = client
        .get(format!("http://{}:{}/health", host, port))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["status"], "ok");

    shutdown.cancel();
}

#[tokio::test]
async fn test_proxy_no_provider_error() {
    // Test that requesting without a provider gives an error
    let shutdown = tokio_util::sync::CancellationToken::new();
    let state = Arc::new(AppState::new(WormholeConfig::default(), shutdown.clone()));
    let (host, port) = server::start_server(state, "127.0.0.1", 0, shutdown.clone())
        .await
        .unwrap();

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{}:{}/v1/messages", host, port))
        .json(&helpers::sample_nonstreaming_request())
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 500);
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["type"], "error");

    shutdown.cancel();
}
