//! `feedbackr-api` -- HTTP layer for Feedbackr (FR-FBR-02..06).
//!
//! Library + binary. The library form exposes `build_router` and `AppState`
//! so integration tests (and Worker B's submission handler) can wire the
//! same router the binary uses.
//!
//! Module split:
//!   - `state` -- `AppState` (shared application context: pool, repos, mailer, secrets)
//!   - `auth`  -- password hashing (argon2) + signed-cookie admin session
//!   - `email` -- `Mailer` trait + Mailpit (dev) / SMTP-env (prod) impls
//!   - `error` -- `ApiError` HTTP error type
//!   - `handlers` -- request handlers (signup, `verify_email`, projects, `signing_keys`)
//!   - `router` -- composes Worker A's handler subtree into one Router

pub mod auth;
pub mod email;
pub mod error;
pub mod handlers;
pub mod router;
pub mod state;

pub use error::ApiError;
pub use handlers::admin_feedback::routes as admin_feedback_routes;
pub use handlers::feedback::submission_router;
pub use router::router as worker_a_router;
pub use state::AppState;
