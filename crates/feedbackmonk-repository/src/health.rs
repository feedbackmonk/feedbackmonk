//! Health-check probe for the Postgres pool.
//!
//! `SqlxHealthCheck::ping` runs a trivial `SELECT 1` round-trip and returns
//! whether it succeeded. Used by Stage 3's `/health` and `/health/ready`
//! endpoints (FR-FBR-18, Contract C5).
//!
//! This lives inside `feedbackmonk-repository` so the `multi-tenant-isolation-check`
//! oracle's Probe A (raw-SQL ban outside this crate) is satisfied. The `ping`
//! method takes only `&self` — no scope arg — which Probe B accepts as a
//! special case (a `&self`-only method has no caller-supplied identifier to
//! check, and nothing tenant-bound flows through it; this is observability,
//! not a domain operation).
//!
//! The constructor `new` is allow-listed alongside the other repo
//! constructors (stores a `PgPool` handle; performs no queries).

use sqlx::PgPool;

#[derive(Clone)]
pub struct SqlxHealthCheck {
    pool: PgPool,
}

impl SqlxHealthCheck {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// `true` if a `SELECT 1` round-trip succeeded against the pool.
    pub async fn ping(&self) -> bool {
        sqlx::query("SELECT 1").execute(&self.pool).await.is_ok()
    }
}
