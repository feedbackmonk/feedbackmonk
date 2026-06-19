#![allow(clippy::doc_markdown)] // module-doc references HTTP paths / crate paths / column names verbatim

//! Admin moderation handlers — Contract C28 + the board-settings toggle.
//!
//! Three surfaces, all behind the `AdminSession` extractor, all merged WITHOUT
//! `.layer(cors)` (admin-only, never a browser embed):
//!
//! ```text
//! moderation_router(state):
//!   POST  /api/v1/admin/feedback/{feedback_id}/moderate        (C28)
//!   GET   /api/v1/admin/feedback/moderation-queue              (C28)
//!   GET   /api/v1/admin/projects/{project_id}/board-settings   (C28-adjacent)
//!   PATCH /api/v1/admin/projects/{project_id}/board-settings   (C28-adjacent)
//! ```
//!
//! The `moderate` handler honours C28 Hard Invariant #1: the
//! `feedback.moderation_status` UPDATE and the `feedback_moderation_events`
//! ledger append land in ONE DB transaction (mirrors `perform_transition` for
//! the triage axis). Illegal transitions are rejected pre-DB-check (inv. 2);
//! a TOCTOU racer is caught by re-validating the lock-read `from_status`.
//!
//! The board-settings endpoints are contract-adjacent (the per-project toggle
//! GATE 1 requires; flagged to LEAD as not-in-C28/C29). Scoped per-project via
//! `ProjectRepo::open` (admin session → tenant scope → project scope).
//!
//! Lineage:
//!   Contract C28 (moderation state machine + admin queue)
//!   feedbackmonk-core::moderation (FROZEN state machine — Stage 0)
//!   migration 00016 (moderation_status + ledger + per-project board flags)

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use feedbackmonk_core::{
    legal_moderation_transitions_from, FeedbackId, FeedbackKind, ModerationStatus,
};

use crate::auth::AdminSession;
use crate::error::ApiError;
use crate::handlers::admin_feedback::{format_submitter_label, sole_project_scope};
use crate::state::AppState;

const DEFAULT_QUEUE_LIMIT: u32 = 20;
const MAX_QUEUE_LIMIT: u32 = 100;

/// Register the admin moderation + board-settings routes.
pub fn routes(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/v1/admin/feedback/moderation-queue",
            get(moderation_queue),
        )
        .route(
            "/api/v1/admin/feedback/:feedback_id/moderate",
            post(moderate),
        )
        .route(
            "/api/v1/admin/projects/:project_id/board-settings",
            get(get_board_settings).patch(patch_board_settings),
        )
        .with_state(state)
}

// ---------- Contract C28: moderate ----------

#[derive(Debug, Clone, Deserialize)]
pub struct ModerateRequest {
    pub to_status: ModerationStatus,
    pub reason_note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModerateResponse {
    pub feedback_id: String,
    pub from_status: ModerationStatus,
    pub to_status: ModerationStatus,
    pub moderated_at: DateTime<Utc>,
    pub audit_id: Uuid,
}

#[derive(Debug, Clone, Serialize)]
struct ModerationErrorBody {
    error: &'static str,
    from_status: ModerationStatus,
    to_status: ModerationStatus,
}

fn illegal_transition(from: ModerationStatus, to: ModerationStatus) -> ApiError {
    // C28: mirror C7's structured 409 transition-error body.
    let body = serde_json::to_string(&ModerationErrorBody {
        error: "IllegalModerationTransition",
        from_status: from,
        to_status: to,
    })
    .unwrap_or_else(|_| "IllegalModerationTransition".to_string());
    ApiError::Conflict(body)
}

pub async fn moderate(
    State(state): State<AppState>,
    session: AdminSession,
    Path(feedback_id): Path<String>,
    Json(req): Json<ModerateRequest>,
) -> Result<Json<ModerateResponse>, ApiError> {
    let project_scope = sole_project_scope(&state, &session.scope).await?;
    let fb_id = FeedbackId::from(feedback_id);

    // Pre-DB legality check (C28 inv. 2). The current status read is outside
    // the txn; the executor re-reads it under a row lock and we re-validate
    // there so a concurrent racer still rolls back.
    let from_status = state
        .feedback
        .get_moderation_status(&project_scope, &fb_id)
        .await?;
    if req.to_status == from_status
        || !legal_moderation_transitions_from(from_status).contains(&req.to_status)
    {
        return Err(illegal_transition(from_status, req.to_status));
    }

    // Same-transaction status flip + ledger append (C28 inv. 1).
    let mut tx = state.pool.begin().await?;
    let (actual_from, audit_id) = state
        .feedback
        .moderate_in_executor(
            &project_scope,
            &mut tx,
            &fb_id,
            req.to_status,
            req.reason_note.as_deref(),
            session.scope.tenant_id(),
        )
        .await?;

    // Re-validate against the lock-read status (TOCTOU): if a concurrent
    // transition moved the row between our outer read and the FOR UPDATE read,
    // roll back rather than commit an illegal step + its ledger row.
    if actual_from == req.to_status
        || !legal_moderation_transitions_from(actual_from).contains(&req.to_status)
    {
        tx.rollback().await.ok();
        return Err(illegal_transition(actual_from, req.to_status));
    }
    tx.commit().await?;

    tracing::info!(
        target: "admin",
        feedback_id = %fb_id,
        from_status = %actual_from.as_db_str(),
        to_status = %req.to_status.as_db_str(),
        "feedback moderation transition committed"
    );

    Ok(Json(ModerateResponse {
        feedback_id: fb_id.as_str().to_string(),
        from_status: actual_from,
        to_status: req.to_status,
        moderated_at: Utc::now(),
        audit_id,
    }))
}

// ---------- Contract C28: moderation queue ----------

#[derive(Debug, Clone, Deserialize)]
pub struct QueueParams {
    /// Defaults to `pending` (the queue). Accepts `approved`/`rejected` for review.
    pub status: Option<ModerationStatus>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct QueueResponse {
    pub items: Vec<QueueItemWire>,
    pub total: u32,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct QueueItemWire {
    pub feedback_id: String,
    pub kind: FeedbackKind,
    pub moderation_status: ModerationStatus,
    pub body_excerpt: String,
    pub submitted_at: DateTime<Utc>,
    pub submitter_label: String,
}

pub async fn moderation_queue(
    State(state): State<AppState>,
    session: AdminSession,
    Query(params): Query<QueueParams>,
) -> Result<Json<QueueResponse>, ApiError> {
    let project_scope = sole_project_scope(&state, &session.scope).await?;
    let status = params.status.unwrap_or_default(); // ModerationStatus::Pending
    let limit = params.limit.unwrap_or(DEFAULT_QUEUE_LIMIT).min(MAX_QUEUE_LIMIT);
    let offset = params.offset.unwrap_or(0);

    let (items, total) = state
        .feedback
        .list_pending_for_admin(&project_scope, status, limit, offset)
        .await?;

    let wire_items = items
        .into_iter()
        .map(|it| QueueItemWire {
            feedback_id: it.feedback_id.as_str().to_string(),
            kind: it.kind,
            moderation_status: it.moderation_status,
            body_excerpt: it.body_excerpt,
            submitter_label: format_submitter_label(it.submitter_email.as_deref(), it.is_anonymous),
            submitted_at: it.submitted_at,
        })
        .collect();

    Ok(Json(QueueResponse {
        items: wire_items,
        total,
        limit,
        offset,
    }))
}

// ---------- Board settings (contract-adjacent: per-project toggle) ----------

#[derive(Debug, Clone, Serialize)]
pub struct BoardSettingsWire {
    pub public_board_enabled: bool,
    pub board_requires_moderation: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BoardSettingsPatch {
    #[serde(default)]
    pub public_board_enabled: Option<bool>,
    #[serde(default)]
    pub board_requires_moderation: Option<bool>,
}

pub async fn get_board_settings(
    State(state): State<AppState>,
    session: AdminSession,
    Path(project_id): Path<Uuid>,
) -> Result<Json<BoardSettingsWire>, ApiError> {
    let scope = state.projects.open(&session.scope, project_id).await?;
    let s = state.projects.get_board_settings(&scope).await?;
    Ok(Json(BoardSettingsWire {
        public_board_enabled: s.public_board_enabled,
        board_requires_moderation: s.board_requires_moderation,
    }))
}

pub async fn patch_board_settings(
    State(state): State<AppState>,
    session: AdminSession,
    Path(project_id): Path<Uuid>,
    Json(req): Json<BoardSettingsPatch>,
) -> Result<Json<BoardSettingsWire>, ApiError> {
    let scope = state.projects.open(&session.scope, project_id).await?;
    let s = state
        .projects
        .set_board_settings(&scope, req.public_board_enabled, req.board_requires_moderation)
        .await?;
    Ok(Json(BoardSettingsWire {
        public_board_enabled: s.public_board_enabled,
        board_requires_moderation: s.board_requires_moderation,
    }))
}
