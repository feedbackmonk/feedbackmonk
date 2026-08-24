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

/// Default cooldown after a prompt the user ENGAGED with — answered, or
/// explicitly ended (≈ twice a year). Override with
/// `FEEDBACKMONK_SOLICITATION_COOLDOWN_DAYS`.
pub const DEFAULT_SOLICITATION_COOLDOWN_DAYS: i64 = 182;

/// Default cooldown after a prompt the user set aside rather than answered
/// (`dismissed` — the ✕, or an explicit "ask me later" control). Override with
/// `FEEDBACKMONK_SOLICITATION_SNOOZE_DAYS`.
///
/// WHY THIS IS SEPARATE: until now the cooldown was status-BLIND — every
/// non-opt-out outcome waited the same 182 days, so closing the prompt because
/// you were busy was indistinguishable from having answered it. That makes a
/// consumer-side "ask me later" affordance impossible to honour: the control
/// would say "later" and mean "in six months". Setting it aside is a much
/// weaker signal than answering, so it earns a much shorter wait — while
/// `opted_out` stays terminal and answering still rests for the full period.
pub const DEFAULT_SOLICITATION_SNOOZE_DAYS: i64 = 14;

/// Compile-time invariant: a prompt the user merely set aside must rest for a
/// SHORTER period than one they answered. If these defaults are ever edited so
/// the snooze meets or exceeds the full cooldown, "ask me later" silently starts
/// meaning "ask me never" -- so this fails the BUILD rather than a test.
const _SNOOZE_IS_SHORTER_THAN_COOLDOWN: () = assert!(
    DEFAULT_SOLICITATION_SNOOZE_DAYS < DEFAULT_SOLICITATION_COOLDOWN_DAYS,
);

fn cooldown_days() -> i64 {
    env_days(
        "FEEDBACKMONK_SOLICITATION_COOLDOWN_DAYS",
        DEFAULT_SOLICITATION_COOLDOWN_DAYS,
    )
}

fn snooze_days() -> i64 {
    env_days(
        "FEEDBACKMONK_SOLICITATION_SNOOZE_DAYS",
        DEFAULT_SOLICITATION_SNOOZE_DAYS,
    )
}

fn env_days(var: &str, default: i64) -> i64 {
    std::env::var(var)
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|d| *d >= 1)
        .unwrap_or(default)
}

/// The cooldown that applies to a record, given the outcome it last recorded.
/// `dismissed` (set aside) gets the short snooze; everything else gets the full
/// period. `opted_out` never consults this — it is terminal.
fn cooldown_for(status: SolicitationStatus) -> i64 {
    match status {
        SolicitationStatus::Dismissed => snooze_days(),
        _ => cooldown_days(),
    }
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
    /// Days a consumer must wait after a prompt the user engaged with.
    pub cooldown_days: i64,
    /// Days a consumer must wait after a prompt the user merely set aside
    /// (`dismissed`). Shorter than `cooldown_days` — this is what makes an
    /// "ask me later" control mean what it says.
    pub snooze_days: i64,
    /// The cooldown ACTUALLY applied to this record, in days — i.e. whichever
    /// of the two above matches the recorded status. Sent so a consumer never
    /// has to re-derive the policy branch to explain the wait.
    pub applied_cooldown_days: i64,
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
    // The applied cooldown depends on the recorded status (a set-aside prompt
    // rests briefly; an answered one rests the full period), so it is resolved
    // per-record below rather than once up front.
    let applied = record.map_or_else(cooldown_days, |r| cooldown_for(r.status));
    let policy = SolicitationPolicy {
        cooldown_days: cooldown_days(),
        snooze_days: snooze_days(),
        applied_cooldown_days: applied,
    };
    let cooldown = Duration::days(applied);

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

    // ---- status-aware cooldown -------------------------------------------
    // A prompt SET ASIDE rests briefly; one the user ANSWERED rests the full
    // period. Before this split the cooldown was status-blind, so an
    // "ask me later" control could only ever mean "in six months".

    #[test]
    fn a_set_aside_prompt_becomes_eligible_again_after_the_short_snooze() {
        let elapsed = DEFAULT_SOLICITATION_SNOOZE_DAYS + 1;
        // Sanity: the window must sit strictly inside the full cooldown, or
        // this test would pass for the wrong reason.
        assert!(elapsed < DEFAULT_SOLICITATION_COOLDOWN_DAYS);

        let r = build_response(Some(&rec(
            SolicitationStatus::Dismissed,
            Some(Utc::now() - Duration::days(elapsed)),
        )));
        assert!(
            r.eligible,
            "a dismissed prompt must be re-askable after the snooze, not the full cooldown"
        );
        assert_eq!(r.policy.applied_cooldown_days, DEFAULT_SOLICITATION_SNOOZE_DAYS);
    }

    #[test]
    fn an_answered_prompt_still_rests_the_full_cooldown() {
        // The SAME elapsed time that frees a dismissed prompt must NOT free an
        // answered one -- this is the invertible half of the test above.
        let elapsed = DEFAULT_SOLICITATION_SNOOZE_DAYS + 1;
        let r = build_response(Some(&rec(
            SolicitationStatus::GaveFeedback,
            Some(Utc::now() - Duration::days(elapsed)),
        )));
        assert!(
            !r.eligible,
            "answering must not re-arm the prompt on the snooze window"
        );
        assert_eq!(r.policy.applied_cooldown_days, DEFAULT_SOLICITATION_COOLDOWN_DAYS);
    }

    #[test]
    fn opting_out_is_terminal_regardless_of_elapsed_time() {
        // Opt-out outranks every cooldown branch, including the short one.
        let r = build_response(Some(&rec(
            SolicitationStatus::OptedOut,
            Some(Utc::now() - Duration::days(DEFAULT_SOLICITATION_COOLDOWN_DAYS * 10)),
        )));
        assert!(!r.eligible);
        assert!(r.next_eligible_at.is_none());
    }

    #[test]
    fn policy_reports_both_windows_and_the_one_applied() {
        let r = build_response(Some(&rec(
            SolicitationStatus::Dismissed,
            Some(Utc::now() - Duration::days(1)),
        )));
        assert_eq!(r.policy.cooldown_days, DEFAULT_SOLICITATION_COOLDOWN_DAYS);
        assert_eq!(r.policy.snooze_days, DEFAULT_SOLICITATION_SNOOZE_DAYS);
        assert_eq!(r.policy.applied_cooldown_days, DEFAULT_SOLICITATION_SNOOZE_DAYS);
        // The snooze-shorter-than-cooldown invariant is enforced at COMPILE time
        // (see SNOOZE_IS_SHORTER_THAN_COOLDOWN above) rather than asserted here:
        // both values are constants, so a runtime assert never fails a test that
        // a build would already have rejected.
    }
}
