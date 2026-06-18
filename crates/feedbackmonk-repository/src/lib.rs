//! `feedbackmonk-repository` -- the SOLE query path for feedbackmonk domain data.
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

pub mod analysis_sweeps;
pub mod attachments;
pub mod clusters;
pub mod email_verifications;
pub mod error;
pub mod feedback;
pub mod feedback_replies;
pub mod feedback_status_history;
pub mod health;
pub mod projects;
pub mod recommendations;
pub mod roadmap_items;
pub mod roadmap_votes;
pub mod runner_token_revocations;
pub mod runner_tokens;
pub mod scope;
pub mod signing_keys;
pub mod tenants;
pub mod tier_quota;
pub mod work_order_events;
pub mod work_orders;

pub use analysis_sweeps::{AnalysisSweep, AnalysisSweepRepo, SqlxAnalysisSweepRepo, SweepOutcome};
pub use attachments::{
    AttachmentKind, AttachmentRepo, AttachmentRow, NewAttachment, SqlxAttachmentRepo,
};
pub use clusters::{ClusterRepo, FeedbackCluster, SqlxClusterRepo};
pub use email_verifications::{EmailVerificationRepo, Redemption, SqlxEmailVerificationRepo};
pub use error::{RepoError, Result};
pub use feedback::{
    EndUserFeedback, FeedbackListItem, FeedbackRepo, SqlxFeedbackRepo, StatusHistoryRow,
};
pub use feedback_replies::{FeedbackReply, FeedbackReplyRepo, ReplyVisibility, SqlxFeedbackReplyRepo};
pub use feedback_status_history::{FeedbackStatusHistoryRepo, SqlxFeedbackStatusHistoryRepo};
pub use health::SqlxHealthCheck;
pub use projects::{ProjectRepo, SqlxProjectRepo};
pub use recommendations::{
    NewRecommendation, Recommendation, RecommendationRepo, SqlxRecommendationRepo,
};
pub use roadmap_items::{
    NewRoadmapItem, RoadmapItemPatch, RoadmapItemRepo, SqlxRoadmapItemRepo,
};
pub use roadmap_votes::{
    RetractOutcome, RoadmapVoteRepo, SqlxRoadmapVoteRepo, DEFAULT_RETRACTION_WINDOW,
};
pub use runner_token_revocations::{
    RunnerTokenRevocation, RunnerTokenRevocationRepo, SqlxRunnerTokenRevocationRepo,
};
pub use runner_tokens::{
    NewRunnerToken, RunnerTokenRecord, RunnerTokenRepo, SqlxRunnerTokenRepo,
};
pub use scope::{ProjectScope, TenantScope};
pub use signing_keys::{SigningKeyRepo, SqlxSigningKeyRepo};
pub use tenants::{EmailTenantBrand, SqlxTenantRepo, TenantRepo, WidgetBrandOverride};
pub use tier_quota::{
    QuotaStatus, SqlxTierQuotaRepo, TierQuotaRepo, TierStatus, TierUsage,
    ROLLING_FEEDBACK_WINDOW_DAYS,
};
pub use work_order_events::{
    NewWorkOrderEvent, SqlxWorkOrderEventRepo, WorkOrderEvent, WorkOrderEventRepo,
};
pub use work_orders::{
    NewWorkOrder, SqlxWorkOrderRepo, WorkOrder, WorkOrderRepo, WorkOrderStatePatch,
};
