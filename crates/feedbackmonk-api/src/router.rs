//! Worker A router subtree: signup, verify-email, projects, signing-keys.
//!
//! Worker B exposes their own `router(state)` from their handler module(s);
//! `main.rs` merges both into the single binary Router.

use axum::routing::{delete, get, post};
use axum::Router;

use crate::handlers::{health, login, projects, signing_keys, signup, verify_email};
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/signup", post(signup::signup))
        .route("/api/v1/verify-email", post(verify_email::verify))
        // Admin password login -- re-auth after the one-time verify-email
        // session lapses (DEC-FBR-IMPL-10). ConnectInfo-dependent (rate-limit
        // bucket keys on client IP); served via into_make_service_with_connect_info.
        .route("/api/v1/login", post(login::login))
        .route(
            "/api/v1/projects",
            post(projects::create).get(projects::list),
        )
        .route(
            "/api/v1/projects/:project_id/signing-keys",
            post(signing_keys::register),
        )
        .route(
            "/api/v1/projects/:project_id/signing-keys/:key_id",
            delete(signing_keys::deactivate),
        )
        // FR-FBR-18 (Contract C5): liveness (always 200, body indicates degradation)
        // + readiness (200 healthy / 503 degraded; 12-factor split for orchestrators).
        .route("/health", get(health::liveness))
        .route("/health/ready", get(health::readiness))
        .with_state(state)
}
