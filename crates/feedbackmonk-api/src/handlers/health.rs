//! `/health` and `/health/ready` endpoints (FR-FBR-18, Contract C5).
//!
//! - `GET /health` always returns HTTP 200 with a JSON body summarising
//!   liveness + DB connectivity. When the DB ping fails the JSON body's
//!   `status` flips to `"degraded"` but the HTTP code stays 200 so load
//!   balancers can distinguish "alive but degraded" from "dead".
//! - `GET /health/ready` returns 200 when all dependencies are healthy; 503
//!   otherwise. Liveness vs. readiness split is a 12-factor convention used
//!   by Docker Compose `depends_on: { condition: service_healthy }` and
//!   (later) Kubernetes-style orchestration.
//!
//! ## Security posture (scrutiny P2-7)
//!
//! The PUBLIC (unauthenticated) health bodies carry ONLY the liveness /
//! readiness signal (`status` + `db_connected`). They deliberately do NOT
//! expose the exact build version or process start time: an unauthenticated
//! attacker cannot use `/health` to fingerprint the running binary for
//! version-specific targeting. Detailed build info, if ever needed, belongs
//! behind the ops token (DEC-FBR-IMPL-11) rather than on the public probe.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Serialize;

use crate::state::AppState;

#[derive(Serialize)]
struct HealthBody {
    status: &'static str,
    db_connected: bool,
}

async fn ping_db(state: &AppState) -> bool {
    let ok = state.health.ping().await;
    if !ok {
        tracing::warn!("db ping failed");
    }
    ok
}

fn build_body(db_connected: bool) -> HealthBody {
    HealthBody {
        status: if db_connected { "ok" } else { "degraded" },
        db_connected,
    }
}

/// `GET /health` — liveness probe. Always 200; body indicates degradation.
pub async fn liveness(State(state): State<AppState>) -> impl IntoResponse {
    let db_connected = ping_db(&state).await;
    Json(build_body(db_connected))
}

/// `GET /health/ready` — readiness probe. 200 if healthy, 503 otherwise.
pub async fn readiness(State(state): State<AppState>) -> impl IntoResponse {
    let db_connected = ping_db(&state).await;
    let body = build_body(db_connected);
    if db_connected {
        (StatusCode::OK, Json(body))
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json(body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The public body carries only the liveness signal — no version, uptime,
    /// or start time (scrutiny P2-7: don't fingerprint the binary for an
    /// unauthenticated attacker). Full handler coverage (DB ping + JSON shape)
    /// lives at integration tier with `#[sqlx::test]`.
    #[test]
    fn body_reports_ok_when_db_connected() {
        let body = build_body(true);
        assert_eq!(body.status, "ok");
        assert!(body.db_connected);
    }

    #[test]
    fn body_reports_degraded_when_db_down() {
        let body = build_body(false);
        assert_eq!(body.status, "degraded");
        assert!(!body.db_connected);
    }

    /// Guards the P2-7 invariant at the type level: the serialized public body
    /// exposes ONLY `status` + `db_connected` (no `version` / `started_at` /
    /// `uptime_seconds`).
    #[test]
    fn public_body_has_no_version_or_start_time() {
        let json = serde_json::to_value(build_body(true)).unwrap();
        let obj = json.as_object().unwrap();
        assert_eq!(obj.len(), 2, "public health body must be exactly 2 fields");
        assert!(obj.contains_key("status"));
        assert!(obj.contains_key("db_connected"));
        assert!(!obj.contains_key("version"));
        assert!(!obj.contains_key("started_at"));
        assert!(!obj.contains_key("uptime_seconds"));
    }
}
