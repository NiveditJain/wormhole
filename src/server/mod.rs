pub mod handlers;
pub mod session_api;
pub mod state;

use axum::routing::{delete, get, post};
use axum::Router;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::server::state::AppState;

pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        // Core Anthropic API endpoints
        .route("/v1/messages", post(handlers::messages))
        .route(
            "/v1/messages/count_tokens",
            post(handlers::count_tokens),
        )
        .route("/v1/models", get(handlers::list_models))
        // Session management API (daemon mode)
        .route("/v1/sessions", post(session_api::create_session))
        .route("/v1/sessions", get(session_api::list_sessions))
        .route("/v1/sessions/{id}", get(session_api::get_session))
        .route("/v1/sessions/{id}", delete(session_api::delete_session))
        // Health check
        .route("/health", get(handlers::health))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

pub async fn start_server(
    state: Arc<AppState>,
    host: &str,
    port: u16,
    shutdown: CancellationToken,
) -> anyhow::Result<(String, u16)> {
    let router = build_router(state);

    let addr = format!("{}:{}", host, port);
    let listener = TcpListener::bind(&addr).await?;
    let local_addr = listener.local_addr()?;
    let actual_port = local_addr.port();
    let actual_host = local_addr.ip().to_string();

    info!("Proxy server listening on {}:{}", actual_host, actual_port);

    let shutdown_signal = shutdown.clone();
    tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                shutdown_signal.cancelled().await;
            })
            .await
            .ok();
    });

    Ok((actual_host, actual_port))
}
