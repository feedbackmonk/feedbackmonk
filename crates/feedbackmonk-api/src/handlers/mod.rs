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

// Shared voter-resolution chokepoint for the public voting surfaces (roadmap
// C15 + board C30). Extracted from roadmap.rs so the anon/JWT voter primitive
// is implemented ONCE (migration 00007/00018 invariant #2). PF-BOARD-VOTING-01.
pub mod voting_common;

// Public Feedback Board + Moderation Gate (Contracts C28/C29):
//   moderation — admin moderate + queue + board-settings (C28, AdminSession, no CORS)
//   board      — public approved-only board read (C29, CORS-exposed)
pub mod board;
pub mod moderation;

// P3 Stage 1: admin tier-status endpoint (FR-FBR-14, Contract C17).
pub mod admin_tier;

// Post-v1: operator tier + widget brand-override mutation (DEC-FBR-IMPL-11).
// Bearer-token-guarded (OpsAuth) operator surface — NOT tenant-self-serve.
pub mod admin_ops;

// Gap #1 (GitCellar parity): feedback attachment multipart upload.
pub mod attachments;

// Gap #4 (GitCellar parity): end-user (JWT-sub-scoped) my-feedback read API.
pub mod me_feedback;

// GitCellar in-app solicitation (FR-FBR-29): durable per-user solicitation
// state API (JWT end-user surface, mirrors me_feedback; no CORS).
pub mod solicitation;

// GitCellar capability negotiation (FR-FBR-28/27): public capability/version
// discovery (`GET /api/v1/capabilities`).
pub mod capabilities;

// P5a (Contract C22, FR-FBR-22 / FR-FBR-25a): work-order API + approval state
// machine — THE security boundary between public feedback and code execution
// (Worker A). Admin routes behind AdminSession; runner routes behind the runner
// write-token seam (Q14). Both merge WITHOUT CORS.
pub mod work_orders;

// P5a (Contract C23/C24, FR-FBR-19 / FR-FBR-20): clustering-on-submit +
// merge/split + analyst sweep/digest + recommendation ingestion (Worker B).
// Admin routes behind AdminSession, merged WITHOUT CORS.
pub mod clusters;
pub mod recommendations;
pub mod sweeps;

// P5b (Contract C25, FR-FBR-24): runner-token lifecycle admin surface
// (list/register/revoke). Behind AdminSession; merged WITHOUT CORS. The
// runner-token VERIFY seam lives in `work_orders.rs` (verify_runner_token).
pub mod runner_tokens;
