//! `feedbackmonk-core` -- pure domain types shared across all feedbackmonk crates.
//!
//! No DB access, no network, no async. Plain data + minimal value-construction
//! helpers (e.g. `FeedbackId::generate`). The DB-touching layer lives in
//! `feedbackmonk-repository`; the request/response layer lives in `feedbackmonk-api`.
//!
//! Lineage: FR-FBR-01 (data model) + Contract C1 (P0 plan).

#![deny(unsafe_code)]

pub mod action_type;
pub mod ids;
pub mod models;
pub mod roadmap;
pub mod status;
pub mod tier;
pub mod work_order;

pub use action_type::ActionType;
pub use ids::{FeedbackId, SigningKeyId};
pub use models::{
    AnonSubmission, Feedback, FeedbackKind, KeyClass, Project, RateLimitCounter, SigningKey,
    Tenant, WidgetBrand,
};
pub use roadmap::{RoadmapItem, RoadmapItemStatus, RoadmapVote, RoadmapVoterMode};
pub use status::{legal_transitions_from, FeedbackStatus, TransitionError};
pub use tier::{tier_quotas, ResourceKind, Tier, TierParseError, TierQuotas};
pub use work_order::{
    is_legal_transition, legal_transitions_from as work_order_legal_transitions_from,
    WorkOrderState, WorkOrderTransitionError,
};
