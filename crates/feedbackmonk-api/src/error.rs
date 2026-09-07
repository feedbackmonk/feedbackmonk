//! `ApiError` -- the single error type Worker A handlers return.
//!
//! Maps repository errors + validation errors + auth failures to HTTP status
//! codes. Implements `IntoResponse` so handlers can `?` freely.
//!
//! ## The wire body: `error` + `code` + `message`
//!
//! Every body carries a stable machine-readable [`ApiError::code`] alongside the
//! original `error` field, plus `message` (the same human string `error` has
//! always carried). `error` is byte-identical to what shipped, so no existing
//! consumer moves; the two new fields are purely additive.
//!
//! **Why `message` as well as `code`.** The widget's `readError` accepts the
//! server's shape only when BOTH `code` and `message` are strings, and otherwise
//! synthesises `http_<status>` — so a `code` shipped on its own would still be
//! discarded and every widget error message would still be chosen by HTTP status
//! rather than by what went wrong. The pair is the smallest additive change that
//! actually reaches the client.
//!
//! **The vocabulary is chosen, not invented.** `widget/src/ui.ts::errorKey`
//! looks up `widget.error.<code>` first and falls back to a status class only
//! for a code it does not know, so a "better" name the widget has no key for is
//! WORSE than the status fallback it replaces. The codes below are spelled to
//! match the keys `i18n/locales/en/widget.json` already ships — with ONE
//! deliberate exception: `Forbidden` keeps its own `forbidden` token rather than
//! borrowing `unauthorized`, because `code` is a machine contract with external
//! consumers and merging two HTTP semantics into one token to match a client's
//! presentation would be a defect in that contract. `forbidden`, `conflict` and
//! `gone` have no widget key and render the generic line — each was verified to
//! have no widget-reachable construction site (critic finding C-005; the
//! enumeration is in `docs/planning/observations-ledger.md`). `internal` DOES
//! have one — any database failure on the submit path becomes `Internal` — so it
//! carries its own widget key restoring the wording the `http_5xx` status
//! fallback used to give.
//!
//! Bodies built by hand elsewhere (the public submit path's bare 402, the
//! rate-limit 429, the JWT 401, the attachment 415) are NOT `ApiError` and are
//! unchanged by this.

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

    /// The stable machine-readable code for this variant (see the module docs
    /// for why these particular spellings).
    ///
    /// `Unauthorized` and `Forbidden` stay **distinct** (`unauthorized` /
    /// `forbidden`) even though the widget renders both identically. `code` is a
    /// machine-readable contract with external consumers; collapsing two HTTP
    /// semantics into one token would be a defect in that contract, and how one
    /// client chooses to present them is a presentation decision that has no
    /// business reaching back into the wire vocabulary (LEAD ruling on MSG-001,
    /// item 4).
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::BadRequest(_) => "invalid_input",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not_found",
            Self::Conflict(_) => "conflict",
            Self::Gone => "gone",
            Self::PayloadTooLarge(_) => "payload_too_large",
            Self::Internal(_) => "internal",
            Self::TierCapExceeded { .. } => "tier_cap",
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
            // Tier-cap errors carry a structured body whose `error` is a machine
            // token; this prose lands in their `message` field.
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
                // Additive; every field Contract C18 specifies is untouched.
                // `message` comes from the same `body_message()` every other
                // variant uses — one rule, no per-variant judgement — which is
                // what gives the two bodies whose `error` is a machine token
                // (`tier_cap_exceeded`, `IdempotencyKeyReuse`) a prose field
                // without moving the token.
                "code": self.code(),
                "message": self.body_message(),
            }));
            return (status, body).into_response();
        }
        let message = self.body_message();
        let body = Json(json!({
            "error": message,
            "code": self.code(),
            "message": message,
        }));
        (status, body).into_response()
    }
}

impl From<RepoError> for ApiError {
    fn from(e: RepoError) -> Self {
        match e {
            RepoError::NotFound => Self::NotFound,
            RepoError::Conflict => Self::Conflict("uniqueness or state violation".into()),
            // Security scrutiny P1-3 / M6: a key reused with different content
            // → 409 with a stable machine-readable body `{"error":"IdempotencyKeyReuse"}`.
            RepoError::IdempotencyKeyReuse => Self::Conflict("IdempotencyKeyReuse".into()),
            RepoError::TenantProjectMismatch => Self::Forbidden,
            // FR-FBR-32: a `tenant_domains` row outside its CHECK set means the
            // schema and the code disagree — an operator/deploy problem, not
            // something the caller can fix, so it is a 500 rather than a 4xx.
            RepoError::DomainValue(err) => Self::Internal(format!("hosting schema drift: {err}")),
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

    async fn body_of(err: ApiError) -> (StatusCode, serde_json::Value) {
        let resp = err.into_response();
        let status = resp.status();
        let bytes = to_bytes(resp.into_body(), 4 * 1024).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn every_variant_carries_the_widget_code_vocabulary() {
        // The spellings `widget/src/ui.ts::errorKey` already keys on, plus the
        // three honest names it has no key for (they render the generic line).
        let cases = [
            (ApiError::BadRequest("subject required".into()), StatusCode::BAD_REQUEST, "invalid_input"),
            (ApiError::Unauthorized, StatusCode::UNAUTHORIZED, "unauthorized"),
            (ApiError::Forbidden, StatusCode::FORBIDDEN, "forbidden"),
            (ApiError::NotFound, StatusCode::NOT_FOUND, "not_found"),
            (ApiError::Conflict("dup".into()), StatusCode::CONFLICT, "conflict"),
            (ApiError::Gone, StatusCode::GONE, "gone"),
            (ApiError::PayloadTooLarge("too big".into()), StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large"),
            (ApiError::Internal("boom".into()), StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        ];
        for (err, want_status, want_code) in cases {
            let want_error = err.to_string();
            let (status, body) = body_of(err).await;
            assert_eq!(status, want_status, "status for {want_code}");
            assert_eq!(body["code"], want_code, "code for {want_error}");
            // `message` must be a STRING and equal to `error` — the widget's
            // readError accepts the server shape only when both are strings.
            assert!(body["message"].is_string(), "message must be a string for {want_code}");
            assert_eq!(body["message"], body["error"], "message mirrors error for {want_code}");
        }
    }

    #[tokio::test]
    async fn the_error_field_stays_byte_identical() {
        // The additive fields must not disturb what shipped: `error` carries the
        // same prose it always has, and internal details still never leak.
        let (_, body) = body_of(ApiError::BadRequest("subject required".into())).await;
        assert_eq!(body["error"], "subject required");
        let (_, body) = body_of(ApiError::NotFound).await;
        assert_eq!(body["error"], "not found");
        let (_, body) = body_of(ApiError::Internal("connection string leaked?".into())).await;
        assert_eq!(body["error"], "internal error");
        assert_eq!(body["message"], "internal error");
        assert!(!serde_json::to_string(&body).unwrap().contains("connection string"));
    }

    #[tokio::test]
    async fn tier_cap_body_gains_code_and_keeps_every_c18_field() {
        let (_, body) = body_of(ApiError::TierCapExceeded {
            tier: Tier::Free,
            resource: ResourceKind::Project,
            current: 1,
            limit: 1,
            upgrade_hint: "Upgrade to Starter for 3 projects".into(),
        })
        .await;
        for field in ["error", "tier", "resource", "current", "limit", "upgrade_hint"] {
            assert!(!body[field].is_null(), "C18 field `{field}` disappeared");
        }
        assert_eq!(body["error"], "tier_cap_exceeded");
        assert_eq!(body["code"], "tier_cap");
        assert!(body["message"].is_string());
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
