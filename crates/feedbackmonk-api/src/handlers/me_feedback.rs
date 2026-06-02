//! `GET /api/v1/projects/{project_id}/me/feedback` +
//! `GET /api/v1/projects/{project_id}/me/feedback/{fb}/thread` — the
//! end-user (JWT-`sub`-scoped) read surface (GitCellar customer-#1 parity
//! gap #4; contract `docs/integrations/gitcellar-adoption.md` §6).
//!
//! GitCellar Desktop's "My Feedback" view + tray poll consume these. Today
//! the only public end-user route is `POST …/feedback` (submit); these add
//! the read half. **No schema change** — `feedback.end_user_sub` is already
//! stored and `feedback_replies.visibility ∈ {public,internal}` already
//! exists.
//!
//! ## Auth (DEC-FBR-04)
//!
//! Both routes are JWT-only. The handler resolves the project scope via
//! `ProjectRepo::open_for_submission` (the public pre-auth boundary, same as
//! the submit handler — there is no admin session), then verifies the Bearer
//! JWT against the project's active signing keys with
//! `feedbackmonk_jwt::verify_with_leeway` (aud == project_id). The verified
//! `sub` is the ONLY identity used; every query is scoped to it.
//!
//! ## Privacy invariants (load-bearing — frozen by
//! `tests/me_feedback_isolation.rs`)
//!
//! - `/me/feedback` returns ONLY rows whose `end_user_sub == jwt.sub`. A
//!   caller never sees another user's feedback, and anonymous rows
//!   (`end_user_sub IS NULL`) are structurally excluded.
//! - `/me/feedback/{fb}/thread` returns the feedback's status + **PUBLIC
//!   replies only**. Internal replies are NEVER exposed. Requesting a
//!   feedback id that belongs to a different `sub` returns 404, not a leak.
//! - The wire shapes deliberately omit internal columns (other users' email,
//!   `external_metadata`, admin reply authorship/visibility).
//!
//! ## Error shape
//!
//! JWT verification failure → 401 with `{"error":"<JwtError variant>"}` (so
//! Desktop can disambiguate `Expired` / `WrongAudience` / … and re-mint),
//! mirroring `handlers/feedback.rs`. Missing/empty Bearer → 401
//! `{"error":"unauthorized"}`. Unknown project → 404.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{Path, Query, State};
use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use feedbackmonk_core::{FeedbackId, FeedbackKind, FeedbackStatus};
use feedbackmonk_jwt::{verify_with_leeway as jwt_verify_with_leeway, JwtError, VerifiedClaims};
use feedbackmonk_repository::ProjectScope;

use crate::error::ApiError;
use crate::state::AppState;

/// Default + max page size for the list endpoint (mirrors the admin list
/// caps in `admin_feedback.rs`).
const DEFAULT_LIST_LIMIT: u32 = 20;
const MAX_LIST_LIMIT: u32 = 100;

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct ListParams {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MeFeedbackItem {
    pub feedback_id: String,
    pub kind: FeedbackKind,
    pub status: FeedbackStatus,
    pub body: String,
    pub submitted_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MeListResponse {
    pub items: Vec<MeFeedbackItem>,
    pub total: u32,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct MeReplyItem {
    pub reply_id: Uuid,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MeThreadResponse {
    pub feedback_id: String,
    pub kind: FeedbackKind,
    pub status: FeedbackStatus,
    pub body: String,
    pub submitted_at: DateTime<Utc>,
    /// Public replies only, chronological. Internal replies never appear.
    pub replies: Vec<MeReplyItem>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /api/v1/projects/{project_id}/me/feedback` — paginated list of the
/// caller's own feedback (newest-first).
pub async fn list_my_feedback(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Query(params): Query<ListParams>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let (scope, claims) = match authenticate(&state, project_id, &headers).await {
        Ok(v) => v,
        Err(resp) => return Ok(resp),
    };

    let limit = params.limit.unwrap_or(DEFAULT_LIST_LIMIT).min(MAX_LIST_LIMIT);
    let offset = params.offset.unwrap_or(0);

    let (rows, total) = state
        .feedback
        .list_for_end_user(&scope, &claims.sub, limit, offset)
        .await?;

    let items = rows
        .into_iter()
        .map(|f| MeFeedbackItem {
            feedback_id: f.feedback_id.as_str().to_string(),
            kind: f.kind,
            status: f.status,
            body: f.body,
            submitted_at: f.submitted_at,
        })
        .collect();

    Ok((
        StatusCode::OK,
        Json(MeListResponse {
            items,
            total,
            limit,
            offset,
        }),
    )
        .into_response())
}

/// `GET /api/v1/projects/{project_id}/me/feedback/{fb}/thread` — the
/// caller's feedback status + PUBLIC replies only.
pub async fn my_feedback_thread(
    State(state): State<AppState>,
    Path((project_id, feedback_id)): Path<(Uuid, String)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let (scope, claims) = match authenticate(&state, project_id, &headers).await {
        Ok(v) => v,
        Err(resp) => return Ok(resp),
    };

    let fb_id = FeedbackId::from(feedback_id);

    // Scoped to the caller's sub: a feedback id owned by a different user (or
    // anonymous) returns NotFound → 404, never another user's thread.
    let fb = state
        .feedback
        .get_for_end_user(&scope, &claims.sub, &fb_id)
        .await?;

    let replies = state
        .feedback_replies
        .list_public_for_feedback(&scope, &fb_id)
        .await?;

    let replies = replies
        .into_iter()
        .map(|r| MeReplyItem {
            reply_id: r.id,
            body: r.body,
            created_at: r.created_at,
        })
        .collect();

    Ok((
        StatusCode::OK,
        Json(MeThreadResponse {
            feedback_id: fb.feedback_id.as_str().to_string(),
            kind: fb.kind,
            status: fb.status,
            body: fb.body,
            submitted_at: fb.submitted_at,
            replies,
        }),
    )
        .into_response())
}

// ---------------------------------------------------------------------------
// Auth helper
// ---------------------------------------------------------------------------

/// Resolve the project scope + verify the Bearer JWT for `project_id`.
///
/// On success returns `(ProjectScope, VerifiedClaims)`. On failure returns a
/// ready-to-send error `Response`:
///   - unknown project → 404 (`ApiError::NotFound`),
///   - missing/empty Bearer → 401 `{"error":"unauthorized"}`,
///   - JWT verification failure → 401 `{"error":"<JwtError variant>"}`.
///
/// Order matches `handlers/feedback.rs`: project scope first (so an unknown
/// project 404s before any auth work), then token presence, then verify.
async fn authenticate(
    state: &AppState,
    project_id: Uuid,
    headers: &HeaderMap,
) -> Result<(ProjectScope, VerifiedClaims), Response> {
    let scope = state
        .projects
        .open_for_submission(project_id)
        .await
        .map_err(|e| ApiError::from(e).into_response())?;

    let token = extract_bearer(headers).ok_or_else(|| ApiError::Unauthorized.into_response())?;

    let active_keys = state
        .signing_keys
        .list_active(&scope)
        .await
        .map_err(|e| ApiError::from(e).into_response())?;

    let now_unix = current_unix_timestamp();
    let claims = jwt_verify_with_leeway(
        &token,
        project_id,
        &active_keys,
        now_unix,
        state.jwt_iat_leeway_seconds,
    )
    .map_err(|e| jwt_error_response(&e))?;

    Ok((scope, claims))
}

/// Read a non-empty `Authorization: Bearer <token>`. Local copy of the
/// submit handler's helper (kept private there); duplicated rather than
/// widening `feedback.rs`'s surface during parallel work.
fn extract_bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let stripped = value.strip_prefix("Bearer ")?;
    if stripped.is_empty() {
        return None;
    }
    Some(stripped.to_string())
}

/// 401 with `{"error":"<JwtError variant>"}` — same body shape the submit
/// handler emits so integrations disambiguate consistently.
fn jwt_error_response(err: &JwtError) -> Response {
    let body = Json(json!({ "error": err.variant_name() }));
    (StatusCode::UNAUTHORIZED, body).into_response()
}

fn current_unix_timestamp() -> i64 {
    #[allow(clippy::cast_possible_wrap)]
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Router subtree — merged into the binary by `main::build_app`
// ---------------------------------------------------------------------------

/// Gap #4 end-user read subtree. Exposed for `main.rs` to `.merge()` and for
/// the isolation fixture to mount in isolation.
pub fn me_feedback_router(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/v1/projects/:project_id/me/feedback",
            get(list_my_feedback),
        )
        .route(
            "/api/v1/projects/:project_id/me/feedback/:feedback_id/thread",
            get(my_feedback_thread),
        )
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn hdr(name: &str, value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(
            axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
            HeaderValue::from_str(value).unwrap(),
        );
        h
    }

    #[test]
    fn extract_bearer_present() {
        let h = hdr("Authorization", "Bearer abc.def.ghi");
        assert_eq!(extract_bearer(&h).as_deref(), Some("abc.def.ghi"));
    }

    #[test]
    fn extract_bearer_missing_and_empty() {
        assert_eq!(extract_bearer(&HeaderMap::new()), None);
        assert_eq!(extract_bearer(&hdr("Authorization", "Bearer ")), None);
        assert_eq!(extract_bearer(&hdr("Authorization", "abc")), None);
    }

    #[test]
    fn jwt_error_response_carries_variant_name() {
        let resp = jwt_error_response(&JwtError::WrongAudience);
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
