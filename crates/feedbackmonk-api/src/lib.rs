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
pub mod cors;
pub mod crash_correlation;
pub mod email;
pub mod error;
pub mod handlers;
pub mod roadmap_voting_cache;
pub mod router;
pub mod state;
pub mod storage;

pub use cors::{parse_origins, public_cors_layer};
pub use crash_correlation::{
    CorrelationOutcome, CrashCorrelator, CrashEvent, GlitchtipCorrelator,
};
pub use error::ApiError;
pub use handlers::attachments::{attachments_router, scrub_log_for_storage, AttachmentState};
pub use handlers::admin_feedback::routes as admin_feedback_routes;
// Public Feedback Board + Moderation Gate (Contracts C28/C29).
pub use handlers::board::board_router;
pub use handlers::moderation::routes as moderation_router;
pub use handlers::admin_ops::ops_router;
pub use handlers::admin_tier::admin_tier_router;
pub use handlers::capabilities::capabilities_router;
pub use handlers::feedback::submission_router;
pub use handlers::me_feedback::me_feedback_router;
pub use handlers::solicitation::solicitation_router;
pub use handlers::promote::routes as promote_router;
pub use handlers::roadmap::{admin_roadmap_router, roadmap_router};
pub use handlers::widget_config::widget_config_router;
pub use handlers::work_orders::{work_order_admin_router, work_order_runner_router};
// P5a (Worker B, Contract C23/C24): cluster/recommendation/sweep admin routers +
// the testable pure surface (clustering heuristic + source_refs exfil validator)
// re-exported for the C24 corpus integration test.
pub use handlers::clusters::{
    assign_cluster_on_submit, derive_label, jaccard, normalize_tokens, router as cluster_admin_router,
    CLUSTER_JACCARD_THRESHOLD,
};
pub use handlers::recommendations::{
    router as recommendation_admin_router, validate_source_refs,
};
pub use handlers::sweeps::router as sweep_admin_router;
// P5b (Contract C25, FR-FBR-24): runner-token lifecycle admin router.
pub use handlers::runner_tokens::router as runner_tokens_admin_router;
pub use roadmap_voting_cache::{
    spawn_refresh_tick as spawn_voting_cache_refresh, VotingCache, VOTING_CACHE_TTL_SECS,
};
pub use router::router as worker_a_router;
pub use state::AppState;
