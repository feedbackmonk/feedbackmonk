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

use feedbackmonk_core::{Rating, Sentiment, Severity};

use crate::handlers::solicitation::{
    DEFAULT_SOLICITATION_COOLDOWN_DAYS, DEFAULT_SOLICITATION_SNOOZE_DAYS,
};
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
    // Phase A (GitCellar contract build-out) — additive end-user capabilities.
    // Each string is authoritative; GitCellar feature-detects on presence.
    "feedback.delete",       // A1: DELETE …/me/feedback/{id} erasure (body + attachment bytes purged).
    "feedback.reply_state",  // A3: me/feedback items carry updated_at + reply_count (+ ?since=).
    "feedback.export",       // A5: GET …/me/feedback/export (portability companion to delete).
    "feedback.severity",     // A4a: first-class `severity` submit field (low|medium|high|blocker).
    // Additive 1-5 rating (migration 00029). NOT a widening of `sentiment` —
    // both fields coexist and a rating derives its sentiment, so a consumer
    // that never sends a rating is completely unaffected.
    "feedback.rating",
    "feedback.idempotency",  // A4b: `Idempotency-Key` header dedupe on submit.
    "feedback.attachments",  // A2: attachment list + tenant-scoped download (upload pre-existed).
    // P1-16 / M1: user-level "forget me" — DELETE …/me erases the caller's ENTIRE
    // footprint (all feedback + solicitation state + roadmap/board votes).
    "feedback.erase_all",
    // FR-FBR-32/33: the commercial hosting shape. BUILD-level, like every token
    // above — they say this binary implements host-based tenant resolution and
    // tier-gated custom domains, not that a given deployment has switched them
    // on. Whether THIS deployment has is self-describing and needs no token:
    // `GET /api/v1/public/site` 404s on a host that is not tenant-bound, and
    // `GET /api/v1/admin/hosting` returns a null `cname_target` when no root
    // domain is configured.
    "hosting.subdomains",
    "hosting.custom_domain",
];

pub async fn capabilities() -> Json<Value> {
    let sentiment_values: Vec<&str> = Sentiment::ALL.iter().map(|s| s.as_db_str()).collect();
    let severity_values: Vec<&str> = Severity::ALL.iter().map(|s| s.as_db_str()).collect();
    Json(json!({
        "version": API_VERSION,
        "capabilities": CAPABILITIES,
        "feedback": {
            "sentiment": {
                "field": "sentiment",
                "values": sentiment_values,
                // A submission may now carry a body, a sentiment, or both.
                "body_optional": true,
            },
            // A4a: first-class severity submit field (optional, tenant-generic).
            "severity": {
                "field": "severity",
                "values": severity_values,
            },
            // Additive 1-5 rating (migration 00029). `derives_sentiment` tells a
            // consumer it may send ONLY a rating: the server fills `sentiment`
            // from it, so the 3-point contract stays populated either way.
            "rating": {
                "field": "rating",
                "min": Rating::MIN,
                "max": Rating::MAX,
                "derives_sentiment": true,
            },
            // A4b: submit dedupe on flaky-network retry.
            "idempotency": {
                "header": "Idempotency-Key",
            },
            // A2: attachment upload (pre-existing) + list + tenant-scoped download.
            "attachments": {
                "max_images": 4,
                "max_image_bytes": 5_242_880,
                "image_types": ["image/png", "image/jpeg", "image/webp"],
                "log_kinds": ["service_log", "console_log"],
            },
            // A3: end-user list carries per-row reply-state for unseen-reply detection.
            "reply_state": {
                "fields": ["updated_at", "reply_count"],
                "since_param": "since",
            },
            // A1/A5: erasure + export on the end-user surface.
            "delete": true,
            "export": true,
            // P1-16 / M1: DELETE …/me user-level "forget me" (full footprint).
            "erase_all": true,
        },
        "solicitation": {
            "events": ["prompted", "dismissed", "gave_feedback", "opted_out"],
            "states": ["eligible", "prompted", "dismissed", "gave_feedback", "opted_out"],
            "cooldown_days_default": DEFAULT_SOLICITATION_COOLDOWN_DAYS,
            // A prompt merely set aside (`dismissed`) rests for this much
            // shorter period than one the user engaged with — which is what
            // lets a consumer offer a truthful "ask me later" control.
            "snooze_days_default": DEFAULT_SOLICITATION_SNOOZE_DAYS,
        }
    }))
}

/// Capability-discovery subtree. Public; no CORS layer (metadata-only probe).
pub fn capabilities_router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/capabilities", get(capabilities))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// capability-advertisement-parity (static leg): the `capabilities` array
    /// and the structured `feedback` descriptors must not drift. If a
    /// `feedback.*` string is advertised, its descriptor must be present and
    /// non-empty. The behavioural leg — that each advertised route is actually
    /// MOUNTED and behaves — is covered by the per-capability integration
    /// tests: `me_feedback_delete.rs` (`feedback.delete`), `me_feedback_export.rs`
    /// (`feedback.export`), `me_feedback_reply_state.rs` (`feedback.reply_state`),
    /// `submit_idempotency.rs` (`feedback.severity` + `feedback.idempotency`), and
    /// `attachment_list_download.rs` (`feedback.attachments`).
    #[tokio::test]
    async fn advertised_capabilities_match_descriptors() {
        // The full expected set — a change here is a deliberate contract change.
        let expected = [
            "feedback.sentiment",
            "feedback.body-optional",
            "feedback.sentiment-trend",
            "solicitation.v1",
            "feedback.my-feedback",
            "feedback.delete",
            "feedback.reply_state",
            "feedback.export",
            "feedback.severity",
            "feedback.rating",
            "feedback.idempotency",
            "feedback.attachments",
            "feedback.erase_all",
            // FR-FBR-32/33 (DEC-FBR-13/14): the commercial hosting shape.
            "hosting.subdomains",
            "hosting.custom_domain",
        ];
        assert_eq!(
            CAPABILITIES.len(),
            expected.len(),
            "CAPABILITIES drifted from the expected set"
        );
        for e in expected {
            assert!(CAPABILITIES.contains(&e), "missing capability: {e}");
        }

        let body = capabilities().await.0;
        let fb = &body["feedback"];
        // Each advertised feedback.* capability has a truthful descriptor.
        assert!(fb["severity"]["values"].as_array().is_some_and(|v| v.len() == 4));
        assert_eq!(fb["idempotency"]["header"], "Idempotency-Key");
        assert_eq!(fb["attachments"]["max_images"].as_i64(), Some(4));
        assert!(fb["reply_state"]["fields"].as_array().is_some_and(|v| v.len() == 2));
        assert_eq!(fb["delete"], true);
        assert_eq!(fb["export"], true);
        assert_eq!(fb["erase_all"], true);
    }
}
