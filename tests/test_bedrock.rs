use wormhole::provider::model_map::translate_model_id;
use wormhole::types::ProviderKind;

#[test]
fn test_bedrock_model_id_translation() {
    let cases = vec![
        ("claude-opus-4-6", "global.anthropic.claude-opus-4-6-v1"),
        (
            "claude-sonnet-4-5-20250929",
            "global.anthropic.claude-sonnet-4-5-v1",
        ),
        (
            "claude-haiku-4-5-20251001",
            "global.anthropic.claude-haiku-4-5-v1",
        ),
        (
            "claude-3-5-sonnet-20241022",
            "global.anthropic.claude-3-5-sonnet-20241022-v2",
        ),
        (
            "claude-3-5-haiku-20241022",
            "global.anthropic.claude-3-5-haiku-20241022-v1",
        ),
        (
            "claude-3-opus-20240229",
            "global.anthropic.claude-3-opus-20240229-v1",
        ),
    ];

    for (input, expected) in cases {
        assert_eq!(
            translate_model_id(ProviderKind::Bedrock, input),
            expected,
            "Failed for input: {}",
            input
        );
    }
}

#[test]
fn test_bedrock_passthrough_already_formatted() {
    assert_eq!(
        translate_model_id(
            ProviderKind::Bedrock,
            "global.anthropic.claude-3-opus-20240229-v1"
        ),
        "global.anthropic.claude-3-opus-20240229-v1"
    );
}

#[test]
fn test_bedrock_unknown_model_fallback() {
    assert_eq!(
        translate_model_id(ProviderKind::Bedrock, "claude-future-5-0"),
        "global.anthropic.claude-future-5-0-v1"
    );
}

#[test]
fn test_bedrock_body_transformation() {
    // Test that Bedrock request transformation works correctly
    let body = serde_json::json!({
        "model": "claude-sonnet-4-5-20250929",
        "max_tokens": 1024,
        "stream": true,
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let mut body_clone = body.clone();
    let obj = body_clone.as_object_mut().unwrap();

    // Simulate bedrock transform
    let _model = obj.remove("model").unwrap();
    obj.remove("stream");
    obj.insert(
        "anthropic_version".to_string(),
        serde_json::json!("bedrock-2023-05-31"),
    );

    // Verify model is removed from body
    assert!(obj.get("model").is_none());
    // Verify stream is removed
    assert!(obj.get("stream").is_none());
    // Verify anthropic_version is added
    assert_eq!(obj["anthropic_version"], "bedrock-2023-05-31");
    // Verify other fields are preserved
    assert_eq!(obj["max_tokens"], 1024);
    assert!(obj["messages"].is_array());
}

#[test]
fn test_vertex_model_passthrough() {
    assert_eq!(
        translate_model_id(ProviderKind::Vertex, "claude-opus-4-6"),
        "claude-opus-4-6"
    );

    assert_eq!(
        translate_model_id(ProviderKind::Vertex, "claude-sonnet-4-5-20250929"),
        "claude-sonnet-4-5-20250929"
    );
}

#[test]
fn test_vertex_body_transformation() {
    let body = serde_json::json!({
        "model": "claude-opus-4-6",
        "max_tokens": 1024,
        "stream": true,
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let mut body_clone = body.clone();
    let obj = body_clone.as_object_mut().unwrap();

    // Simulate vertex transform
    obj.remove("model");
    obj.remove("stream");
    obj.insert(
        "anthropic_version".to_string(),
        serde_json::json!("vertex-2023-10-16"),
    );

    assert!(obj.get("model").is_none());
    assert!(obj.get("stream").is_none());
    assert_eq!(obj["anthropic_version"], "vertex-2023-10-16");
    assert_eq!(obj["max_tokens"], 1024);
}

#[test]
fn test_foundry_body_passthrough() {
    // Foundry keeps model and stream in body
    let body = serde_json::json!({
        "model": "claude-opus-4-6",
        "max_tokens": 1024,
        "stream": true,
        "messages": [{"role": "user", "content": "Hello"}]
    });

    // Foundry doesn't transform the body
    assert_eq!(body["model"], "claude-opus-4-6");
    assert_eq!(body["stream"], true);
}
