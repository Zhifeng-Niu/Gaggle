//! Gaggle统一错误类型

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

/// Gaggle错误类型
#[derive(Error, Debug)]
pub enum GaggleError {
    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Space not found: {0}")]
    SpaceNotFound(String),

    #[error("Space closed: {0}")]
    SpaceClosed(String),

    #[error("Invalid message type: {0}")]
    InvalidMessageType(String),

    #[error("Encryption error: {0}")]
    EncryptionError(String),

    #[error("Solana error: {0}")]
    SolanaError(String),

    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("WebSocket error: {0}")]
    WebSocketError(String),
}

impl IntoResponse for GaggleError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            GaggleError::Unauthorized(msg) => {
                (StatusCode::UNAUTHORIZED, "UNAUTHORIZED", msg.clone())
            }
            GaggleError::Forbidden(msg) => (StatusCode::FORBIDDEN, "FORBIDDEN", msg.clone()),
            GaggleError::SpaceNotFound(msg) => {
                (StatusCode::NOT_FOUND, "SPACE_NOT_FOUND", msg.clone())
            }
            GaggleError::SpaceClosed(msg) => (StatusCode::BAD_REQUEST, "SPACE_CLOSED", msg.clone()),
            GaggleError::InvalidMessageType(msg) => {
                (StatusCode::BAD_REQUEST, "INVALID_MESSAGE_TYPE", msg.clone())
            }
            GaggleError::EncryptionError(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "ENCRYPTION_ERROR",
                msg.clone(),
            ),
            GaggleError::SolanaError(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "SOLANA_ERROR",
                msg.clone(),
            ),
            GaggleError::DatabaseError(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                msg.clone(),
            ),
            GaggleError::ValidationError(msg) => {
                (StatusCode::BAD_REQUEST, "VALIDATION_ERROR", msg.clone())
            }
            GaggleError::NotFound(msg) => (StatusCode::NOT_FOUND, "NOT_FOUND", msg.clone()),
            GaggleError::Internal(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                msg.clone(),
            ),
            GaggleError::WebSocketError(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "WEBSOCKET_ERROR",
                msg.clone(),
            ),
        };

        let body = Json(json!({
            "error": {
                "code": code,
                "message": message,
            }
        }));

        (status, body).into_response()
    }
}

impl From<rusqlite::Error> for GaggleError {
    fn from(err: rusqlite::Error) -> Self {
        GaggleError::DatabaseError(err.to_string())
    }
}

impl From<aes_gcm::Error> for GaggleError {
    fn from(err: aes_gcm::Error) -> Self {
        GaggleError::EncryptionError(err.to_string())
    }
}

impl From<serde_json::Error> for GaggleError {
    fn from(err: serde_json::Error) -> Self {
        GaggleError::ValidationError(err.to_string())
    }
}
