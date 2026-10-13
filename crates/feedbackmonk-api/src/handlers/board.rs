#![allow(clippy::unused_async)] // axum handlers must be async even when the body has no .await branch
#![allow(clippy::doc_markdown)] // module-doc references HTTP paths / column names verbatim

//! Public feedback board HTTP handlers — Contracts C29, C30 and C43.
//!
//! Public, CORS-exposed endpoints (mirror `roadmap_router`):
//!
//! ```text
//! board_router(state):
//!   GET    /api/v1/projects/{project_id}/board?kind=&limit=&offset=
//!   GET    /api/v1/projects/{project_id}/board/items/{short_code}
//!   POST   /api/v1/projects/{project_id}/board/items/{short_code}/vote
//!   DELETE /api/v1/projects/{project_id}/board/items/{short_code}/vote
//!   PUT    /api/v1/projects/{project_id}/board/items/{short_code}/not-needed
//!   DELETE /api/v1/projects/{project_id}/board/items/{short_code}/not-needed
//!   GET    /api/v1/projects/{project_id}/board/priorities
//!   PUT    /api/v1/projects/{project_id}/board/priorities
//!   POST   /api/v1/projects/{project_id}/board/session     (board_session.rs)
//!   GET    /api/v1/projects/{project_id}/board/session     (board_session.rs)
//!   DELETE /api/v1/projects/{project_id}/board/session     (board_session.rs)
//! ```
//!
//! Public-scope minting via `ProjectRepo::open_for_submission` — the
//! allowlisted pre-auth boundary (DEC-PODS-001) the submission + roadmap
//! endpoints already use. Merged into `build_app` WITH `.layer(cors)` (public
//! widget surface, matches submit/attachments).
//!
//! Every route runs, in this order (Contract C43, "The rules every handler
//! enforces"): scope → [`ensure_board_enabled`] (404 when disabled) →
//! `resolve_board_viewer` (bearer JWT, else board-session cookie, else
//! anonymous; a bad bearer token is the JWT error) → on a `signed_in` board an
//! anonymous viewer gets `401 SignInRequired` (FR-FBR-43) → writes that need a
//! person (not-needed, priority list) refuse an anonymous viewer on any board
//! (DEC-FBR-20) → the moderation gate before every write (FR-FBR-61). The three
//! session routes skip the sign-in check: they are how a viewer signs in.
//!
//! Load-bearing C29 invariants, enforced here + in the repository layer:
//!   1. **Approved-only** — every board read and write resolves through an
//!      approved-only SQL filter. A non-approved row is structurally
//!      unreachable through these handlers.
//!   2. **Board-disabled → 404** — when the project has not opted into the
//!      public board, every board endpoint returns 404.
//!   3. **No submitter identity** — the wire shapes carry ONLY public-facing
//!      fields (privacy invariant, sibling to Q24; FR-FBR-59 for the detail).
//!
//! And the C43 privacy rules: per-viewer state is the caller's own and is
//! absent for an anonymous viewer (FR-FBR-46); no response carries a
//! not-needed count or any priority aggregate (FR-FBR-49/56), and the board
//! order (newest first) does not depend on them.
//!
//! Voting (Contract C30): `vote_count` is the real aggregate over
//! `feedback_board_votes`. A signed-in voter (bearer or session) votes as
//! their `sub` in `jwt` mode and may retract at any time (FR-FBR-47); an
//! anonymous voter resolves through the shared `voting_common` chokepoint and
//! keeps the retraction window.
//!
//! Lineage:
//!   Contract C29 (public board read + privacy shape) + C30 (board voting) +
//!   C43 (end-user prioritization, FR-FBR-42..61)
//!   FR-FBR-12 sibling (public roadmap) / DEC-FBR-02 + Q24 (public-surface privacy)
//!   migrations 00016, 00018, 00033..00036

use std::net::SocketAddr;
use std::time::Duration;

use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::header::SET_COOKIE;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use feedbackmonk_core::{
    in_ranking_pool, BoardAccess, FeedbackId, FeedbackKind, FeedbackStatus, ModerationStatus,
    RoadmapVoterMode, ViewerItemState,
};
use feedbackmonk_repository::{
    BoardFilter, BoardSettings, PriorityListView, ProjectScope, RepoError, RetractOutcome,
    DEFAULT_RETRACTION_WINDOW,
};

use crate::error::ApiError;
use crate::handlers::board_session::{
    end_board_session, exchange_board_session, get_board_session, resolve_board_viewer,
    sign_in_required_response, BoardViewer,
};
use crate::handlers::voting_common::{
    already_voted_response, jwt_error_response, rate_limited_response, resolve_voter,
    resolve_voter_no_rate_limit, retraction_window_expired_response, vote_not_found_response,
    VoteResolveError,
};
use crate::state::AppState;

const DEFAULT_BOARD_LIMIT: u32 = 20;
const MAX_BOARD_LIMIT: u32 = 100;

/// Anonymous board-vote retraction window — mirrors `roadmap.rs::RETRACTION_WINDOW`
/// (`DEFAULT_RETRACTION_WINDOW`, 60s default; flex 30..=120s per self-mediation).
/// A signed-in voter's retract has no window (FR-FBR-47).
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
        // C43: a signed-in viewer's private signals (FR-FBR-48/50).
        .route(
            "/api/v1/projects/:project_id/board/items/:short_code/not-needed",
            put(public_board_not_needed_set).delete(public_board_not_needed_clear),
        )
        .route(
            "/api/v1/projects/:project_id/board/priorities",
            get(public_board_priorities_get).put(public_board_priorities_put),
        )
        // C43: the hosted board's sign-in handoff (FR-FBR-44). Handlers live in
        // `board_session.rs`; registered here so the router set is unchanged.
        .route(
            "/api/v1/projects/:project_id/board/session",
            post(exchange_board_session)
                .get(get_board_session)
                .delete(end_board_session),
        )
        .with_state(state)
}

// ===========================================================================
// Response shapes (Contracts C29 / C43)
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

/// A list item: the C29 item plus the CALLER's own state, present only for a
/// signed-in viewer (FR-FBR-46). Absent, not null, for an anonymous viewer.
#[derive(Debug, Clone, Serialize)]
pub struct BoardListItemResponse {
    #[serde(flatten)]
    pub item: BoardItemResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub viewer: Option<ViewerItemState>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BoardListResponse {
    pub items: Vec<BoardListItemResponse>,
    /// Counts only the filtered kind (FR-FBR-42).
    pub total: u32,
    pub limit: u32,
    pub offset: u32,
    pub access: BoardAccess,
    pub signed_in: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BoardListQuery {
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub offset: Option<u32>,
}

/// The owner's board note as a board viewer sees it (FR-FBR-58).
#[derive(Debug, Clone, Serialize)]
pub struct BoardNoteResponse {
    pub body: String,
    pub updated_at: DateTime<Utc>,
}

/// One submitter-visible status transition: status and date ONLY (FR-FBR-57).
#[derive(Debug, Clone, Serialize)]
pub struct BoardTimelineEntryResponse {
    pub status: FeedbackStatus,
    pub at: DateTime<Utc>,
}

/// `GET …/board/items/{short_code}` (FR-FBR-57/58/59). Carries no submitter
/// identity, reply, internal or status-change note, attachment, translated
/// body or locale.
#[derive(Debug, Clone, Serialize)]
pub struct BoardItemDetailResponse {
    #[serde(flatten)]
    pub item: BoardItemResponse,
    pub board_note: Option<BoardNoteResponse>,
    /// Oldest first.
    pub timeline: Vec<BoardTimelineEntryResponse>,
    pub in_ranking_pool: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub viewer: Option<ViewerItemState>,
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

/// `PUT|DELETE …/not-needed` response (FR-FBR-48). `upvote_cleared` only on PUT.
#[derive(Debug, Clone, Serialize)]
pub struct NotNeededResponse {
    pub short_code: String,
    pub not_needed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upvote_cleared: Option<bool>,
}

/// `PUT …/priorities` body: the whole order, top first; `[]` clears.
#[derive(Debug, Clone, Deserialize)]
pub struct PriorityListRequest {
    pub items: Vec<String>,
}

/// `GET|PUT …/priorities` response (FR-FBR-50/51). The viewer's OWN list only;
/// no aggregate, no other user's order (FR-FBR-56).
#[derive(Debug, Clone, Serialize)]
pub struct PriorityListResponse {
    pub entries: Vec<BoardItemResponse>,
    pub unranked: Vec<BoardItemResponse>,
    pub removed: Vec<BoardItemResponse>,
    pub removed_hidden: u32,
    /// `entries.len() >= 2` — a shorter list is kept but moves nothing.
    pub influences_priority: bool,
}

// ===========================================================================
// Handlers
// ===========================================================================

/// The viewer, after the board-enabled check (Contract C43 rules 3–4): a
/// bearer JWT, else a live board session, else anonymous; and on a
/// `signed_in` board an anonymous viewer is refused with the sign-in response
/// (FR-FBR-43). `Err` is the finished response (sign-in or JWT error).
#[allow(clippy::result_large_err)] // the error is a finished refusal response
async fn board_viewer(
    state: &AppState,
    scope: &ProjectScope,
    settings: &BoardSettings,
    headers: &HeaderMap,
) -> Result<BoardViewer, Response> {
    let viewer = resolve_board_viewer(state, scope, headers)
        .await
        .map_err(IntoResponse::into_response)?;
    if settings.board_access == BoardAccess::SignedIn && viewer == BoardViewer::Anonymous {
        return Err(sign_in_required_response());
    }
    Ok(viewer)
}

/// The signed-in `sub`, or the sign-in refusal for a write that needs a person
/// (DEC-FBR-20) — on a `public` board too.
#[allow(clippy::result_large_err)] // the error is a finished refusal response
fn require_person(viewer: &BoardViewer) -> Result<&str, Response> {
    viewer.sub().ok_or_else(sign_in_required_response)
}

async fn public_board_list(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Query(q): Query<BoardListQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let scope = state.projects.open_for_submission(project_id).await?;
    let settings = ensure_board_enabled(&state, &scope).await?;
    let viewer = match board_viewer(&state, &scope, &settings, &headers).await {
        Ok(v) => v,
        Err(resp) => return Ok(resp),
    };
    let kind = parse_kind_filter(q.kind.as_deref())?;
    let limit = clamp_board_limit(q.limit);
    let offset = q.offset.unwrap_or(0);

    let (items, total) = state
        .board_votes
        .signals()
        .list_board_for_viewer(&scope, BoardFilter { kind }, viewer.sub(), limit, offset)
        .await?;
    let items = items
        .into_iter()
        .map(|v| BoardListItemResponse {
            item: item_response(v.item),
            viewer: v.viewer,
        })
        .collect();

    Ok(Json(BoardListResponse {
        items,
        total,
        limit,
        offset,
        access: settings.board_access,
        signed_in: viewer.sub().is_some(),
    })
    .into_response())
}

async fn public_board_item(
    State(state): State<AppState>,
    Path((project_id, short_code)): Path<(Uuid, String)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let scope = state.projects.open_for_submission(project_id).await?;
    let settings = ensure_board_enabled(&state, &scope).await?;
    let viewer = match board_viewer(&state, &scope, &settings, &headers).await {
        Ok(v) => v,
        Err(resp) => return Ok(resp),
    };
    let fb = FeedbackId::from(short_code);

    // Approved-only detail record (NotFound → 404 for a hidden item).
    let detail = state
        .board_votes
        .notes()
        .get_board_item_detail(&scope, &fb)
        .await?;
    let viewer = match viewer.sub() {
        Some(sub) => Some(
            state
                .board_votes
                .signals()
                .viewer_state_for_board_item(&scope, &fb, sub)
                .await?,
        ),
        None => None,
    };

    let pool = in_ranking_pool(ModerationStatus::Approved, detail.item.kind, detail.item.status);
    Ok(Json(BoardItemDetailResponse {
        item: item_response(detail.item),
        board_note: detail.board_note.map(|n| BoardNoteResponse {
            body: n.body,
            updated_at: n.updated_at,
        }),
        timeline: detail
            .timeline
            .into_iter()
            .map(|t| BoardTimelineEntryResponse {
                status: t.status,
                at: t.at,
            })
            .collect(),
        in_ranking_pool: pool,
        viewer,
    })
    .into_response())
}

/// `POST .../board/items/{short_code}/vote` — cast a board vote (Contract C30).
///
/// Board gate first (a `signed_in` board refuses an anonymous voter before
/// anything is resolved, so the refusal confirms nothing about the item), then
/// the moderation gate (D2): the target resolves through an approved-only SQL
/// filter (`resolve_approved_board_feedback_id`) BEFORE any write, so a
/// pending/rejected item 404s identically to the read path. A signed-in viewer
/// votes as their `sub` (`jwt` mode); an anonymous voter resolves through the
/// shared chokepoint (`voting_common::resolve_voter`, rate-limited).
async fn public_board_vote_cast(
    State(state): State<AppState>,
    Path((project_id, short_code)): Path<(Uuid, String)>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let scope = state.projects.open_for_submission(project_id).await?;
    let settings = ensure_board_enabled(&state, &scope).await?;
    let viewer = match board_viewer(&state, &scope, &settings, &headers).await {
        Ok(v) => v,
        Err(resp) => return Ok(resp),
    };

    // D2 gate — approved-only resolution before the write. NotFound → 404.
    let internal_id = state
        .feedback
        .resolve_approved_board_feedback_id(&scope, &FeedbackId::from(short_code.clone()))
        .await?;

    let (voter_id, voter_mode, set_cookie_header) = match viewer {
        BoardViewer::SignedIn { sub } => (sub, RoadmapVoterMode::Jwt, None),
        BoardViewer::Anonymous => {
            match resolve_voter(&state, &scope, project_id, &headers, addr).await {
                Ok(r) => r,
                Err(VoteResolveError::JwtError(e)) => return Ok(jwt_error_response(&e)),
                Err(VoteResolveError::RateLimited(retry)) => {
                    return Ok(rate_limited_response(retry))
                }
                Err(VoteResolveError::Api(api)) => return Err(api),
            }
        }
    };

    let vote = match state
        .board_votes
        .cast(&scope, internal_id, &voter_id, voter_mode)
        .await
    {
        Ok(v) => v,
        Err(RepoError::Conflict) => return Ok(already_voted_response()),
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

/// `DELETE .../board/items/{short_code}/vote` — retract a board vote (Contract
/// C30, FR-FBR-47). Same gates as cast. A signed-in voter retracts at any time;
/// an anonymous voter keeps [`RETRACTION_WINDOW`] and its 403. Retract does NOT
/// touch the anon rate limit (it's not a new submission).
async fn public_board_vote_retract(
    State(state): State<AppState>,
    Path((project_id, short_code)): Path<(Uuid, String)>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let scope = state.projects.open_for_submission(project_id).await?;
    let settings = ensure_board_enabled(&state, &scope).await?;
    let viewer = match board_viewer(&state, &scope, &settings, &headers).await {
        Ok(v) => v,
        Err(resp) => return Ok(resp),
    };

    // D2 gate — approved-only resolution before the write. NotFound → 404.
    let internal_id = state
        .feedback
        .resolve_approved_board_feedback_id(&scope, &FeedbackId::from(short_code.clone()))
        .await?;

    let (voter_id, window) = match viewer {
        BoardViewer::SignedIn { sub } => (sub, Duration::MAX),
        BoardViewer::Anonymous => {
            match resolve_voter_no_rate_limit(&state, &scope, project_id, &headers, addr).await {
                Ok((voter_id, _mode, _set_cookie)) => (voter_id, RETRACTION_WINDOW),
                Err(VoteResolveError::JwtError(e)) => return Ok(jwt_error_response(&e)),
                Err(VoteResolveError::RateLimited(_)) => unreachable!("no_rate_limit variant"),
                Err(VoteResolveError::Api(api)) => return Err(api),
            }
        }
    };

    match state
        .board_votes
        .retract(&scope, internal_id, &voter_id, window)
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

/// `PUT .../board/items/{short_code}/not-needed` (FR-FBR-48/61). Signed-in
/// only. The repository write resolves the item approved-only and in the
/// ranking pool before it writes (`NotFound` → 404), and clears the caller's
/// upvote in the same transaction.
async fn public_board_not_needed_set(
    State(state): State<AppState>,
    Path((project_id, short_code)): Path<(Uuid, String)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let scope = state.projects.open_for_submission(project_id).await?;
    let settings = ensure_board_enabled(&state, &scope).await?;
    let viewer = match board_viewer(&state, &scope, &settings, &headers).await {
        Ok(v) => v,
        Err(resp) => return Ok(resp),
    };
    let sub = match require_person(&viewer) {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };

    // FR-FBR-61 gate: the repository write resolves the item approved-only and
    // in the ranking pool (`resolve_pool_board_items`) in its own transaction,
    // before the insert. A miss is NotFound → 404.
    let fb = FeedbackId::from(short_code.clone());
    let outcome = state
        .board_votes
        .signals()
        .set_not_needed(&scope, &fb, sub)
        .await?;
    Ok(Json(NotNeededResponse {
        short_code,
        not_needed: true,
        upvote_cleared: Some(outcome.upvote_cleared),
    })
    .into_response())
}

/// `DELETE .../board/items/{short_code}/not-needed` — clear the caller's own
/// mark. Withdrawing a signal is never gated on the item's state, and the
/// answer is the same whether a mark existed, so it confirms nothing.
async fn public_board_not_needed_clear(
    State(state): State<AppState>,
    Path((project_id, short_code)): Path<(Uuid, String)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let scope = state.projects.open_for_submission(project_id).await?;
    let settings = ensure_board_enabled(&state, &scope).await?;
    let viewer = match board_viewer(&state, &scope, &settings, &headers).await {
        Ok(v) => v,
        Err(resp) => return Ok(resp),
    };
    let sub = match require_person(&viewer) {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    state
        .board_votes
        .signals()
        .clear_not_needed(&scope, &FeedbackId::from(short_code.clone()), sub)
        .await?;
    Ok(Json(NotNeededResponse {
        short_code,
        not_needed: false,
        upvote_cleared: None,
    })
    .into_response())
}

/// `GET .../board/priorities` — the caller's own list (FR-FBR-50/51). Items
/// that left the pool are reported once here and dropped.
async fn public_board_priorities_get(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let scope = state.projects.open_for_submission(project_id).await?;
    let settings = ensure_board_enabled(&state, &scope).await?;
    let viewer = match board_viewer(&state, &scope, &settings, &headers).await {
        Ok(v) => v,
        Err(resp) => return Ok(resp),
    };
    let sub = match require_person(&viewer) {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    let view = state
        .board_votes
        .signals()
        .load_priority_list(&scope, sub)
        .await?;
    Ok(Json(priority_list_response(view)).into_response())
}

/// `PUT .../board/priorities` — replace the caller's whole list (FR-FBR-50).
/// Every item resolves approved-only and in the pool before anything is
/// written (FR-FBR-61): a miss is `404 NotInRankingPool`, a repeat is
/// `409 DuplicateItem`, and the stored list is unchanged either way.
async fn public_board_priorities_put(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    headers: HeaderMap,
    Json(req): Json<PriorityListRequest>,
) -> Result<Response, ApiError> {
    let scope = state.projects.open_for_submission(project_id).await?;
    let settings = ensure_board_enabled(&state, &scope).await?;
    let viewer = match board_viewer(&state, &scope, &settings, &headers).await {
        Ok(v) => v,
        Err(resp) => return Ok(resp),
    };
    let sub = match require_person(&viewer) {
        Ok(s) => s,
        Err(resp) => return Ok(resp),
    };
    let mut seen = std::collections::HashSet::with_capacity(req.items.len());
    if !req.items.iter().all(|c| seen.insert(c.as_str())) {
        return Ok(error_response(StatusCode::CONFLICT, "DuplicateItem"));
    }
    let items: Vec<FeedbackId> = req.items.into_iter().map(FeedbackId::from).collect();

    match state
        .board_votes
        .signals()
        .save_priority_list(&scope, sub, &items)
        .await
    {
        Ok(view) => Ok(Json(priority_list_response(view)).into_response()),
        Err(RepoError::NotFound) => Ok(error_response(StatusCode::NOT_FOUND, "NotInRankingPool")),
        // A concurrent save of the same list (critic C-004): the caller retries.
        Err(RepoError::Conflict) => Ok(error_response(StatusCode::CONFLICT, "Conflict")),
        Err(e) => Err(e.into()),
    }
}

// ===========================================================================
// Helpers
// ===========================================================================

/// C29 inv. 2: a project that has not opted into the public board exposes
/// nothing — every board endpoint 404s. Checked BEFORE any row read so an
/// approved row never leaks from a board-disabled project. Returns the
/// settings, whose `board_access` the caller enforces (FR-FBR-43).
pub(crate) async fn ensure_board_enabled(
    state: &AppState,
    scope: &ProjectScope,
) -> Result<BoardSettings, ApiError> {
    let settings = state.projects.get_board_settings(scope).await?;
    if settings.public_board_enabled {
        Ok(settings)
    } else {
        Err(ApiError::NotFound)
    }
}

/// FR-FBR-42: `kind` ∈ bug|feature|question|other, or absent; anything else
/// is a 400 (never silently ignored, which would widen the result).
fn parse_kind_filter(raw: Option<&str>) -> Result<Option<FeedbackKind>, ApiError> {
    match raw {
        None | Some("") => Ok(None),
        Some("bug") => Ok(Some(FeedbackKind::Bug)),
        Some("feature") => Ok(Some(FeedbackKind::Feature)),
        Some("question") => Ok(Some(FeedbackKind::Question)),
        Some("other") => Ok(Some(FeedbackKind::Other)),
        Some(_) => Err(ApiError::BadRequest(
            "kind must be one of bug, feature, question, other".into(),
        )),
    }
}

fn error_response(status: StatusCode, error: &str) -> Response {
    (status, Json(json!({ "error": error }))).into_response()
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
        // Real aggregate over `feedback_board_votes` (D1, PF-BOARD-VOTING-01).
        vote_count: item.vote_count,
        accepted_at: item.accepted_at,
    }
}

fn priority_list_response(view: PriorityListView) -> PriorityListResponse {
    let influences_priority = view.entries.len() >= 2;
    PriorityListResponse {
        entries: view.entries.into_iter().map(item_response).collect(),
        unranked: view.unranked.into_iter().map(item_response).collect(),
        removed: view.removed.into_iter().map(item_response).collect(),
        removed_hidden: view.removed_hidden,
        influences_priority,
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

    #[test]
    fn kind_filter_is_strict() {
        assert_eq!(parse_kind_filter(None).unwrap(), None);
        assert_eq!(parse_kind_filter(Some("feature")).unwrap(), Some(FeedbackKind::Feature));
        assert_eq!(parse_kind_filter(Some("bug")).unwrap(), Some(FeedbackKind::Bug));
        assert!(parse_kind_filter(Some("Feature")).is_err());
        assert!(parse_kind_filter(Some("bogus")).is_err());
    }
}
