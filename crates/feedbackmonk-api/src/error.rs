//! `ApiError` -- the single error type Worker A handlers return.
//!
//! Maps repository errors + validation errors + auth failures to HTTP status
//! codes. Implements `IntoResponse` so handlers can `?` freely.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use thiserror::Error;

use feedbackmonk_repository::RepoError;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("unauthorized")]
    Unauthorized,

    #[error("forbidden")]
    Forbidden,

    #[error("not found")]
    NotFound,

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("gone")]
    Gone,

    #[error("payload too large: {0}")]
    PayloadTooLarge(String),

    #[error("internal: {0}")]
    Internal(String),
}

impl ApiError {
    fn status(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Gone => StatusCode::GONE,
            Self::PayloadTooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn body_message(&self) -> String {
        match self {
            Self::BadRequest(m) | Self::Conflict(m) | Self::PayloadTooLarge(m) => m.clone(),
            Self::Unauthorized => "unauthorized".into(),
            Self::Forbidden => "forbidden".into(),
            Self::NotFound => "not found".into(),
            Self::Gone => "verification token expired".into(),
            // Don't leak internal details to clients.
            Self::Internal(_) => "internal error".into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        if matches!(self, Self::Internal(_)) {
            tracing::error!(error = %self, "internal error");
        } else {
            tracing::warn!(error = %self, "api error");
        }
        let body = Json(json!({ "error": self.body_message() }));
        (status, body).into_response()
    }
}

impl From<RepoError> for ApiError {
    fn from(e: RepoError) -> Self {
        match e {
            RepoError::NotFound => Self::NotFound,
            RepoError::Conflict => Self::Conflict("uniqueness or state violation".into()),
            RepoError::TenantProjectMismatch => Self::Forbidden,
            RepoError::Sqlx(err) => Self::Internal(format!("database error: {err}")),
        }
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(format!("database error: {e}"))
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        Self::Internal(e.to_string())
    }
}
