use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

use crate::adapters::AdapterError;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Validation(String),
    #[error("{0} not found")]
    NotFound(&'static str),
    #[error("{0}")]
    Conflict(String),
    #[error("quote has expired")]
    QuoteExpired,
    #[error("unauthorized")]
    Unauthorized,
    #[error("invalid webhook signature")]
    InvalidSignature,
    #[error(transparent)]
    Adapter(#[from] AdapterError),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        AppError::Internal(err.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            AppError::Validation(_) => (StatusCode::BAD_REQUEST, "validation_error"),
            AppError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            AppError::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            AppError::QuoteExpired => (StatusCode::GONE, "quote_expired"),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            AppError::InvalidSignature => (StatusCode::UNAUTHORIZED, "invalid_signature"),
            AppError::Adapter(AdapterError::InvalidRecipient(_)) => {
                (StatusCode::UNPROCESSABLE_ENTITY, "invalid_recipient")
            }
            AppError::Adapter(AdapterError::RecipientNotFound) => {
                (StatusCode::UNPROCESSABLE_ENTITY, "recipient_not_found")
            }
            AppError::Adapter(AdapterError::Unavailable(_)) => {
                (StatusCode::SERVICE_UNAVAILABLE, "provider_unavailable")
            }
            AppError::Adapter(AdapterError::Rejected(_)) => (StatusCode::BAD_GATEWAY, "provider_rejected"),
            AppError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        };
        let message = match &self {
            AppError::Internal(err) => {
                tracing::error!(error = ?err, "internal error");
                "internal server error".to_string()
            }
            other => other.to_string(),
        };
        (status, Json(json!({ "error": { "code": code, "message": message } }))).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;
