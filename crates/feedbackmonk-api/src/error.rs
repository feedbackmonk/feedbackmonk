//! `ApiError` -- the single error type Worker A handlers return.
//!
//! Maps repository errors + validation errors + auth failures to HTTP status
//! codes. Implements `IntoResponse` so handlers can `?` freely.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use thiserror::Error;

use feedbackmonk_core::{ResourceKind, Tier};
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

    /// P3 Stage 1 (FR-FBR-14, Contract C18): the request would exceed a
    /// tier cap. Maps to:
    /// - HTTP 409 Conflict when `resource = ResourceKind::Project`
    ///   (idiomatic for "state conflict — too many projects").
    /// - HTTP 402 Payment Required when
    ///   `resource = ResourceKind::FeedbackInRollingMonth`
    ///   (idiomatic paywall semantic).
    ///
    /// Body shape per Contract C18 mirrored verbatim:
    /// ```json
    /// {
    ///   "error": "tier_cap_exceeded",
    ///   "tier": "free" | "starter" | "pro" | "self_host",
    ///   "resource": "project" | "feedback_in_rolling_month",
    ///   "current": N,
    ///   "limit": N,
    ///   "upgrade_hint": "..."
    /// }
    /// ```
    #[error("tier cap exceeded: tier={tier:?} resource={resource:?} current={current} limit={limit}")]
    TierCapExceeded {
        tier: Tier,
        resource: ResourceKind,
        current: i64,
        limit: i64,
        upgrade_hint: String,
    },
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
            Self::TierCapExceeded { resource, .. } => match resource {
                ResourceKind::Project => StatusCode::CONFLICT,
                ResourceKind::FeedbackInRollingMonth => StatusCode::PAYMENT_REQUIRED,
            },
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
            // Tier-cap errors carry a structured body — not used by
            // the message-based path; see `into_response` below.
            Self::TierCapExceeded { .. } => "tier cap exceeded".into(),
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
        // Tier-cap errors emit a structured body per Contract C18.
        if let Self::TierCapExceeded {
            tier,
            resource,
            current,
            limit,
            upgrade_hint,
        } = &self
        {
            let body = Json(json!({
                "error": "tier_cap_exceeded",
                "tier": tier.as_db_str(),
                "resource": resource.as_wire_str(),
                "current": current,
                "limit": limit,
                "upgrade_hint": upgrade_hint,
            }));
            return (status, body).into_response();
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[tokio::test]
    async fn tier_cap_exceeded_project_maps_to_409() {
        let err = ApiError::TierCapExceeded {
            tier: Tier::Free,
            resource: ResourceKind::Project,
            current: 1,
            limit: 1,
            upgrade_hint: "Upgrade to Starter for 3 projects".into(),
        };
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let bytes = to_bytes(resp.into_body(), 4 * 1024).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"], "tier_cap_exceeded");
        assert_eq!(body["tier"], "free");
        assert_eq!(body["resource"], "project");
        assert_eq!(body["current"], 1);
        assert_eq!(body["limit"], 1);
        assert!(body["upgrade_hint"].as_str().unwrap().contains("Starter"));
    }

    #[tokio::test]
    async fn tier_cap_exceeded_feedback_maps_to_402() {
        let err = ApiError::TierCapExceeded {
            tier: Tier::Free,
            resource: ResourceKind::FeedbackInRollingMonth,
            current: 50,
            limit: 50,
            upgrade_hint: "Upgrade to Starter for 500/mo".into(),
        };
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::PAYMENT_REQUIRED);
        let bytes = to_bytes(resp.into_body(), 4 * 1024).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"], "tier_cap_exceeded");
        assert_eq!(body["resource"], "feedback_in_rolling_month");
        assert_eq!(body["current"], 50);
        assert_eq!(body["limit"], 50);
    }

    #[tokio::test]
    async fn tier_cap_exceeded_pro_tier_renders_correctly() {
        let err = ApiError::TierCapExceeded {
            tier: Tier::Pro,
            resource: ResourceKind::FeedbackInRollingMonth,
            current: 10000,
            limit: 10000,
            upgrade_hint: "Contact sales for higher volume".into(),
        };
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::PAYMENT_REQUIRED);
        let bytes = to_bytes(resp.into_body(), 4 * 1024).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["tier"], "pro");
    }
}
