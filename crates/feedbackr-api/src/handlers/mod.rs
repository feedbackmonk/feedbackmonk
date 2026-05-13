//! Request handlers.

// Worker A's endpoints (signup, verify-email, projects, signing-keys).
pub mod projects;
pub mod signing_keys;
pub mod signup;
pub mod verify_email;

// Worker B's endpoint (public submission API, FR-FBR-03/05/06).
pub mod feedback;

// Stage 3: FR-FBR-18 health + observability.
pub mod health;
