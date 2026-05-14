//! `feedbackmonk-core` -- pure domain types shared across all feedbackmonk crates.
//!
//! No DB access, no network, no async. Plain data + minimal value-construction
//! helpers (e.g. `FeedbackId::generate`). The DB-touching layer lives in
//! `feedbackmonk-repository`; the request/response layer lives in `feedbackmonk-api`.
//!
//! Lineage: FR-FBR-01 (data model) + Contract C1 (P0 plan).

#![deny(unsafe_code)]

pub mod ids;
pub mod models;
pub mod roadmap;
pub mod status;

pub use ids::{FeedbackId, SigningKeyId};
pub use models::{
    AnonSubmission, Feedback, FeedbackKind, Project, RateLimitCounter, SigningKey, Tenant,
    WidgetBrand,
};
pub use roadmap::{RoadmapItem, RoadmapItemStatus, RoadmapVote, RoadmapVoterMode};
pub use status::{legal_transitions_from, FeedbackStatus, TransitionError};
