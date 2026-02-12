use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use thiserror::Error;

use crate::types::ProviderKind;

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("HTTP error from {provider}: {status} - {message}")]
    Upstream {
        provider: ProviderKind,
        status: u16,
        message: String,
        body: Option<String>,
    },

    #[error("Connection error to {provider}: {source}")]
    Connection {
        provider: ProviderKind,
        #[source]
        source: reqwest::Error,
    },

    #[error("Authentication error for {provider}: {message}")]
    Auth {
        provider: ProviderKind,
        message: String,
    },

    #[error("Request transformation error: {0}")]
    Transform(String),

    #[error("Stream decoding error: {0}")]
    StreamDecode(String),

    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

impl ProviderError {
    pub fn status_code(&self) -> u16 {
        match self {
            ProviderError::Upstream { status, .. } => *status,
            ProviderError::Connection { .. } => 502,
            ProviderError::Auth { .. } => 401,
            ProviderError::Transform(_) => 400,
            ProviderError::StreamDecode(_) => 502,
            ProviderError::Other(_) => 500,
        }
    }

    #[allow(dead_code)]
    pub fn is_retryable(&self) -> bool {
        match self {
            ProviderError::Upstream { status, .. } => {
                matches!(status, 429 | 500 | 502 | 503 | 529)
            }
            ProviderError::Connection { .. } => true,
            _ => false,
        }
    }
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("Session not found: {0}")]
    NotFound(String),

    #[error("Session storage error: {0}")]
    Storage(#[from] std::io::Error),

    #[error("Session serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Provider(#[from] ProviderError),

    #[error(transparent)]
    Session(#[from] SessionError),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_type, message) = match &self {
            AppError::Provider(e) => {
                let status = StatusCode::from_u16(e.status_code())
                    .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                let error_type = match e {
                    ProviderError::Auth { .. } => "authentication_error",
                    ProviderError::Upstream { status, .. } if *status == 429 => {
                        "rate_limit_error"
                    }
                    ProviderError::Upstream { status, .. } if *status >= 500 => "api_error",
                    ProviderError::Upstream { .. } => "invalid_request_error",
                    ProviderError::Connection { .. } => "api_error",
                    ProviderError::Transform(_) => "invalid_request_error",
                    ProviderError::StreamDecode(_) => "api_error",
                    ProviderError::Other(_) => "api_error",
                };
                (status, error_type, e.to_string())
            }
            AppError::Session(e) => {
                let status = match e {
                    SessionError::NotFound(_) => StatusCode::NOT_FOUND,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                ("api_error", "api_error", e.to_string());
                (status, "not_found_error", e.to_string())
            }
            AppError::Config(msg) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "api_error", msg.clone())
            }
            AppError::Internal(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "api_error",
                e.to_string(),
            ),
        };

        let body = json!({
            "type": "error",
            "error": {
                "type": error_type,
                "message": message,
            }
        });

        (status, axum::Json(body)).into_response()
    }
}
