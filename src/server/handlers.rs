use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures_util::StreamExt;
use std::sync::Arc;
use tracing::{debug, error};

use crate::error::AppError;
use crate::server::state::AppState;

/// Resolve the provider for the current request.
/// Uses X-Wormhole-Session header if present, otherwise default provider.
fn resolve_provider(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Arc<dyn crate::provider::Provider>, AppError> {
    let session_id = headers
        .get("x-wormhole-session")
        .and_then(|v| v.to_str().ok());

    state
        .get_provider(session_id)
        .ok_or_else(|| {
            if let Some(id) = session_id {
                AppError::Session(crate::error::SessionError::NotFound(format!(
                    "Session '{}' not found",
                    id
                )))
            } else {
                AppError::Config("No default provider configured".to_string())
            }
        })
}

/// POST /v1/messages
pub async fn messages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Response, AppError> {
    let provider = resolve_provider(&state, &headers)?;

    let is_streaming = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    debug!(
        provider = %provider.kind(),
        streaming = is_streaming,
        "Handling /v1/messages"
    );

    if is_streaming {
        let stream = provider.messages_stream(body, &headers).await?;

        // Convert the provider stream to a response body
        let body_stream = stream.map(|result| {
            result.map_err(|e| {
                error!("Stream error: {}", e);
                std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
            })
        });

        let body = Body::from_stream(body_stream);

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .header("cache-control", "no-cache")
            .header("connection", "keep-alive")
            .header("x-accel-buffering", "no")
            .body(body)
            .unwrap())
    } else {
        let result = provider.messages(body, &headers).await?;
        Ok(Json(result).into_response())
    }
}

/// POST /v1/messages/count_tokens
pub async fn count_tokens(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let provider = resolve_provider(&state, &headers)?;

    debug!(
        provider = %provider.kind(),
        "Handling /v1/messages/count_tokens"
    );

    let result = provider.count_tokens(body, &headers).await?;
    Ok(Json(result))
}

/// GET /v1/models
pub async fn list_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let provider = resolve_provider(&state, &headers)?;

    debug!(
        provider = %provider.kind(),
        "Handling /v1/models"
    );

    let result = provider.list_models().await?;
    Ok(Json(result))
}

/// GET /health
pub async fn health() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}
