use std::sync::Arc;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use wormhole::config::types::WormholeConfig;
use wormhole::provider::anthropic::AnthropicProvider;
use wormhole::provider::Provider;
use wormhole::server;
use wormhole::server::state::AppState;

mod helpers;

#[tokio::test]
async fn test_daemon_session_creation() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(helpers::sample_messages_response()),
        )
        .mount(&mock_server)
        .await;

    // Start daemon-like proxy (no default provider)
    let shutdown = tokio_util::sync::CancellationToken::new();

    let mut config = WormholeConfig::default();
    // Configure anthropic provider pointing to mock
    config.providers.anthropic = Some(wormhole::config::types::AnthropicConfig {
        api_key: Some("test-key".to_string()),
        api_key_env: None,
        base_url: Some(mock_server.uri()),
        default_model: None,
        name: None,
    });

    let state = Arc::new(AppState::new(config, shutdown.clone()));
    let (host, port) = server::start_server(state, "127.0.0.1", 0, shutdown.clone())
        .await
        .unwrap();

    let client = reqwest::Client::new();

    // Create a session
    let create_resp = client
        .post(format!("http://{}:{}/v1/sessions", host, port))
        .json(&serde_json::json!({
            "provider": "anthropic",
            "model": "claude-sonnet-4-5-20250929"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(create_resp.status(), 201);
    let session: serde_json::Value = create_resp.json().await.unwrap();
    let session_id = session["id"].as_str().unwrap().to_string();

    // List sessions
    let list_resp = client
        .get(format!("http://{}:{}/v1/sessions", host, port))
        .send()
        .await
        .unwrap();

    assert_eq!(list_resp.status(), 200);
    let list: serde_json::Value = list_resp.json().await.unwrap();
    assert!(list["sessions"].as_array().unwrap().len() >= 1);

    // Get session
    let get_resp = client
        .get(format!("http://{}:{}/v1/sessions/{}", host, port, session_id))
        .send()
        .await
        .unwrap();
    assert_eq!(get_resp.status(), 200);

    // Use session for a request
    let msg_resp = client
        .post(format!("http://{}:{}/v1/messages", host, port))
        .header("X-Wormhole-Session", &session_id)
        .json(&helpers::sample_nonstreaming_request())
        .send()
        .await
        .unwrap();

    assert_eq!(msg_resp.status(), 200);
    let result: serde_json::Value = msg_resp.json().await.unwrap();
    assert_eq!(result["type"], "message");

    // Delete session
    let del_resp = client
        .delete(format!("http://{}:{}/v1/sessions/{}", host, port, session_id))
        .send()
        .await
        .unwrap();
    assert_eq!(del_resp.status(), 204);

    // Verify session is gone
    let get_resp = client
        .get(format!("http://{}:{}/v1/sessions/{}", host, port, session_id))
        .send()
        .await
        .unwrap();
    assert_eq!(get_resp.status(), 404);

    shutdown.cancel();
}

#[tokio::test]
async fn test_daemon_multiple_sessions() {
    let mock_server_1 = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "type": "message",
            "content": [{"type": "text", "text": "From provider 1"}],
        })))
        .mount(&mock_server_1)
        .await;

    let mock_server_2 = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "type": "message",
            "content": [{"type": "text", "text": "From provider 2"}],
        })))
        .mount(&mock_server_2)
        .await;

    let shutdown = tokio_util::sync::CancellationToken::new();
    let mut config = WormholeConfig::default();
    config.providers.anthropic = Some(wormhole::config::types::AnthropicConfig {
        api_key: Some("key1".to_string()),
        api_key_env: None,
        base_url: Some(mock_server_1.uri()),
        default_model: None,
        name: None,
    });

    let state = Arc::new(AppState::new(config, shutdown.clone()));

    // Manually add two sessions with different providers
    let provider_1: Arc<dyn Provider> = Arc::new(AnthropicProvider::new(
        secrecy::SecretString::from("key1".to_string()),
        Some(mock_server_1.uri()),
    ));
    let provider_2: Arc<dyn Provider> = Arc::new(AnthropicProvider::new(
        secrecy::SecretString::from("key2".to_string()),
        Some(mock_server_2.uri()),
    ));

    state.add_session("session-1".to_string(), provider_1);
    state.add_session("session-2".to_string(), provider_2);

    let (host, port) = server::start_server(state, "127.0.0.1", 0, shutdown.clone())
        .await
        .unwrap();

    let client = reqwest::Client::new();

    // Request via session 1
    let resp1 = client
        .post(format!("http://{}:{}/v1/messages", host, port))
        .header("X-Wormhole-Session", "session-1")
        .json(&helpers::sample_nonstreaming_request())
        .send()
        .await
        .unwrap();

    let result1: serde_json::Value = resp1.json().await.unwrap();
    assert_eq!(result1["content"][0]["text"], "From provider 1");

    // Request via session 2
    let resp2 = client
        .post(format!("http://{}:{}/v1/messages", host, port))
        .header("X-Wormhole-Session", "session-2")
        .json(&helpers::sample_nonstreaming_request())
        .send()
        .await
        .unwrap();

    let result2: serde_json::Value = resp2.json().await.unwrap();
    assert_eq!(result2["content"][0]["text"], "From provider 2");

    shutdown.cancel();
}

#[tokio::test]
async fn test_daemon_invalid_session() {
    let shutdown = tokio_util::sync::CancellationToken::new();
    let state = Arc::new(AppState::new(WormholeConfig::default(), shutdown.clone()));
    let (host, port) = server::start_server(state, "127.0.0.1", 0, shutdown.clone())
        .await
        .unwrap();

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{}:{}/v1/messages", host, port))
        .header("X-Wormhole-Session", "nonexistent")
        .json(&helpers::sample_nonstreaming_request())
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);

    shutdown.cancel();
}
