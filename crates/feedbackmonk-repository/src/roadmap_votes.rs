//! Roadmap-votes repository — Contract C14 backing surface.
//!
//! Mirrors the schema in `migrations/00007_roadmap_votes.sql`. Every public
//! method takes `&ProjectScope` as its first non-self argument (Probe B
//! compliance). The constructor `SqlxRoadmapVoteRepo::new` is allowlisted
//! as a structural mirror of `SqlxRoadmapItemRepo::new` (and of
//! `SqlxFeedbackRepo::new` before that).
//!
//! Hard invariants (per Contract C14 + module C14 in p2-fanout-contracts.md):
//!
//! 1. `cast` returns `Err(RepoError::Conflict)` on duplicate `(item_id, voter_id)`.
//!    The handler maps to HTTP 409 `{"error": "AlreadyVoted"}`. NOT silent
//!    upsert.
//!
//! 2. `voter_id` resolution is the caller's responsibility (anon mode →
//!    `hex(AnonGate::token_hash(ip, cookie, project_id))`; jwt mode →
//!    `verified_claims.sub`). The repo treats it as an opaque string.
//!
//! 3. `retract` enforces the 60-second window: it reads `cast_at`, checks
//!    `now() - cast_at <= window`, then DELETEs. If the row doesn't exist,
//!    returns `RepoError::NotFound`; if outside the window, returns
//!    `RepoError::Conflict` (handler maps to 403 `RetractionWindowExpired`).
//!
//! Lineage:
//!   FR-FBR-13 (voting + aggregator)
//!   Contract C14 (P2 plan §Interface Contracts)
//!   docs/planning/handoffs/p2-fanout-contracts.md §C14

use std::time::Duration;

use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use feedbackmonk_core::{RoadmapVote, RoadmapVoterMode};

use crate::error::{RepoError, Result};
use crate::scope::ProjectScope;

/// Default vote-retraction window. The final value flows through the API
/// layer's `RETRACTION_WINDOW_SECS` constant.
// `from_secs(60)` is kept intentionally: this constant mirrors the seconds-based
// `RETRACTION_WINDOW_SECS` API constant, so the seconds unit is the readable form here.
#[allow(clippy::duration_suboptimal_units)]
pub const DEFAULT_RETRACTION_WINDOW: Duration = Duration::from_secs(60);

/// Outcome of `retract`. The `Removed` variant carries the row that was
/// deleted so the handler can include `retracted_at` in the response without
/// a second DB round-trip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetractOutcome {
    /// Row deleted; window check passed.
    Removed { retracted_at: chrono::DateTime<chrono::Utc> },
    /// No vote existed for this `(item_id, voter_id)` in scope.
    NotFound,
    /// Window expired; row NOT deleted.
    WindowExpired { cast_at: chrono::DateTime<chrono::Utc> },
}

#[async_trait]
pub trait RoadmapVoteRepo: Send + Sync {
    /// Cast a vote. Returns `Err(RepoError::Conflict)` on duplicate
    /// `(item_id, voter_id)` (Hard Invariant 1). Cross-tenant inserts are
    /// prevented by the scope filter on `item_id` membership.
    async fn cast(
        &self,
        scope: &ProjectScope,
        item_id: Uuid,
        voter_id: &str,
        voter_mode: RoadmapVoterMode,
    ) -> Result<RoadmapVote>;

    /// Retract a vote. Enforces the retraction window. See `RetractOutcome`
    /// variants for the three outcomes.
    async fn retract(
        &self,
        scope: &ProjectScope,
        item_id: Uuid,
        voter_id: &str,
        window: Duration,
    ) -> Result<RetractOutcome>;

    /// Single-item vote count. O(log n) via the `(item_id)` index.
    async fn vote_count_for_item(&self, scope: &ProjectScope, item_id: Uuid) -> Result<i64>;

    /// Whether a given voter has already voted for an item. Used by the
    /// detail endpoint to render the vote button in the right state.
    async fn has_voted(
        &self,
        scope: &ProjectScope,
        item_id: Uuid,
        voter_id: &str,
    ) -> Result<bool>;
}

#[derive(Clone)]
pub struct SqlxRoadmapVoteRepo {
    pool: PgPool,
}

impl SqlxRoadmapVoteRepo {
    /// Constructor — allowlisted as a structural mirror of
    /// `SqlxFeedbackRepo::new`. Pre-authorized per GUIDE.md §8.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl RoadmapVoteRepo for SqlxRoadmapVoteRepo {
    async fn cast(
        &self,
        scope: &ProjectScope,
        item_id: Uuid,
        voter_id: &str,
        voter_mode: RoadmapVoterMode,
    ) -> Result<RoadmapVote> {
        // Reject cross-tenant by joining-via-WHERE: the INSERT validates
        // the item belongs to this scope via a sub-select. Postgres won't
        // raise a FK error on cross-tenant because the FK on item_id only
        // proves "some roadmap item with this id exists" — we need a scope
        // check too. Do that in one round-trip via an INSERT...SELECT.
        //
        // The unique-violation on (item_id, voter_id) maps to Conflict.
        let row = match sqlx::query!(
            r#"
            INSERT INTO roadmap_votes (tenant_id, project_id, item_id, voter_id, voter_mode)
            SELECT $1, $2, i.id, $4, $5
            FROM roadmap_items AS i
            WHERE i.id = $3 AND i.tenant_id = $1 AND i.project_id = $2
            RETURNING id, tenant_id, project_id, item_id, voter_id, voter_mode, cast_at
            "#,
            scope.tenant_id(),
            scope.project_id(),
            item_id,
            voter_id,
            voter_mode.as_db_str(),
        )
        .fetch_optional(&self.pool)
        .await
        {
            Ok(Some(r)) => r,
            // No row returned -> the INSERT...SELECT found no matching item
            // in scope. Cross-tenant or missing item.
            Ok(None) => return Err(RepoError::NotFound),
            Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                return Err(RepoError::Conflict);
            }
            Err(e) => return Err(e.into()),
        };

        Ok(RoadmapVote {
            id: row.id,
            tenant_id: row.tenant_id,
            project_id: row.project_id,
            item_id: row.item_id,
            voter_id: row.voter_id,
            voter_mode: RoadmapVoterMode::from_db_str(&row.voter_mode),
            cast_at: row.cast_at,
        })
    }

    async fn retract(
        &self,
        scope: &ProjectScope,
        item_id: Uuid,
        voter_id: &str,
        window: Duration,
    ) -> Result<RetractOutcome> {
        // Two-step inside one txn so the window check is consistent: read
        // the cast_at FOR UPDATE, decide, then DELETE (or not).
        let mut tx = self.pool.begin().await?;

        let row = sqlx::query!(
            r#"
            SELECT v.cast_at
            FROM roadmap_votes AS v
            WHERE v.tenant_id = $1
              AND v.project_id = $2
              AND v.item_id = $3
              AND v.voter_id = $4
            FOR UPDATE
            "#,
            scope.tenant_id(),
            scope.project_id(),
            item_id,
            voter_id,
        )
        .fetch_optional(&mut *tx)
        .await?;

        let Some(row) = row else {
            tx.rollback().await?;
            return Ok(RetractOutcome::NotFound);
        };

        let now = chrono::Utc::now();
        let elapsed = now.signed_duration_since(row.cast_at);
        // Compare at nanosecond precision via chrono::Duration so zero-second
        // windows reject any non-negative elapsed time (test fixture relies
        // on this; production windows are 60s+ where seconds resolution is
        // already correct).
        let window_chrono = chrono::Duration::from_std(window).unwrap_or(chrono::Duration::MAX);
        if elapsed > window_chrono {
            tx.rollback().await?;
            return Ok(RetractOutcome::WindowExpired { cast_at: row.cast_at });
        }

        sqlx::query!(
            r#"
            DELETE FROM roadmap_votes
            WHERE tenant_id = $1
              AND project_id = $2
              AND item_id = $3
              AND voter_id = $4
            "#,
            scope.tenant_id(),
            scope.project_id(),
            item_id,
            voter_id,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(RetractOutcome::Removed { retracted_at: now })
    }

    async fn vote_count_for_item(&self, scope: &ProjectScope, item_id: Uuid) -> Result<i64> {
        let row = sqlx::query!(
            r#"
            SELECT count(*) AS "count!"
            FROM roadmap_votes
            WHERE tenant_id = $1 AND project_id = $2 AND item_id = $3
            "#,
            scope.tenant_id(),
            scope.project_id(),
            item_id,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.count)
    }

    async fn has_voted(
        &self,
        scope: &ProjectScope,
        item_id: Uuid,
        voter_id: &str,
    ) -> Result<bool> {
        let row = sqlx::query!(
            r#"
            SELECT 1 AS "one!"
            FROM roadmap_votes
            WHERE tenant_id = $1 AND project_id = $2 AND item_id = $3 AND voter_id = $4
            "#,
            scope.tenant_id(),
            scope.project_id(),
            item_id,
            voter_id,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::{ProjectRepo, SqlxProjectRepo};
    use crate::roadmap_items::{NewRoadmapItem, RoadmapItemRepo, SqlxRoadmapItemRepo};
    use crate::tenants::{SqlxTenantRepo, TenantRepo};
    use feedbackmonk_core::RoadmapItemStatus;

    async fn seed_project_scope(pool: &PgPool, email: &str) -> ProjectScope {
        let trepo = SqlxTenantRepo::new(pool.clone());
        let prepo = SqlxProjectRepo::new(pool.clone());
        let t = trepo.create(email, "h").await.unwrap();
        let scope = trepo.scope_for(t.id).await.unwrap();
        let p = prepo.create(&scope, "Proj", "proj").await.unwrap();
        prepo.open(&scope, p.id).await.unwrap()
    }

    async fn seed_item(pool: &PgPool, scope: &ProjectScope, slug: &str) -> Uuid {
        let repo = SqlxRoadmapItemRepo::new(pool.clone());
        let item = repo
            .create(
                scope,
                &NewRoadmapItem {
                    slug,
                    title: "T",
                    body: "B",
                    status: RoadmapItemStatus::Considering,
                    origin_feedback_id: None,
                    created_by: Uuid::new_v4(),
                },
            )
            .await
            .unwrap();
        item.id
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn cast_round_trips(pool: PgPool) {
        let repo = SqlxRoadmapVoteRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "rt@example.com").await;
        let item = seed_item(&pool, &scope, "dark-mode").await;

        let vote = repo
            .cast(&scope, item, "voter-1", RoadmapVoterMode::Jwt)
            .await
            .unwrap();
        assert_eq!(vote.voter_id, "voter-1");
        assert_eq!(vote.voter_mode, RoadmapVoterMode::Jwt);
        assert_eq!(vote.item_id, item);

        let count = repo.vote_count_for_item(&scope, item).await.unwrap();
        assert_eq!(count, 1);
        assert!(repo.has_voted(&scope, item, "voter-1").await.unwrap());
        assert!(!repo.has_voted(&scope, item, "voter-2").await.unwrap());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn cast_duplicate_returns_conflict(pool: PgPool) {
        let repo = SqlxRoadmapVoteRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "dup@example.com").await;
        let item = seed_item(&pool, &scope, "dark-mode").await;

        repo.cast(&scope, item, "v1", RoadmapVoterMode::Anon)
            .await
            .unwrap();
        let err = repo
            .cast(&scope, item, "v1", RoadmapVoterMode::Anon)
            .await
            .unwrap_err();
        assert!(matches!(err, RepoError::Conflict));

        // Count stayed at 1 — no silent upsert.
        let count = repo.vote_count_for_item(&scope, item).await.unwrap();
        assert_eq!(count, 1);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn cast_cross_tenant_item_returns_not_found(pool: PgPool) {
        let repo = SqlxRoadmapVoteRepo::new(pool.clone());
        let s1 = seed_project_scope(&pool, "owner1@example.com").await;
        let s2 = seed_project_scope(&pool, "owner2@example.com").await;
        let item_s1 = seed_item(&pool, &s1, "dark-mode").await;

        // s2 trying to cast a vote on s1's item must NotFound.
        let err = repo
            .cast(&s2, item_s1, "v1", RoadmapVoterMode::Jwt)
            .await
            .unwrap_err();
        assert!(matches!(err, RepoError::NotFound));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn retract_inside_window_removes_vote(pool: PgPool) {
        let repo = SqlxRoadmapVoteRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "ret@example.com").await;
        let item = seed_item(&pool, &scope, "dark-mode").await;

        repo.cast(&scope, item, "v1", RoadmapVoterMode::Anon)
            .await
            .unwrap();

        let outcome = repo
            .retract(&scope, item, "v1", DEFAULT_RETRACTION_WINDOW)
            .await
            .unwrap();
        assert!(matches!(outcome, RetractOutcome::Removed { .. }));
        assert_eq!(repo.vote_count_for_item(&scope, item).await.unwrap(), 0);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn retract_with_no_prior_vote_returns_not_found(pool: PgPool) {
        let repo = SqlxRoadmapVoteRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "ret-none@example.com").await;
        let item = seed_item(&pool, &scope, "dark-mode").await;

        let outcome = repo
            .retract(&scope, item, "never-voted", DEFAULT_RETRACTION_WINDOW)
            .await
            .unwrap();
        assert!(matches!(outcome, RetractOutcome::NotFound));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn retract_outside_window_keeps_vote(pool: PgPool) {
        let repo = SqlxRoadmapVoteRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "ret-late@example.com").await;
        let item = seed_item(&pool, &scope, "dark-mode").await;

        repo.cast(&scope, item, "v1", RoadmapVoterMode::Anon)
            .await
            .unwrap();

        // Window of 0s means any retract after cast is outside the window.
        let outcome = repo
            .retract(&scope, item, "v1", Duration::from_secs(0))
            .await
            .unwrap();
        assert!(
            matches!(outcome, RetractOutcome::WindowExpired { .. }),
            "got {outcome:?}"
        );
        // Row still there.
        assert_eq!(repo.vote_count_for_item(&scope, item).await.unwrap(), 1);
    }
}
