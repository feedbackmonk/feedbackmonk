//! `GET` + `POST /api/v1/projects/{project_id}/me/solicitation` — the durable
//! per-user **feedback-solicitation state** API (Capability 2, FR-FBR-29).
//!
//! A consumer (e.g. GitCellar Desktop) shows an ambient "got a minute for
//! feedback?" nudge to engaged users. feedbackmonk owns the DURABLE record of
//! whether a given end-user may be asked, keyed by the stable JWT `sub`, so the
//! consumer can honor "ask at most ~twice/year, honor dismissal, honor opt-out"
//! WITHOUT that state resetting on client reinstall.
//!
//! ## Auth (DEC-FBR-04)
//!
//! JWT-only, EXACTLY like the submit + `/me/feedback` surfaces: project scope
//! via `ProjectRepo::open_for_submission`, then a Bearer JWT verified against
//! the project's active IDENTITY-class signing keys (`aud == project_id`). The
//! verified `sub` is the sole identity; the durable record is keyed by it.
//! Anonymous solicitation state is intentionally NOT supported — a durable
//! "don't ask me again" requires a stable identity.
//!
//! ## State machine
//!
//! `eligible → prompted → {dismissed | gave_feedback | opted_out}`. The legal
//! transitions live in `feedbackmonk_core::solicitation`; this handler applies
//! them and maps an illegal transition to `409`. `opted_out` is terminal.
//!
//! ## Eligibility / frequency cap
//!
//! The handler computes `eligible` + `next_eligible_at` from the record and a
//! cooldown window (default 182 days ≈ "twice a year"), configurable via
//! `FEEDBACKMONK_SOLICITATION_COOLDOWN_DAYS`. A sub is eligible iff it has
//! never been prompted, OR the last prompt is older than the cooldown — and
//! NEVER if it has opted out.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{Path, State};
use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use feedbackmonk_core::{
    apply_solicitation_event, KeyClass, SolicitationError, SolicitationEvent, SolicitationStatus,
};
use feedbackmonk_jwt::{verify_with_leeway as jwt_verify_with_leeway, JwtError, VerifiedClaims};
use feedbackmonk_repository::{
    ProjectScope, SolicitationRecord, SolicitationRepo, SqlxSolicitationRepo,
};

use crate::error::ApiError;
use crate::state::AppState;

/// Default cooldown between prompts (≈ twice a year). Override with
/// `FEEDBACKMONK_SOLICITATION_COOLDOWN_DAYS`.
pub const DEFAULT_SOLICITATION_COOLDOWN_DAYS: i64 = 182;

fn cooldown_days() -> i64 {
    std::env::var("FEEDBACKMONK_SOLICITATION_COOLDOWN_DAYS")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|d| *d >= 1)
        .unwrap_or(DEFAULT_SOLICITATION_COOLDOWN_DAYS)
}

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct EventRequest {
    /// One of `prompted | dismissed | gave_feedback | opted_out`.
    pub event: SolicitationEvent,
}

#[derive(Debug, Clone, Serialize)]
pub struct SolicitationPolicy {
    pub cooldown_days: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SolicitationResponse {
    pub status: SolicitationStatus,
    /// Whether the consumer may show a prompt right now (cooldown + opt-out
    /// applied). This is the field the consumer keys its decision off.
    pub eligible: bool,
    pub prompt_count: i64,
    /// Timestamp of the most recent prompt, or `null` if never prompted.
    pub prompted_at: Option<DateTime<Utc>>,
    /// Timestamp of the most recent state-changing event, or `null` if the sub
    /// has no record yet.
    pub last_event_at: Option<DateTime<Utc>>,
    /// When the sub becomes eligible again, or `null` if eligible now OR
    /// permanently ineligible (opted out).
    pub next_eligible_at: Option<DateTime<Utc>>,
    pub policy: SolicitationPolicy,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /api/v1/projects/{project_id}/me/solicitation` — read the caller's
/// current solicitation state + computed eligibility.
pub async fn get_solicitation(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let (scope, claims) = match authenticate(&state, project_id, &headers).await {
        Ok(v) => v,
        Err(resp) => return Ok(resp),
    };

    let repo = SqlxSolicitationRepo::new(state.pool.clone());
    let record = repo.get(&scope, &claims.sub).await?;
    Ok((StatusCode::OK, Json(build_response(record.as_ref()))).into_response())
}

/// `POST /api/v1/projects/{project_id}/me/solicitation` — record a solicitation
/// event and return the updated state.
pub async fn post_solicitation_event(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    headers: HeaderMap,
    Json(req): Json<EventRequest>,
) -> Result<Response, ApiError> {
    let (scope, claims) = match authenticate(&state, project_id, &headers).await {
        Ok(v) => v,
        Err(resp) => return Ok(resp),
    };

    let repo = SqlxSolicitationRepo::new(state.pool.clone());
    let current = repo.get(&scope, &claims.sub).await?;
    let current_status = current.as_ref().map_or(SolicitationStatus::Eligible, |r| r.status);
    let current_prompt_count = current.as_ref().map_or(0, |r| r.prompt_count);
    let current_prompted_at = current.as_ref().and_then(|r| r.prompted_at);

    // Validate the transition (state machine lives in feedbackmonk-core).
    let new_status = match apply_solicitation_event(current_status, req.event) {
        Ok(s) => s,
        Err(e) => return Ok(solicitation_error_response(&e)),
    };

    // `prompted` bumps the count + stamps the prompt time (drives the cooldown);
    // every other event leaves those untouched.
    let (prompt_count, prompted_at) = if req.event == SolicitationEvent::Prompted {
        (current_prompt_count + 1, Some(Utc::now()))
    } else {
        (current_prompt_count, current_prompted_at)
    };

    let record = repo
        .upsert(&scope, &claims.sub, new_status, prompt_count, prompted_at)
        .await?;

    Ok((StatusCode::OK, Json(build_response(Some(&record)))).into_response())
}

// ---------------------------------------------------------------------------
// Eligibility computation
// ---------------------------------------------------------------------------

/// Build the wire response from a record (or its absence = default `eligible`).
fn build_response(record: Option<&SolicitationRecord>) -> SolicitationResponse {
    let policy = SolicitationPolicy {
        cooldown_days: cooldown_days(),
    };
    let cooldown = Duration::days(policy.cooldown_days);

    let Some(r) = record else {
        // No record yet: the sub is eligible and has never been prompted.
        return SolicitationResponse {
            status: SolicitationStatus::Eligible,
            eligible: true,
            prompt_count: 0,
            prompted_at: None,
            last_event_at: None,
            next_eligible_at: None,
            policy,
        };
    };

    let (eligible, next_eligible_at) = if r.status.is_opted_out() {
        // Opted out: permanently ineligible.
        (false, None)
    } else {
        match r.prompted_at {
            // Never prompted ⇒ eligible now.
            None => (true, None),
            Some(p) => {
                let next = p + cooldown;
                if Utc::now() >= next {
                    (true, None)
                } else {
                    (false, Some(next))
                }
            }
        }
    };

    SolicitationResponse {
        status: r.status,
        eligible,
        prompt_count: r.prompt_count,
        prompted_at: r.prompted_at,
        last_event_at: Some(r.last_event_at),
        next_eligible_at,
        policy,
    }
}

// ---------------------------------------------------------------------------
// Auth helper (mirrors handlers/me_feedback.rs)
// ---------------------------------------------------------------------------

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
        .list_active_for_class(&scope, KeyClass::Identity)
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

fn extract_bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let stripped = value.strip_prefix("Bearer ")?;
    if stripped.is_empty() {
        return None;
    }
    Some(stripped.to_string())
}

fn jwt_error_response(err: &JwtError) -> Response {
    let body = Json(json!({ "error": err.variant_name() }));
    (StatusCode::UNAUTHORIZED, body).into_response()
}

/// Map a [`SolicitationError`] to a `409` with a disambiguating body.
fn solicitation_error_response(err: &SolicitationError) -> Response {
    let error = match err {
        SolicitationError::IllegalTransition { .. } => "IllegalTransition",
        SolicitationError::OptedOut => "OptedOut",
    };
    (
        StatusCode::CONFLICT,
        Json(json!({ "error": error, "detail": err.to_string() })),
    )
        .into_response()
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

/// Solicitation-state subtree. Mirrors `me_feedback_router` (JWT end-user
/// surface). Merged WITHOUT the credentialed CORS layer — like `/me/feedback`,
/// it is driven by the consumer's own client (e.g. GitCellar Desktop), not a
/// browser embed.
pub fn solicitation_router(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/v1/projects/:project_id/me/solicitation",
            get(get_solicitation).post(post_solicitation_event),
        )
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use feedbackmonk_repository::SolicitationRecord;

    fn rec(status: SolicitationStatus, prompted_at: Option<DateTime<Utc>>) -> SolicitationRecord {
        SolicitationRecord {
            status,
            prompt_count: 1,
            prompted_at,
            last_event_at: Utc::now(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn no_record_is_eligible() {
        let r = build_response(None);
        assert!(r.eligible);
        assert_eq!(r.status, SolicitationStatus::Eligible);
        assert_eq!(r.prompt_count, 0);
        assert!(r.next_eligible_at.is_none());
        assert!(r.last_event_at.is_none());
    }

    #[test]
    fn opted_out_is_never_eligible() {
        let r = build_response(Some(&rec(SolicitationStatus::OptedOut, Some(Utc::now()))));
        assert!(!r.eligible);
        assert!(r.next_eligible_at.is_none());
    }

    #[test]
    fn recent_prompt_is_not_eligible_with_next_time() {
        let r = build_response(Some(&rec(
            SolicitationStatus::Dismissed,
            Some(Utc::now() - Duration::days(1)),
        )));
        assert!(!r.eligible);
        assert!(r.next_eligible_at.is_some());
    }

    #[test]
    fn old_prompt_past_cooldown_is_eligible_again() {
        let r = build_response(Some(&rec(
            SolicitationStatus::Dismissed,
            Some(Utc::now() - Duration::days(DEFAULT_SOLICITATION_COOLDOWN_DAYS + 1)),
        )));
        assert!(r.eligible);
        assert!(r.next_eligible_at.is_none());
    }
}
