//! `feedbackmonk-api` -- HTTP layer for feedbackmonk (FR-FBR-02..13).
//!
//! Library + binary. The library form exposes the composed router and
//! `AppState` so integration tests can wire the same router the binary uses.
//!
//! Module split:
//!   - `state` -- `AppState` (shared application context: pool, repos, mailer, secrets, voting cache)
//!   - `auth`  -- password hashing (argon2) + signed-cookie admin session
//!   - `email` -- `Mailer` trait + Mailpit (dev) / SMTP-env (prod) impls
//!   - `error` -- `ApiError` HTTP error type
//!   - `handlers` -- request handlers (signup, `verify_email`, projects, `signing_keys`,
//!     feedback submission + admin, `widget_config`, roadmap public + admin, promote)
//!   - `roadmap_voting_cache` -- 60s-TTL per-project vote tallies refreshed by a
//!     background task; consumed by the public-roadmap top-voted endpoint
//!   - `router` -- composes the signup/onboarding subtree (Stage 1 carry-over)

pub mod auth;
pub mod email;
pub mod error;
pub mod handlers;
pub mod roadmap_voting_cache;
pub mod router;
pub mod state;

pub use error::ApiError;
pub use handlers::admin_feedback::routes as admin_feedback_routes;
pub use handlers::admin_tier::admin_tier_router;
pub use handlers::feedback::submission_router;
pub use handlers::promote::routes as promote_router;
pub use handlers::roadmap::{admin_roadmap_router, roadmap_router};
pub use handlers::widget_config::widget_config_router;
pub use roadmap_voting_cache::{
    spawn_refresh_tick as spawn_voting_cache_refresh, VotingCache, VOTING_CACHE_TTL_SECS,
};
pub use router::router as worker_a_router;
pub use state::AppState;
