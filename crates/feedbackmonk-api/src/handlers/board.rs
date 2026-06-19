#![allow(clippy::unused_async)] // axum handlers must be async even when the body has no .await branch
#![allow(clippy::doc_markdown)] // module-doc references HTTP paths / column names verbatim

//! Public feedback board HTTP handlers — Contract C29.
//!
//! Two unauthenticated, CORS-exposed endpoints (mirror `roadmap_router`):
//!
//! ```text
//! board_router(state):
//!   GET /api/v1/projects/{project_id}/board?limit=&offset=
//!   GET /api/v1/projects/{project_id}/board/items/{short_code}
//! ```
//!
//! Public-scope minting via `ProjectRepo::open_for_submission` — the
//! allowlisted pre-auth boundary (DEC-PODS-001) the submission + roadmap
//! endpoints already use. Merged into `build_app` WITH `.layer(cors)` (public
//! widget surface, matches submit/attachments).
//!
//! Three load-bearing C29 invariants enforced here + in the repo layer:
//!   1. **Approved-only** — the repo board read (`list_public_board` /
//!      `get_public_board_item`) hard-filters approved rows in SQL. A
//!      non-approved row is structurally unreachable through these handlers.
//!   2. **Board-disabled → 404** — when the project has not opted into the
//!      public board, every board endpoint returns 404 (no approved row leaks
//!      from a project that hasn't enabled the board).
//!   3. **No submitter identity** — the wire shape carries ONLY public-facing
//!      fields (privacy invariant, sibling to Q24). This module names no
//!      private columns by construction.
//!
//! Voting is DEFERRED (Worker A Task Zero): `vote_count` ships as a hard `0`
//! placeholder this stage (as `reply_count` did in C8 Stage 1).
//!
//! Lineage:
//!   Contract C29 (public board read + privacy shape)
//!   FR-FBR-12 sibling (public roadmap) / DEC-FBR-02 + Q24 (public-surface privacy)
//!   migration 00016 (moderation_status + per-project board flags)

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use feedbackmonk_core::{FeedbackId, FeedbackKind, FeedbackStatus};

use crate::error::ApiError;
use crate::state::AppState;

const DEFAULT_BOARD_LIMIT: u32 = 20;
const MAX_BOARD_LIMIT: u32 = 100;

/// `vote_count` placeholder while board voting is deferred (Task Zero).
const VOTE_COUNT_PLACEHOLDER: i64 = 0;

pub fn board_router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/projects/:project_id/board", get(public_board_list))
        .route(
            "/api/v1/projects/:project_id/board/items/:short_code",
            get(public_board_item),
        )
        .with_state(state)
}

// ===========================================================================
// Response shapes (Contract C29)
// ===========================================================================

#[derive(Debug, Clone, Serialize)]
pub struct BoardItemResponse {
    pub short_code: String,
    pub body: String,
    pub kind: FeedbackKind,
    pub status: FeedbackStatus,
    pub vote_count: i64,
    pub accepted_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BoardListResponse {
    pub items: Vec<BoardItemResponse>,
    pub total: u32,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BoardListQuery {
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub offset: Option<u32>,
}

// ===========================================================================
// Handlers
// ===========================================================================

async fn public_board_list(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Query(q): Query<BoardListQuery>,
) -> Result<Json<BoardListResponse>, ApiError> {
    let scope = state.projects.open_for_submission(project_id).await?;
    ensure_board_enabled(&state, &scope).await?;

    let limit = clamp_board_limit(q.limit);
    let offset = q.offset.unwrap_or(0);

    let (items, total) = state.feedback.list_public_board(&scope, limit, offset).await?;
    let wire = items.into_iter().map(item_response).collect();

    Ok(Json(BoardListResponse {
        items: wire,
        total,
        limit,
        offset,
    }))
}

async fn public_board_item(
    State(state): State<AppState>,
    Path((project_id, short_code)): Path<(Uuid, String)>,
) -> Result<Json<BoardItemResponse>, ApiError> {
    let scope = state.projects.open_for_submission(project_id).await?;
    ensure_board_enabled(&state, &scope).await?;

    let item = state
        .feedback
        .get_public_board_item(&scope, &FeedbackId::from(short_code))
        .await?;
    Ok(Json(item_response(item)))
}

// ===========================================================================
// Helpers
// ===========================================================================

/// C29 inv. 2: a project that has not opted into the public board exposes
/// nothing — every board endpoint 404s. Checked BEFORE any row read so an
/// approved row never leaks from a board-disabled project.
async fn ensure_board_enabled(state: &AppState, scope: &feedbackmonk_repository::ProjectScope) -> Result<(), ApiError> {
    let settings = state.projects.get_board_settings(scope).await?;
    if settings.public_board_enabled {
        Ok(())
    } else {
        Err(ApiError::NotFound)
    }
}

fn clamp_board_limit(requested: Option<u32>) -> u32 {
    requested.map_or(DEFAULT_BOARD_LIMIT, |n| n.clamp(1, MAX_BOARD_LIMIT))
}

fn item_response(item: feedbackmonk_repository::BoardItem) -> BoardItemResponse {
    BoardItemResponse {
        short_code: item.feedback_id.as_str().to_string(),
        body: item.body,
        kind: item.kind,
        status: item.status,
        vote_count: VOTE_COUNT_PLACEHOLDER,
        accepted_at: item.accepted_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_board_limit_defaults_and_caps() {
        assert_eq!(clamp_board_limit(None), DEFAULT_BOARD_LIMIT);
        assert_eq!(clamp_board_limit(Some(0)), 1, "zero clamps up to 1");
        assert_eq!(clamp_board_limit(Some(10_000)), MAX_BOARD_LIMIT);
        assert_eq!(clamp_board_limit(Some(25)), 25);
    }
}
