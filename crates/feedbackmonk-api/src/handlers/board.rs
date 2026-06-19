#![allow(clippy::unused_async)] // axum handlers must be async even when the body has no .await branch
#![allow(clippy::doc_markdown)] // module-doc references HTTP paths / column names verbatim

//! Public feedback board HTTP handlers — Contract C29.
//!
//! Unauthenticated, CORS-exposed endpoints (mirror `roadmap_router`):
//!
//! ```text
//! board_router(state):
//!   GET    /api/v1/projects/{project_id}/board?limit=&offset=
//!   GET    /api/v1/projects/{project_id}/board/items/{short_code}
//!   POST   /api/v1/projects/{project_id}/board/items/{short_code}/vote
//!   DELETE /api/v1/projects/{project_id}/board/items/{short_code}/vote
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
//! Voting (Contract C30, PF-BOARD-VOTING-01) is LIVE: the POST/DELETE vote
//! routes wire end-user voting on approved board items, and `vote_count` is the
//! real aggregate over `feedback_board_votes` (D1, direct SQL count — not a
//! cache). The vote handlers run the moderation gate (D2): a vote/retract on a
//! non-approved or board-disabled item 404s identically to the read path, so the
//! endpoint never leaks the existence of hidden feedback. Voter resolution
//! reuses the shared `voting_common` chokepoint (the same anon/JWT primitive the
//! roadmap voting handlers use — migration 00007/00018 invariant #2).
//!
//! Lineage:
//!   Contract C29 (public board read + privacy shape) + C30 (board voting)
//!   FR-FBR-12 sibling (public roadmap) / DEC-FBR-02 + Q24 (public-surface privacy)
//!   migration 00016 (moderation_status + per-project board flags) + 00018 (votes)
//!   PF-BOARD-VOTING-01 / DEC-FBR-IMPL-21

use std::net::SocketAddr;
use std::time::Duration;

use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::header::SET_COOKIE;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use feedbackmonk_core::{FeedbackId, FeedbackKind, FeedbackStatus, RoadmapVoterMode};
use feedbackmonk_repository::{RetractOutcome, DEFAULT_RETRACTION_WINDOW};

use crate::error::ApiError;
use crate::handlers::voting_common::{
    already_voted_response, jwt_error_response, rate_limited_response, resolve_voter,
    resolve_voter_no_rate_limit, retraction_window_expired_response, vote_not_found_response,
    VoteResolveError,
};
use crate::state::AppState;

const DEFAULT_BOARD_LIMIT: u32 = 20;
const MAX_BOARD_LIMIT: u32 = 100;

/// Board-vote retraction window — mirrors `roadmap.rs::RETRACTION_WINDOW`
/// (`DEFAULT_RETRACTION_WINDOW`, 60s default; flex 30..=120s per self-mediation).
pub const RETRACTION_WINDOW: Duration = DEFAULT_RETRACTION_WINDOW;

pub fn board_router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/projects/:project_id/board", get(public_board_list))
        .route(
            "/api/v1/projects/:project_id/board/items/:short_code",
            get(public_board_item),
        )
        // Board voting (Contract C30, PF-BOARD-VOTING-01). CORS-exposed via the
        // `.layer(cors)` board_router merge in build_app — same public widget
        // posture as the reads + submit. Each handler runs the moderation gate
        // (D2) before any write.
        .route(
            "/api/v1/projects/:project_id/board/items/:short_code/vote",
            post(public_board_vote_cast).delete(public_board_vote_retract),
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

/// Board vote-cast response (Contract C30). Mirrors `roadmap.rs::VoteCastResponse`
/// but keyed on `short_code` (the board item identifier) instead of `item_slug`.
#[derive(Debug, Clone, Serialize)]
pub struct VoteCastResponse {
    pub short_code: String,
    pub voter_mode: RoadmapVoterMode,
    pub cast_at: DateTime<Utc>,
}

/// Board vote-retract response (Contract C30).
#[derive(Debug, Clone, Serialize)]
pub struct VoteRetractResponse {
    pub short_code: String,
    pub retracted_at: DateTime<Utc>,
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

/// `POST .../board/items/{short_code}/vote` — cast a board vote (Contract C30).
///
/// Moderation gate (D2): resolves the target through an approved-only SQL filter
/// (`resolve_approved_board_feedback_id`) BEFORE any write, so a
/// pending/rejected/board-disabled item 404s identically to the read path — the
/// vote endpoint never confirms the existence of hidden feedback. Voter
/// resolution reuses the shared chokepoint (`voting_common::resolve_voter`).
async fn public_board_vote_cast(
    State(state): State<AppState>,
    Path((project_id, short_code)): Path<(Uuid, String)>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let scope = state.projects.open_for_submission(project_id).await?;
    ensure_board_enabled(&state, &scope).await?;

    // D2 gate — approved-only resolution before the write. NotFound → 404.
    let internal_id = state
        .feedback
        .resolve_approved_board_feedback_id(&scope, &FeedbackId::from(short_code.clone()))
        .await?;

    let (voter_id, voter_mode, set_cookie_header) =
        match resolve_voter(&state, &scope, project_id, &headers, addr).await {
            Ok(r) => r,
            Err(VoteResolveError::JwtError(e)) => return Ok(jwt_error_response(&e)),
            Err(VoteResolveError::RateLimited(retry)) => return Ok(rate_limited_response(retry)),
            Err(VoteResolveError::Api(api)) => return Err(api),
        };

    let vote = match state
        .board_votes
        .cast(&scope, internal_id, &voter_id, voter_mode)
        .await
    {
        Ok(v) => v,
        Err(feedbackmonk_repository::RepoError::Conflict) => return Ok(already_voted_response()),
        Err(e) => return Err(e.into()),
    };

    let resp = Json(VoteCastResponse {
        short_code,
        voter_mode: vote.voter_mode,
        cast_at: vote.cast_at,
    });
    let mut response = (StatusCode::OK, resp).into_response();
    if let Some(cookie) = set_cookie_header {
        response.headers_mut().insert(SET_COOKIE, cookie);
    }
    Ok(response)
}

/// `DELETE .../board/items/{short_code}/vote` — retract a board vote within the
/// retraction window (Contract C30). Same D2 moderation gate as cast. Retract
/// does NOT touch the anon rate limit (it's not a new submission), so it resolves
/// the voter via `resolve_voter_no_rate_limit`.
async fn public_board_vote_retract(
    State(state): State<AppState>,
    Path((project_id, short_code)): Path<(Uuid, String)>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let scope = state.projects.open_for_submission(project_id).await?;
    ensure_board_enabled(&state, &scope).await?;

    // D2 gate — approved-only resolution before the write. NotFound → 404.
    let internal_id = state
        .feedback
        .resolve_approved_board_feedback_id(&scope, &FeedbackId::from(short_code.clone()))
        .await?;

    let (voter_id, _voter_mode, _set_cookie) =
        match resolve_voter_no_rate_limit(&state, &scope, project_id, &headers, addr).await {
            Ok(r) => r,
            Err(VoteResolveError::JwtError(e)) => return Ok(jwt_error_response(&e)),
            Err(VoteResolveError::RateLimited(_)) => unreachable!("no_rate_limit variant"),
            Err(VoteResolveError::Api(api)) => return Err(api),
        };

    match state
        .board_votes
        .retract(&scope, internal_id, &voter_id, RETRACTION_WINDOW)
        .await?
    {
        RetractOutcome::Removed { retracted_at } => {
            let resp = Json(VoteRetractResponse {
                short_code,
                retracted_at,
            });
            Ok((StatusCode::OK, resp).into_response())
        }
        RetractOutcome::NotFound => Ok(vote_not_found_response()),
        RetractOutcome::WindowExpired { .. } => Ok(retraction_window_expired_response()),
    }
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
        // Real aggregate over `feedback_board_votes` (D1, PF-BOARD-VOTING-01) —
        // populated by the board read SQL. No longer a hard `0` placeholder.
        vote_count: item.vote_count,
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
