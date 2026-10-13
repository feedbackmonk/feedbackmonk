#![allow(clippy::unused_async)] // axum handlers must be async even when the body has no .await branch
#![allow(clippy::doc_markdown)] // module-doc references HTTP paths / column names verbatim

//! Owner surfaces for end-user prioritization — Contract C44 (FR-FBR-43/49/55/
//! 56/58). Wire contract: `docs/planning/handoffs/prioritization-contracts.md`.
//!
//! **Contract stub.** The `contracts` piece froze the wire shapes and handler
//! names below; the `admin-api` piece writes the bodies and registers the
//! routes from `moderation::routes` (already classified `admin` in
//! `build_app`, so no new router and no oracle table change):
//!
//! ```text
//!   GET    /api/v1/admin/projects/{project_id}/priority        get_priority_view
//!   GET    /api/v1/admin/feedback/{feedback_id}/board-note     get_board_note
//!   PUT    /api/v1/admin/feedback/{feedback_id}/board-note     put_board_note
//!   DELETE /api/v1/admin/feedback/{feedback_id}/board-note     delete_board_note
//! ```
//!
//! plus `board_access` on the existing board-settings GET/PATCH (moderation.rs,
//! `ProjectRepo::set_board_access`). All behind `AdminSession`, no CORS.
//!
//! Privacy (FR-FBR-56): no response here names a user, a user's list, or who
//! marked an item not needed — only counts and the fitted order. The priority
//! view never merges votes, not-needed and priority into one score (FR-FBR-55).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use feedbackmonk_core::FeedbackStatus;

use crate::auth::AdminSession;
use crate::state::AppState;

/// One fitted item in the owner's priority view (FR-FBR-55). `rank` is the
/// 1-based position in the fitted order; `uncertainty` is the fit's standard
/// error on the item's strength (the UI bands it). Votes and not-needed sit
/// beside the rank, never combined with it.
#[derive(Debug, Clone, Serialize)]
pub struct PriorityRankedWire {
    pub short_code: String,
    pub body_excerpt: String,
    pub status: FeedbackStatus,
    pub rank: u32,
    pub rankers: u32,
    pub uncertainty: f64,
    pub vote_count: i64,
    pub not_needed_count: i64,
}

/// A pool item no stated preference touches (FR-FBR-52/54): no rank, no
/// strength.
#[derive(Debug, Clone, Serialize)]
pub struct PriorityUnrankedWire {
    pub short_code: String,
    pub body_excerpt: String,
    pub status: FeedbackStatus,
    pub rankers: u32,
    pub vote_count: i64,
    pub not_needed_count: i64,
}

/// `GET /api/v1/admin/projects/{project_id}/priority` (FR-FBR-55).
#[derive(Debug, Clone, Serialize)]
pub struct PriorityViewResponse {
    pub computed_at: DateTime<Utc>,
    /// Users whose list contributed at least one preference.
    pub contributing_rankers: u32,
    pub ranked: Vec<PriorityRankedWire>,
    pub unranked: Vec<PriorityUnrankedWire>,
}

/// `PUT …/board-note` body (FR-FBR-58). Plain text, 1..=4000 chars trimmed.
#[derive(Debug, Clone, Deserialize)]
pub struct BoardNoteRequest {
    pub body: String,
}

/// The owner's view of an item's board note. `note: null` when none.
#[derive(Debug, Clone, Serialize)]
pub struct BoardNoteResponse {
    pub short_code: String,
    pub note: Option<BoardNoteWire>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BoardNoteWire {
    pub body: String,
    pub updated_at: DateTime<Utc>,
}

/// Contract stub: 501 until `admin-api` implements it.
pub async fn get_priority_view(
    State(state): State<AppState>,
    session: AdminSession,
    Path(project_id): Path<Uuid>,
) -> Response {
    let _ = (state, session, project_id);
    StatusCode::NOT_IMPLEMENTED.into_response()
}

/// Contract stub: 501 until `admin-api` implements it.
pub async fn get_board_note(
    State(state): State<AppState>,
    session: AdminSession,
    Path(feedback_id): Path<String>,
) -> Response {
    let _ = (state, session, feedback_id);
    StatusCode::NOT_IMPLEMENTED.into_response()
}

/// Contract stub: 501 until `admin-api` implements it.
pub async fn put_board_note(
    State(state): State<AppState>,
    session: AdminSession,
    Path(feedback_id): Path<String>,
    Json(req): Json<BoardNoteRequest>,
) -> Response {
    let _ = (state, session, feedback_id, req);
    StatusCode::NOT_IMPLEMENTED.into_response()
}

/// Contract stub: 501 until `admin-api` implements it.
pub async fn delete_board_note(
    State(state): State<AppState>,
    session: AdminSession,
    Path(feedback_id): Path<String>,
) -> Response {
    let _ = (state, session, feedback_id);
    StatusCode::NOT_IMPLEMENTED.into_response()
}
