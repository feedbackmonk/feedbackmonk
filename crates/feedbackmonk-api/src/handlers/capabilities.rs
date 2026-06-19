//! `GET /api/v1/capabilities` — public capability/version discovery.
//!
//! The stable, forward-compatible way for a consumer (e.g. GitCellar) to detect
//! which optional features this feedbackmonk deployment supports BEFORE wiring
//! to them. Unauthenticated and unscoped — it returns only static, public
//! capability metadata (no tenant/project data), so it needs no auth and no
//! CORS allowlist entry (server-to-server / same-origin probe).
//!
//! Detection contract (documented in `docs/integrations/gitcellar-adoption.md`
//! §8): a consumer should treat the presence of a string in `capabilities` as
//! authoritative — NOT parse the semver `version`. `version` is informational.
//! New capabilities are added to the array; removed ones disappear. This makes
//! the negotiation additive and resilient to version-scheme changes.

use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

use feedbackmonk_core::Sentiment;

use crate::handlers::solicitation::DEFAULT_SOLICITATION_COOLDOWN_DAYS;
use crate::state::AppState;

/// The API version (the `feedbackmonk-api` crate version at compile time).
pub const API_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Capability tokens advertised by this build. Consumers match on these
/// strings, not the version.
pub const CAPABILITIES: &[&str] = &[
    // Capability 1 (FR-FBR-28): first-class sentiment + sentiment-only submits.
    "feedback.sentiment",
    "feedback.body-optional",
    "feedback.sentiment-trend",
    // Capability 2 (FR-FBR-29): durable per-user solicitation state.
    "solicitation.v1",
    // Pre-existing end-user surfaces, advertised for completeness.
    "feedback.my-feedback",
];

pub async fn capabilities() -> Json<Value> {
    let sentiment_values: Vec<&str> = Sentiment::ALL.iter().map(|s| s.as_db_str()).collect();
    Json(json!({
        "version": API_VERSION,
        "capabilities": CAPABILITIES,
        "feedback": {
            "sentiment": {
                "field": "sentiment",
                "values": sentiment_values,
                // A submission may now carry a body, a sentiment, or both.
                "body_optional": true,
            }
        },
        "solicitation": {
            "events": ["prompted", "dismissed", "gave_feedback", "opted_out"],
            "states": ["eligible", "prompted", "dismissed", "gave_feedback", "opted_out"],
            "cooldown_days_default": DEFAULT_SOLICITATION_COOLDOWN_DAYS,
        }
    }))
}

/// Capability-discovery subtree. Public; no CORS layer (metadata-only probe).
pub fn capabilities_router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/capabilities", get(capabilities))
        .with_state(state)
}
