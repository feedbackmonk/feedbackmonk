//! `feedbackr-core` -- pure domain types shared across all Feedbackr crates.
//!
//! No DB access, no network, no async. Plain data + minimal value-construction
//! helpers (e.g. `FeedbackId::generate`). The DB-touching layer lives in
//! `feedbackr-repository`; the request/response layer lives in `feedbackr-api`.
//!
//! Lineage: FR-FBR-01 (data model) + Contract C1 (P0 plan).

#![deny(unsafe_code)]

pub mod ids;
pub mod models;

pub use ids::{FeedbackId, SigningKeyId};
pub use models::{
    AnonSubmission, Feedback, FeedbackKind, Project, RateLimitCounter, SigningKey, Tenant,
};
