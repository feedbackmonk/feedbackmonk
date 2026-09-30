//! Crash-event detail for one feedback row (GitCellar parity gap #2, contract
//! `docs/integrations/gitcellar-adoption.md` §5.6).
//!
//! `GET /api/v1/admin/feedback/:feedback_id/crash` resolves the row's stored
//! `crash_event_id` against the crash tracker (Glitchtip, pull-mode) and returns
//! the banner shape the contract freezes. It is a separate request from the
//! feedback detail on purpose: the tracker is a third-party service, so a slow
//! or down tracker delays only the banner, never the triage view.
//!
//! Admin-only. The resolved detail carries the tracker's internal links and
//! code locations, which are the data controller's to see; no public, board or
//! end-user surface serves it.
//!
//! Best-effort by contract: an unconfigured or unreachable tracker answers
//! `200 {"status":"unavailable"}`, never an error, so the console degrades to
//! "crash details unavailable" and the id itself is still shown.

use std::sync::Arc;

use axum::extract::{FromRef, Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use feedbackmonk_core::FeedbackId;

use crate::auth::session::AdminSession;
use crate::crash_correlation::{CorrelationOutcome, CrashCorrelator, CrashEvent};
use crate::error::ApiError;
use crate::handlers::admin_feedback::sole_project_scope;
use crate::state::AppState;

/// Sub-state: carries `AppState` so `AdminSession` extracts, plus the
/// correlator. `None` when the four `FEEDBACKMONK_GLITCHTIP_*` settings are not
/// all set -- the endpoint then answers `unavailable` for every row.
#[derive(Clone)]
pub struct CrashState {
    pub app: AppState,
    pub correlator: Option<Arc<dyn CrashCorrelator>>,
}

impl FromRef<CrashState> for AppState {
    fn from_ref(st: &CrashState) -> Self {
        st.app.clone()
    }
}

pub fn crash_admin_router(state: CrashState) -> Router {
    Router::new()
        .route("/api/v1/admin/feedback/:feedback_id/crash", get(get_crash))
        .with_state(state)
}

/// Wire shape. `status` is one of `none` (the row carries no crash id),
/// `linked` (resolved; `crash` present), `not_found` (the tracker has no such
/// event) or `unavailable` (tracker unconfigured, unreachable or unusable).
#[derive(Debug, Clone, Serialize)]
pub struct CrashResponse {
    pub status: &'static str,
    pub crash_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crash: Option<CrashEvent>,
}

async fn get_crash(
    State(state): State<CrashState>,
    session: AdminSession,
    Path(feedback_id): Path<String>,
) -> Result<Json<CrashResponse>, ApiError> {
    let project_scope = sole_project_scope(&state.app, &session.scope).await?;
    let fb_id = FeedbackId::from(feedback_id);
    let (feedback, _history) = state.app.feedback.get_with_history(&project_scope, &fb_id).await?;

    let Some(crash_event_id) = feedback.crash_event_id.filter(|id| !id.trim().is_empty()) else {
        return Ok(Json(CrashResponse { status: "none", crash_event_id: None, crash: None }));
    };
    let outcome = match &state.correlator {
        Some(c) => c.correlate(&crash_event_id).await,
        None => CorrelationOutcome::Unavailable,
    };
    let (status, crash) = match outcome {
        CorrelationOutcome::Linked(ev) => ("linked", Some(ev)),
        CorrelationOutcome::NotFound => ("not_found", None),
        CorrelationOutcome::Unavailable => ("unavailable", None),
    };
    Ok(Json(CrashResponse { status, crash_event_id: Some(crash_event_id), crash }))
}
