//! Request handlers.

// Worker A's endpoints (signup, verify-email, projects, signing-keys).
pub mod projects;
pub mod signing_keys;
pub mod signup;
pub mod verify_email;

// Admin (tenant) password login -- re-auth after the verify-email session
// lapses (DEC-FBR-IMPL-10, post-v1 GitCellar admin-dashboard enabler).
pub mod login;

// Worker B's endpoint (public submission API, FR-FBR-03/05/06).
pub mod feedback;

// Stage 3: FR-FBR-18 health + observability.
pub mod health;

// P1 Stage 2: admin status workflow + replies (Contracts C7 + C8).
pub mod admin_feedback;

// P2: promote-to-roadmap admin action (FR-FBR-12, Contract C16, Worker C).
pub mod promote;

// P2: widget runtime config endpoint (FR-FBR-04, Contract C12, Worker A).
pub mod widget_config;

// P2: public + admin roadmap endpoints (FR-FBR-11 + FR-FBR-13, Contract C15, Worker B).
pub mod roadmap;

// P3 Stage 1: admin tier-status endpoint (FR-FBR-14, Contract C17).
pub mod admin_tier;

// Gap #1 (GitCellar parity): feedback attachment multipart upload.
pub mod attachments;

// Gap #4 (GitCellar parity): end-user (JWT-sub-scoped) my-feedback read API.
pub mod me_feedback;
