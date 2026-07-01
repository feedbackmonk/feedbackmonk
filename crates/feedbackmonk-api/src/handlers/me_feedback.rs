//! The end-user (JWT-`sub`-scoped) surface (GitCellar customer-#1 parity
//! gap #4 + Phase A A1/A3/A5; contract
//! `docs/integrations/gitcellar-adoption.md` §6):
//!
//! - `GET    /api/v1/projects/{project_id}/me/feedback` (list; A3 adds
//!   `updated_at` + `reply_count` + `severity` fields and `?since=` delta
//!   polling)
//! - `GET    /api/v1/projects/{project_id}/me/feedback/{fb}/thread`
//! - `DELETE /api/v1/projects/{project_id}/me/feedback/{fb}` (A1 — P0
//!   right-to-erasure: object-store byte purge THEN row delete + FK cascade)
//! - `GET    /api/v1/projects/{project_id}/me/feedback/export` (A5 — GDPR
//!   portability companion to A1: the caller's complete data as one JSON doc)
//!
//! GitCellar Desktop's "My Feedback" view + tray poll consume these. The
//! list/thread routes run on [`AppState`]; delete/export need the attachment
//! repo + object store (which `AppState` does not hold), so they run on the
//! dedicated [`MeFeedbackDataState`] — mirroring the `AttachmentState`
//! pattern, zero edits to the many `AppState { … }` construction sites.
//!
//! ## Auth (DEC-FBR-04)
//!
//! Both routes are JWT-only. The handler resolves the project scope via
//! `ProjectRepo::open_for_submission` (the public pre-auth boundary, same as
//! the submit handler — there is no admin session), then verifies the Bearer
//! JWT against the project's active signing keys with
//! `feedbackmonk_jwt::verify_with_leeway` (aud == `project_id`). The verified
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

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{Path, Query, State};
use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use feedbackmonk_core::{FeedbackId, FeedbackKind, FeedbackStatus, KeyClass, Sentiment, Severity};
use feedbackmonk_jwt::{verify_with_leeway as jwt_verify_with_leeway, JwtError, VerifiedClaims};
use feedbackmonk_repository::{
    AttachmentRepo, FeedbackReplyRepo, FeedbackRepo, ProjectRepo, ProjectScope, RepoError,
    SigningKeyRepo,
};

use crate::error::ApiError;
use crate::state::AppState;
use crate::storage::ObjectStore;

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
    /// Phase A A3: RFC3339 delta-poll cursor — only rows with
    /// `updated_at > since` are returned. Absent ⇒ full list (v1 behavior).
    pub since: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MeFeedbackItem {
    pub feedback_id: String,
    pub kind: FeedbackKind,
    pub status: FeedbackStatus,
    pub body: String,
    /// The submitter's own sentiment, if given (FR-FBR-28).
    pub sentiment: Option<Sentiment>,
    /// Optional 4-point impact signal (Phase A A4). `null` when not supplied.
    pub severity: Option<Severity>,
    pub submitted_at: DateTime<Utc>,
    /// Phase A A3: `greatest(submitted_at, latest PUBLIC reply, latest status
    /// transition)` — internal replies never move this.
    pub updated_at: DateTime<Utc>,
    /// Phase A A3: count of PUBLIC replies only.
    pub reply_count: i64,
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
    /// The submitter's own sentiment, if given (FR-FBR-28).
    pub sentiment: Option<Sentiment>,
    /// Optional 4-point impact signal (Phase A A4). `null` when not supplied.
    pub severity: Option<Severity>,
    pub submitted_at: DateTime<Utc>,
    /// Phase A A3: same derived timestamp as the list items.
    pub updated_at: DateTime<Utc>,
    /// Phase A A3: count of PUBLIC replies only (== `replies.len()` here).
    pub reply_count: i64,
    /// Public replies only, chronological. Internal replies never appear.
    pub replies: Vec<MeReplyItem>,
}

/// One attachment's metadata inside the export document (A5). Mirrors the
/// upload response's fields plus `content_type`/`byte_size`/`created_at`.
/// NEVER includes `storage_key` (internal object-store addressing).
#[derive(Debug, Clone, Serialize)]
pub struct ExportAttachment {
    pub attachment_id: Uuid,
    pub kind: &'static str,
    pub url: String,
    pub content_type: String,
    pub byte_size: i64,
    pub created_at: DateTime<Utc>,
}

/// One feedback row inside the export document (A5): the thread shape plus
/// attachment metadata.
#[derive(Debug, Clone, Serialize)]
pub struct ExportFeedbackItem {
    pub feedback_id: String,
    pub kind: FeedbackKind,
    pub status: FeedbackStatus,
    pub body: String,
    pub sentiment: Option<Sentiment>,
    pub severity: Option<Severity>,
    pub submitted_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub reply_count: i64,
    /// Public replies only — internal triage notes are NEVER exported.
    pub replies: Vec<MeReplyItem>,
    pub attachments: Vec<ExportAttachment>,
}

/// `GET …/me/feedback/export` response (A5): the caller's COMPLETE feedback
/// as one JSON document (GDPR portability companion to the A1 erasure route).
#[derive(Debug, Clone, Serialize)]
pub struct ExportResponse {
    pub project_id: Uuid,
    pub exported_at: DateTime<Utc>,
    pub feedback: Vec<ExportFeedbackItem>,
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
        .list_for_end_user(&scope, &claims.sub, limit, offset, params.since)
        .await?;

    let items = rows
        .into_iter()
        .map(|f| MeFeedbackItem {
            feedback_id: f.feedback_id.as_str().to_string(),
            kind: f.kind,
            status: f.status,
            body: f.body,
            sentiment: f.sentiment,
            severity: f.severity,
            submitted_at: f.submitted_at,
            updated_at: f.updated_at,
            reply_count: f.reply_count,
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
            sentiment: fb.sentiment,
            severity: fb.severity,
            submitted_at: fb.submitted_at,
            updated_at: fb.updated_at,
            reply_count: fb.reply_count,
            replies,
        }),
    )
        .into_response())
}

// ---------------------------------------------------------------------------
// Phase A A1 + A5 — erasure + portability (MeFeedbackDataState routes)
// ---------------------------------------------------------------------------

/// State for the delete/export sub-router. Intentionally SEPARATE from the
/// global [`AppState`] — these routes need the attachment repo + object store,
/// which `AppState` does not hold, and adding fields there would ripple
/// through every `AppState { … }` construction site across the test suite.
/// Mirrors the `AttachmentState` pattern (`handlers/attachments.rs`). Built in
/// `main.rs` from the existing repo handles + the env-selected object store.
#[derive(Clone)]
pub struct MeFeedbackDataState {
    /// For `open_for_submission` (pre-auth project-scope mint, DEC-FBR-04).
    pub projects: Arc<dyn ProjectRepo>,
    /// Identity-class key lookup for end-user JWT verification (C25).
    pub signing_keys: Arc<dyn SigningKeyRepo>,
    /// End-user-scoped feedback reads + the A1 `delete_for_end_user` erasure.
    pub feedback: Arc<dyn FeedbackRepo>,
    /// PUBLIC-only reply reads for the export document.
    pub feedback_replies: Arc<dyn FeedbackReplyRepo>,
    /// Attachment metadata (storage keys for the byte purge; export metadata).
    pub attachments: Arc<dyn AttachmentRepo>,
    /// Object store holding attachment bytes (`LocalFs` / S3-compatible).
    pub storage: Arc<dyn ObjectStore>,
    /// `iat` clock-skew tolerance — same value `AppState` carries.
    pub jwt_iat_leeway_seconds: i64,
}

/// `DELETE /api/v1/projects/{project_id}/me/feedback/{fb}` — P0
/// right-to-erasure (Phase A A1, D-A1: hard-delete + FK cascade + explicit
/// object-store byte purge).
///
/// Erasure order is load-bearing:
///   1. resolve ownership (`claims.sub` scoped — 404 otherwise, never a leak),
///   2. fetch all attachment `storage_key`s,
///   3. purge each object's BYTES via `ObjectStore::delete` (idempotent),
///   4. THEN delete the feedback ROW (FK cascade removes attachment rows,
///      replies, status history, moderation events, board votes and the
///      submit-idempotency record; roadmap `origin_feedback_id` +
///      `duplicate_of_feedback_id` are set NULL).
///
/// Bytes-before-row: a mid-failure leaves DB rows (the client retries the
/// DELETE) rather than orphaned object bytes nothing points at.
///
/// `204 No Content` on success; a second delete of the same id → `404` (row
/// already gone).
pub async fn delete_my_feedback(
    State(state): State<MeFeedbackDataState>,
    Path((project_id, feedback_id)): Path<(Uuid, String)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let (scope, claims) = match authenticate_parts(
        state.projects.as_ref(),
        state.signing_keys.as_ref(),
        state.jwt_iat_leeway_seconds,
        project_id,
        &headers,
    )
    .await
    {
        Ok(v) => v,
        Err(resp) => return Ok(resp),
    };

    let fb_id = FeedbackId::from(feedback_id);

    // Ownership gate FIRST (DEC-FBR-04): a feedback id not owned by the
    // caller's sub (or anonymous / cross-project) is 404 — never an existence
    // oracle, and no purge work happens for un-owned rows.
    state
        .feedback
        .get_for_end_user(&scope, &claims.sub, &fb_id)
        .await?;

    // Byte purge BEFORE row delete (see the handler doc). `ObjectStore::delete`
    // is idempotent, so a retry after a partial failure re-deletes cleanly.
    let fb_uuid = state
        .attachments
        .resolve_feedback_uuid(&scope, fb_id.as_str())
        .await?;
    let keys = state
        .attachments
        .list_storage_keys_for_feedback(&scope, fb_uuid)
        .await?;
    for key in &keys {
        state.storage.delete(key).await.map_err(|e| {
            ApiError::Internal(format!("attachment byte purge failed for {key}: {e}"))
        })?;
    }

    // Row delete + FK cascade. Sub-scoped again inside the DELETE itself, so
    // the ownership check above is not a TOCTOU hole.
    let deleted = state
        .feedback
        .delete_for_end_user(&scope, &claims.sub, &fb_id)
        .await?;
    if !deleted {
        return Err(ApiError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `GET /api/v1/projects/{project_id}/me/feedback/export` — GDPR data
/// portability (Phase A A5, D-A5). Returns the caller's COMPLETE feedback as
/// one JSON document: every owned row with its public replies and attachment
/// metadata. No pagination (it's an export). Same privacy posture as
/// list/thread: own-sub rows only, PUBLIC replies only.
pub async fn export_my_feedback(
    State(state): State<MeFeedbackDataState>,
    Path(project_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let (scope, claims) = match authenticate_parts(
        state.projects.as_ref(),
        state.signing_keys.as_ref(),
        state.jwt_iat_leeway_seconds,
        project_id,
        &headers,
    )
    .await
    {
        Ok(v) => v,
        Err(resp) => return Ok(resp),
    };

    // All rows, newest-first (u32::MAX ⇒ effectively unpaginated).
    let (rows, _total) = state
        .feedback
        .list_for_end_user(&scope, &claims.sub, u32::MAX, 0, None)
        .await?;

    let mut feedback = Vec::with_capacity(rows.len());
    for f in rows {
        let replies = state
            .feedback_replies
            .list_public_for_feedback(&scope, &f.feedback_id)
            .await?
            .into_iter()
            .map(|r| MeReplyItem {
                reply_id: r.id,
                body: r.body,
                created_at: r.created_at,
            })
            .collect();

        // Attachment metadata for this row. A concurrent erasure between the
        // list above and this resolve surfaces as NotFound — degrade to an
        // empty attachment set rather than failing the whole export.
        let attachments = match state
            .attachments
            .resolve_feedback_uuid(&scope, f.feedback_id.as_str())
            .await
        {
            Ok(fb_uuid) => state
                .attachments
                .list_for_feedback(&scope, fb_uuid)
                .await?
                .into_iter()
                .map(|a| ExportAttachment {
                    attachment_id: a.id,
                    kind: a.kind.as_str(),
                    url: a.url,
                    content_type: a.content_type,
                    byte_size: a.byte_size,
                    created_at: a.created_at,
                })
                .collect(),
            Err(RepoError::NotFound) => Vec::new(),
            Err(e) => return Err(e.into()),
        };

        feedback.push(ExportFeedbackItem {
            feedback_id: f.feedback_id.as_str().to_string(),
            kind: f.kind,
            status: f.status,
            body: f.body,
            sentiment: f.sentiment,
            severity: f.severity,
            submitted_at: f.submitted_at,
            updated_at: f.updated_at,
            reply_count: f.reply_count,
            replies,
            attachments,
        });
    }

    Ok((
        StatusCode::OK,
        Json(ExportResponse {
            project_id,
            exported_at: Utc::now(),
            feedback,
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
    authenticate_parts(
        state.projects.as_ref(),
        state.signing_keys.as_ref(),
        state.jwt_iat_leeway_seconds,
        project_id,
        headers,
    )
    .await
}

/// Repo-handle-level body of [`authenticate`], shared by the `AppState`
/// (list/thread) and [`MeFeedbackDataState`] (delete/export) routes so the
/// auth chain is implemented exactly once.
async fn authenticate_parts(
    projects: &dyn ProjectRepo,
    signing_keys: &dyn SigningKeyRepo,
    jwt_iat_leeway_seconds: i64,
    project_id: Uuid,
    headers: &HeaderMap,
) -> Result<(ProjectScope, VerifiedClaims), Response> {
    let scope = projects
        .open_for_submission(project_id)
        .await
        .map_err(|e| ApiError::from(e).into_response())?;

    let token = extract_bearer(headers).ok_or_else(|| ApiError::Unauthorized.into_response())?;

    // P5b (C25): identity-class keys only for end-user JWT verification.
    let active_keys = signing_keys
        .list_active_for_class(&scope, KeyClass::Identity)
        .await
        .map_err(|e| ApiError::from(e).into_response())?;

    let now_unix = current_unix_timestamp();
    let claims = jwt_verify_with_leeway(
        &token,
        project_id,
        &active_keys,
        now_unix,
        jwt_iat_leeway_seconds,
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

/// Phase A A1 + A5 erasure/portability subtree — runs on
/// [`MeFeedbackDataState`] (see its doc for why it is not `AppState`). Merged
/// by `main.rs` alongside [`me_feedback_router`], WITHOUT a CORS layer (same
/// posture as the read subtree: consumed by the customer's own client, not a
/// browser embed). The static `/export` segment takes priority over the
/// `:feedback_id` param at the same position (matchit static-over-dynamic).
pub fn me_feedback_data_router(state: MeFeedbackDataState) -> Router {
    Router::new()
        .route(
            "/api/v1/projects/:project_id/me/feedback/export",
            get(export_my_feedback),
        )
        .route(
            "/api/v1/projects/:project_id/me/feedback/:feedback_id",
            delete(delete_my_feedback),
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
