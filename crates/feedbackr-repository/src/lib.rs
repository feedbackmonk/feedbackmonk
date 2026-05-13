//! `feedbackr-repository` -- the SOLE query path for Feedbackr domain data.
//!
//! Per DEC-FBR-03, raw SQL outside this crate is a security incident. The
//! `multi-tenant-isolation-check` Verification Oracle enforces this at AST
//! grade; the `TenantScope` / `ProjectScope` newtypes (in `scope`) enforce
//! it at the type system. Callers obtain a `TenantScope` ONLY after
//! authentication; they obtain a `ProjectScope` ONLY by calling
//! `ProjectRepo::open(&scope, project_id)`, which proves
//! tenant -> project ownership.
//!
//! Lineage:
//!   - FR-FBR-01 (multi-tenant data model)
//!   - DEC-FBR-03 (sole query path) / DEC-FBR-04 (JWT-only end-user identity)
//!   - P0 plan Contract C1

pub mod email_verifications;
pub mod error;
pub mod feedback;
pub mod health;
pub mod projects;
pub mod scope;
pub mod signing_keys;
pub mod tenants;

pub use email_verifications::{EmailVerificationRepo, Redemption, SqlxEmailVerificationRepo};
pub use error::{RepoError, Result};
pub use feedback::{FeedbackRepo, SqlxFeedbackRepo};
pub use health::SqlxHealthCheck;
pub use projects::{ProjectRepo, SqlxProjectRepo};
pub use scope::{ProjectScope, TenantScope};
pub use signing_keys::{SigningKeyRepo, SqlxSigningKeyRepo};
pub use tenants::{SqlxTenantRepo, TenantRepo};
