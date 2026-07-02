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
