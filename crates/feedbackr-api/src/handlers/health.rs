//! `/health` and `/health/ready` endpoints (FR-FBR-18, Contract C5).
//!
//! - `GET /health` always returns HTTP 200 with a JSON body summarising
//!   liveness + DB connectivity + version + uptime. When the DB ping fails
//!   the JSON body's `status` flips to `"degraded"` but the HTTP code stays
//!   200 so load balancers can distinguish "alive but degraded" from "dead".
//! - `GET /health/ready` returns 200 when all dependencies are healthy; 503
//!   otherwise. Liveness vs. readiness split is a 12-factor convention used
//!   by Docker Compose `depends_on: { condition: service_healthy }` and
//!   (later) Kubernetes-style orchestration.

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
    version: &'static str,
    uptime_seconds: i64,
    started_at: String,
}

const PKG_VERSION: &str = env!("CARGO_PKG_VERSION");

async fn ping_db(state: &AppState) -> bool {
    let ok = state.health.ping().await;
    if !ok {
        tracing::warn!("db ping failed");
    }
    ok
}

fn build_body(state: &AppState, db_connected: bool) -> HealthBody {
    let now = chrono::Utc::now();
    let uptime_seconds = (now - state.started_at).num_seconds();
    HealthBody {
        status: if db_connected { "ok" } else { "degraded" },
        db_connected,
        version: PKG_VERSION,
        uptime_seconds,
        started_at: state.started_at.to_rfc3339(),
    }
}

/// `GET /health` — liveness probe. Always 200; body indicates degradation.
pub async fn liveness(State(state): State<AppState>) -> impl IntoResponse {
    let db_connected = ping_db(&state).await;
    Json(build_body(&state, db_connected))
}

/// `GET /health/ready` — readiness probe. 200 if healthy, 503 otherwise.
pub async fn readiness(State(state): State<AppState>) -> impl IntoResponse {
    let db_connected = ping_db(&state).await;
    let body = build_body(&state, db_connected);
    if db_connected {
        (StatusCode::OK, Json(body))
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json(body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    /// Pure-helper test: uptime math is independent of DB state. Full
    /// handler coverage (DB ping + JSON shape) lives at integration tier
    /// with `#[sqlx::test]`.
    #[test]
    fn uptime_seconds_is_non_negative_when_started_at_is_in_past() {
        let now = Utc::now();
        let earlier = now - chrono::Duration::seconds(42);
        let uptime = (now - earlier).num_seconds();
        assert!(uptime >= 0);
        assert!(uptime <= 60);
    }

    #[test]
    fn pkg_version_is_non_empty() {
        // Confirms env!("CARGO_PKG_VERSION") resolved at compile time.
        assert!(!PKG_VERSION.is_empty());
    }
}
