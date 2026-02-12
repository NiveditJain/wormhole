use tempfile::TempDir;

use wormhole::session::{SessionState, SessionStore};
use wormhole::types::ProviderKind;

fn setup_session_store(temp_dir: &TempDir) -> SessionStore {
    std::env::set_var("HOME", temp_dir.path().to_str().unwrap());
    SessionStore::new().unwrap()
}

#[test]
fn test_create_and_get_session() {
    let temp_dir = TempDir::new().unwrap();
    let store = setup_session_store(&temp_dir);

    let session = SessionState::new(ProviderKind::Anthropic, Some("claude-sonnet-4-5-20250929".to_string()), None, vec![]);
    store.create(&session).unwrap();

    let retrieved = store.get(&session.id).unwrap();
    assert_eq!(retrieved.id, session.id);
    assert_eq!(retrieved.provider, ProviderKind::Anthropic);
    assert_eq!(
        retrieved.model.as_deref(),
        Some("claude-sonnet-4-5-20250929")
    );
}

#[test]
fn test_session_prefix_match() {
    let temp_dir = TempDir::new().unwrap();
    let store = setup_session_store(&temp_dir);

    let session = SessionState::new(ProviderKind::Bedrock, None, None, vec![]);
    let prefix = session.id[..8].to_string();
    store.create(&session).unwrap();

    let retrieved = store.get(&prefix).unwrap();
    assert_eq!(retrieved.id, session.id);
}

#[test]
fn test_session_list() {
    let temp_dir = TempDir::new().unwrap();
    let store = setup_session_store(&temp_dir);

    let s1 = SessionState::new(ProviderKind::Anthropic, None, None, vec![]);
    let s2 = SessionState::new(ProviderKind::Bedrock, None, None, vec![]);
    store.create(&s1).unwrap();
    store.create(&s2).unwrap();

    let sessions = store.list().unwrap();
    assert_eq!(sessions.len(), 2);
}

#[test]
fn test_session_delete() {
    let temp_dir = TempDir::new().unwrap();
    let store = setup_session_store(&temp_dir);

    let session = SessionState::new(ProviderKind::Vertex, None, None, vec![]);
    store.create(&session).unwrap();

    store.delete(&session.id).unwrap();

    let result = store.get(&session.id);
    assert!(result.is_err());
}

#[test]
fn test_session_touch() {
    let temp_dir = TempDir::new().unwrap();
    let store = setup_session_store(&temp_dir);

    let session = SessionState::new(ProviderKind::Foundry, None, None, vec![]);
    let original_time = session.last_active;
    store.create(&session).unwrap();

    std::thread::sleep(std::time::Duration::from_millis(10));
    store.touch(&session.id).unwrap();

    let retrieved = store.get(&session.id).unwrap();
    assert!(retrieved.last_active >= original_time);
}

#[test]
fn test_session_most_recent() {
    let temp_dir = TempDir::new().unwrap();
    let store = setup_session_store(&temp_dir);

    let s1 = SessionState::new(ProviderKind::Anthropic, None, None, vec![]);
    store.create(&s1).unwrap();

    std::thread::sleep(std::time::Duration::from_millis(10));

    let s2 = SessionState::new(ProviderKind::Bedrock, None, None, vec![]);
    store.create(&s2).unwrap();

    let most_recent = store.most_recent().unwrap().unwrap();
    assert_eq!(most_recent.id, s2.id);
}

#[test]
fn test_session_not_found() {
    let temp_dir = TempDir::new().unwrap();
    let store = setup_session_store(&temp_dir);

    let result = store.get("nonexistent-id");
    assert!(result.is_err());
}

#[test]
fn test_session_with_fallback_providers() {
    let temp_dir = TempDir::new().unwrap();
    let store = setup_session_store(&temp_dir);

    let session = SessionState::new(
        ProviderKind::Bedrock,
        Some("claude-opus-4-6".to_string()),
        Some("us-east-1".to_string()),
        vec![ProviderKind::Anthropic, ProviderKind::Vertex],
    );
    store.create(&session).unwrap();

    let retrieved = store.get(&session.id).unwrap();
    assert_eq!(retrieved.fallback_providers.len(), 2);
    assert_eq!(retrieved.fallback_providers[0], ProviderKind::Anthropic);
    assert_eq!(retrieved.fallback_providers[1], ProviderKind::Vertex);
}
