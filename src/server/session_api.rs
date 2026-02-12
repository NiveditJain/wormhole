use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::debug;

use crate::config::credentials::resolve_credentials;
use crate::config::types::ProviderCredentials;
use crate::error::AppError;
use crate::provider::anthropic::AnthropicProvider;
use crate::provider::bedrock::BedrockProvider;
use crate::provider::failover::FailoverProvider;
use crate::provider::foundry::FoundryProvider;
use crate::provider::vertex::VertexProvider;
use crate::provider::Provider;
use crate::server::state::AppState;
use crate::session::types::SessionState;
use crate::types::ProviderKind;

#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    pub provider: ProviderKind,
    pub model: Option<String>,
    pub fallback_providers: Option<Vec<ProviderKind>>,
}

#[derive(Debug, Serialize)]
pub struct SessionResponse {
    pub id: String,
    pub provider: ProviderKind,
    pub model: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct SessionListResponse {
    pub sessions: Vec<SessionResponse>,
}

/// POST /v1/sessions - Create a new session with a specific provider
pub async fn create_session(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<SessionResponse>), AppError> {
    debug!("Creating session for provider {:?}", req.provider);

    // Resolve credentials and build provider
    let credentials = resolve_credentials(req.provider, &state.config.providers)
        .await
        .map_err(|e| AppError::Config(e.to_string()))?;

    let provider: Arc<dyn Provider> = build_provider(req.provider, credentials)?;

    // Wrap with failover if fallbacks are specified
    let provider = if let Some(ref fallback_kinds) = req.fallback_providers {
        if !fallback_kinds.is_empty() {
            let mut fallbacks = Vec::new();
            for kind in fallback_kinds {
                let creds = resolve_credentials(*kind, &state.config.providers)
                    .await
                    .map_err(|e| AppError::Config(e.to_string()))?;
                let fb_provider = build_provider(*kind, creds)?;
                fallbacks.push(fb_provider);
            }

            Arc::new(FailoverProvider::new(
                provider,
                fallbacks,
                state.config.failover.max_retries,
                state.config.failover.retry_on.clone(),
                state.config.failover.backoff_base_ms,
            )) as Arc<dyn Provider>
        } else {
            provider
        }
    } else {
        provider
    };

    // Create session state
    let session = SessionState::new(
        req.provider,
        req.model.clone(),
        None,
        req.fallback_providers.clone().unwrap_or_default(),
    );

    let session_id = session.id.clone();
    let created_at = session.created_at.to_rfc3339();

    // Store in memory
    state.add_session(session_id.clone(), provider);

    let response = SessionResponse {
        id: session_id,
        provider: req.provider,
        model: req.model,
        created_at,
    };

    Ok((StatusCode::CREATED, Json(response)))
}

/// GET /v1/sessions - List all active sessions
pub async fn list_sessions(
    State(state): State<Arc<AppState>>,
) -> Json<SessionListResponse> {
    let sessions: Vec<SessionResponse> = state
        .sessions
        .iter()
        .map(|entry| SessionResponse {
            id: entry.key().clone(),
            provider: entry.value().kind(),
            model: None,
            created_at: String::new(),
        })
        .collect();

    Json(SessionListResponse { sessions })
}

/// GET /v1/sessions/:id - Get session details
pub async fn get_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<SessionResponse>, AppError> {
    let provider = state
        .sessions
        .get(&id)
        .ok_or_else(|| {
            AppError::Session(crate::error::SessionError::NotFound(format!(
                "Session '{}' not found",
                id
            )))
        })?;

    Ok(Json(SessionResponse {
        id: id.clone(),
        provider: provider.value().kind(),
        model: None,
        created_at: String::new(),
    }))
}

/// DELETE /v1/sessions/:id - Delete a session
pub async fn delete_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    state.remove_session(&id).ok_or_else(|| {
        AppError::Session(crate::error::SessionError::NotFound(format!(
            "Session '{}' not found",
            id
        )))
    })?;

    Ok(StatusCode::NO_CONTENT)
}

fn build_provider(
    kind: ProviderKind,
    credentials: ProviderCredentials,
) -> Result<Arc<dyn Provider>, AppError> {
    match (kind, credentials) {
        (ProviderKind::Anthropic, ProviderCredentials::Anthropic { api_key, base_url }) => {
            Ok(Arc::new(AnthropicProvider::new(api_key, base_url)))
        }
        (ProviderKind::Bedrock, ProviderCredentials::Bedrock { region, credentials }) => {
            Ok(Arc::new(BedrockProvider::new(credentials, region)))
        }
        (ProviderKind::Vertex, ProviderCredentials::Vertex { project_id, region, token }) => {
            Ok(Arc::new(VertexProvider::new(project_id, region, token)))
        }
        (ProviderKind::Foundry, ProviderCredentials::Foundry { resource, api_key }) => {
            Ok(Arc::new(FoundryProvider::new(resource, api_key)))
        }
        _ => Err(AppError::Config(format!(
            "Credential type mismatch for provider {}",
            kind
        ))),
    }
}
