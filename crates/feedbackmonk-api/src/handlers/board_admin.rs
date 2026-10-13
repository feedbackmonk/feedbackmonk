#![allow(clippy::doc_markdown)] // module-doc references HTTP paths / column names verbatim

//! Owner surfaces for end-user prioritization — Contract C44 (FR-FBR-43/49/55/
//! 56/58). Wire contract: `docs/planning/handoffs/prioritization-contracts.md`.
//! Tests: `tests/board_admin.rs` (behaviour) and `tests/board_admin_privacy.rs`
//! (FR-FBR-56).
//!
//! Registered from `moderation::routes` (classified `admin` in `build_app`, so
//! no new router and no oracle table change):
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
//! marked an item not needed — only counts and the fitted order. The
//! repository's `board_priority_inputs` already strips the user from every
//! list; this module never sees one. The priority view never merges votes,
//! not-needed and priority into one score (FR-FBR-55).

use std::collections::{BTreeMap, BTreeSet};

use axum::extract::{Path, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use feedbackmonk_core::{fit_priority, FeedbackId, FeedbackStatus, PriorityFitConfig};
use feedbackmonk_repository::{PoolItemStats, PriorityInputs, BOARD_NOTE_MAX_CHARS};

use crate::auth::AdminSession;
use crate::error::ApiError;
use crate::handlers::admin_feedback::sole_project_scope;
use crate::state::AppState;

/// One fitted item in the owner's priority view (FR-FBR-55). `rank` is the
/// 1-based position in the fitted order.
///
/// `uncertainty` is the fit's (Wald) standard error on the item's log-strength
/// — the uncertainty OF THE ESTIMATE, not a confidence that grows with votes.
/// It does not shrink monotonically with evidence: an item that wins every
/// comparison, unanimously, can carry a larger one than an item with a single
/// comparison, because a lopsided record pins the strength's direction but
/// not how far it goes. Label it as such in any UI.
///
/// Votes and not-needed sit beside the rank as raw counts, never combined
/// with it.
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
    /// Users whose list holds two or more pool items (so states a pair).
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

// ---------- Priority view (FR-FBR-55/56) ----------

/// Fit the pool and shape the owner's view. Pure: the handler's only I/O is
/// the repository read before it. `computed_at` is passed in so the result is
/// a function of its inputs.
#[must_use]
pub fn build_priority_view(
    inputs: PriorityInputs,
    computed_at: DateTime<Utc>,
) -> PriorityViewResponse {
    let PriorityInputs { pool, lists } = inputs;
    let keys: Vec<String> = pool
        .iter()
        .map(|p| p.feedback_id.as_str().to_string())
        .collect();
    let key_set: BTreeSet<&str> = keys.iter().map(String::as_str).collect();
    let lists: Vec<Vec<String>> = lists
        .into_iter()
        .map(|l| l.into_iter().map(|f| f.as_str().to_string()).collect())
        .collect();

    // A list states a pair only with two or more distinct pool items.
    let contributing = lists
        .iter()
        .filter(|l| {
            l.iter()
                .filter(|k| key_set.contains(k.as_str()))
                .collect::<BTreeSet<_>>()
                .len()
                >= 2
        })
        .count();

    let fit = fit_priority(&keys, &lists, &PriorityFitConfig::default());

    let stats: BTreeMap<String, PoolItemStats> = pool
        .into_iter()
        .map(|p| (p.feedback_id.as_str().to_string(), p))
        .collect();

    let ranked = fit
        .ranked
        .into_iter()
        .zip(1u32..)
        .filter_map(|(item, rank)| {
            let s = stats.get(&item.key)?;
            Some(PriorityRankedWire {
                short_code: item.key,
                body_excerpt: s.body_excerpt.clone(),
                status: s.status,
                rank,
                rankers: item.rankers,
                uncertainty: item.std_error,
                vote_count: s.vote_count,
                not_needed_count: s.not_needed_count,
            })
        })
        .collect();
    let unranked = fit
        .unranked
        .into_iter()
        .filter_map(|item| {
            let s = stats.get(&item.key)?;
            Some(PriorityUnrankedWire {
                short_code: item.key,
                body_excerpt: s.body_excerpt.clone(),
                status: s.status,
                rankers: item.rankers,
                vote_count: s.vote_count,
                not_needed_count: s.not_needed_count,
            })
        })
        .collect();

    PriorityViewResponse {
        computed_at,
        contributing_rankers: u32::try_from(contributing).unwrap_or(u32::MAX),
        ranked,
        unranked,
    }
}

/// `GET /api/v1/admin/projects/{project_id}/priority`. Computed on request;
/// the fit is CPU-bound (O(n³) in touched items), so it runs off the async
/// workers.
pub async fn get_priority_view(
    State(state): State<AppState>,
    session: AdminSession,
    Path(project_id): Path<Uuid>,
) -> Result<Json<PriorityViewResponse>, ApiError> {
    let scope = state.projects.open(&session.scope, project_id).await?;
    let inputs = state
        .board_votes
        .signals()
        .board_priority_inputs(&scope)
        .await?;
    let computed_at = Utc::now();
    let view = tokio::task::spawn_blocking(move || build_priority_view(inputs, computed_at))
        .await
        .map_err(|e| ApiError::Internal(format!("priority fit task failed: {e}")))?;
    Ok(Json(view))
}

// ---------- Board note (FR-FBR-58) ----------

fn note_response(
    feedback_id: &FeedbackId,
    note: Option<feedbackmonk_repository::BoardNote>,
) -> BoardNoteResponse {
    BoardNoteResponse {
        short_code: feedback_id.as_str().to_string(),
        note: note.map(|n| BoardNoteWire {
            body: n.body,
            updated_at: n.updated_at,
        }),
    }
}

/// `GET /api/v1/admin/feedback/{feedback_id}/board-note`. 404 when the item is
/// not on the board (not approved, or not this tenant's).
pub async fn get_board_note(
    State(state): State<AppState>,
    session: AdminSession,
    Path(feedback_id): Path<String>,
) -> Result<Json<BoardNoteResponse>, ApiError> {
    let scope = sole_project_scope(&state, &session.scope).await?;
    let fb = FeedbackId::from(feedback_id);
    let note = state
        .board_votes
        .notes()
        .get_board_note(&scope, &fb)
        .await?;
    Ok(Json(note_response(&fb, note)))
}

/// `PUT /api/v1/admin/feedback/{feedback_id}/board-note`: set or replace the
/// one note. The body is trimmed and must be 1..=4000 chars (400 otherwise).
pub async fn put_board_note(
    State(state): State<AppState>,
    session: AdminSession,
    Path(feedback_id): Path<String>,
    Json(req): Json<BoardNoteRequest>,
) -> Result<Json<BoardNoteResponse>, ApiError> {
    let body = req.body.trim();
    if body.is_empty() || body.chars().count() > BOARD_NOTE_MAX_CHARS {
        return Err(ApiError::BadRequest(format!(
            "board note must be 1..={BOARD_NOTE_MAX_CHARS} characters after trimming"
        )));
    }
    let scope = sole_project_scope(&state, &session.scope).await?;
    let fb = FeedbackId::from(feedback_id);
    let note = state
        .board_votes
        .notes()
        .set_board_note(&scope, &fb, body)
        .await?;
    Ok(Json(note_response(&fb, Some(note))))
}

/// `DELETE /api/v1/admin/feedback/{feedback_id}/board-note`: clear it. Always
/// answers `note: null`, whether or not one existed.
pub async fn delete_board_note(
    State(state): State<AppState>,
    session: AdminSession,
    Path(feedback_id): Path<String>,
) -> Result<Json<BoardNoteResponse>, ApiError> {
    let scope = sole_project_scope(&state, &session.scope).await?;
    let fb = FeedbackId::from(feedback_id);
    state
        .board_votes
        .notes()
        .clear_board_note(&scope, &fb)
        .await?;
    Ok(Json(note_response(&fb, None)))
}
