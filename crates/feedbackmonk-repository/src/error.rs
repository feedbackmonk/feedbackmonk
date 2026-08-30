//! Repository error type. Variants chosen so callers can map cleanly to
//! HTTP status (`NotFound` -> 404, `Conflict` -> 409, `TenantProjectMismatch` -> 403).

use thiserror::Error;

#[derive(Debug, Error)]
pub enum RepoError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("not found")]
    NotFound,

    #[error("conflict (uniqueness or state violation)")]
    Conflict,

    /// A `tenant_domains` row held a `kind`/`status` value outside its CHECK
    /// constraint (migration 00030) — schema and `feedbackmonk-core` have
    /// drifted apart. Surfaced as a conflict rather than a 500 because the row
    /// is genuinely in a state the code cannot serve; it is a deployment
    /// mismatch, not a caller error.
    #[error("unrecognised tenant_domains column value")]
    DomainValue(#[from] feedbackmonk_core::DomainValueError),

    /// A submit reused an `Idempotency-Key` (same
    /// `(project_id, submitter, key)`) with DIFFERENT content than the original
    /// submission. Distinct from a legit retry (identical content → the
    /// original submission's id is returned). Maps to HTTP 409. Security
    /// scrutiny P1-3 / M6.
    #[error("idempotency key reused with different content")]
    IdempotencyKeyReuse,

    /// `ProjectRepo::open` was called with a `project_id` that does not
    /// belong to the tenant in the supplied `TenantScope`. This is a
    /// hard authorization boundary -- treat as 403, log at WARN, and
    /// consider it a tenant-isolation defense activation, not a bug.
    #[error("project does not belong to tenant in scope")]
    TenantProjectMismatch,
}

pub type Result<T> = std::result::Result<T, RepoError>;
