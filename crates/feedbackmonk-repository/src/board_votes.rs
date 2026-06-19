//! Board-votes repository — Contract C30 backing surface (PF-BOARD-VOTING-01).
//!
//! The board sibling of [`crate::roadmap_votes`]. Mirrors the schema in
//! `migrations/00018_feedback_board_votes.sql`, keyed on `feedback_id` (an
//! approved board row) instead of `item_id`. A NEW table, NOT a polymorphic
//! generalization of `roadmap_votes` (DEC-FBR-IMPL-21) — the live
//! `roadmap_voting_cache` + roadmap path stay byte-for-byte untouched.
//!
//! Every public method takes `&ProjectScope` as its first non-self argument
//! (multi-tenant isolation). The `voter_mode` is the shared
//! [`feedbackmonk_core::RoadmapVoterMode`] — board voting reuses roadmap's
//! voter-resolution chokepoint verbatim (the shared `voting_common` module in
//! the API layer), so anon vs. jwt resolution is identical (00007/00018 inv #2).
//!
//! Hard invariants (mirror `roadmap_votes` C14):
//!
//! 1. `cast` returns `Err(RepoError::Conflict)` on duplicate
//!    `(feedback_id, voter_id)`. The handler maps to HTTP 409
//!    `{"error": "AlreadyVoted"}`. NOT a silent upsert.
//!
//! 2. `voter_id` resolution is the caller's responsibility (anon mode →
//!    `hex(AnonGate::token_hash(ip, cookie, project_id))`; jwt mode →
//!    `verified_claims.sub`). The repo treats it as an opaque string.
//!
//! 3. `retract` enforces the retraction window: it reads `cast_at`, checks
//!    `now() - cast_at <= window`, then DELETEs. Missing row → `NotFound`;
//!    outside the window → `WindowExpired` (handler maps to 403).
//!
//! The MODERATION gate (plan D2) is enforced one layer up: the vote handlers
//! resolve the target feedback through an approved-only SQL filter
//! (`FeedbackRepo::resolve_approved_board_feedback_id`) BEFORE calling `cast`,
//! so a non-approved / board-disabled row is structurally un-votable. The
//! `feedback_id` passed here is therefore always an approved, in-scope row.
//!
//! Lineage:
//!   PF-BOARD-VOTING-01 / DEC-FBR-IMPL-21
//!   Contract C30 (plan §Interface Contract)
//!   Mirrors `crate::roadmap_votes` (Contract C14)

use std::time::Duration;

use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use feedbackmonk_core::RoadmapVoterMode;

use crate::error::{RepoError, Result};
use crate::roadmap_votes::RetractOutcome;
use crate::scope::ProjectScope;

/// A row of `feedback_board_votes` (Contract C30). Shape-mirrors
/// [`feedbackmonk_core::RoadmapVote`] but lives in the repository layer (like
/// [`crate::BoardItem`]) — board votes are a repo-level projection, not a
/// core domain type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardVote {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub feedback_id: Uuid,
    pub voter_id: String,
    pub voter_mode: RoadmapVoterMode,
    pub cast_at: chrono::DateTime<chrono::Utc>,
}

#[async_trait]
pub trait BoardVoteRepo: Send + Sync {
    /// Cast a board vote. Returns `Err(RepoError::Conflict)` on duplicate
    /// `(feedback_id, voter_id)` (Hard Invariant 1). Cross-tenant inserts are
    /// prevented by the scope filter on `feedback_id` membership; an unknown /
    /// out-of-scope `feedback_id` yields `RepoError::NotFound`.
    async fn cast(
        &self,
        scope: &ProjectScope,
        feedback_id: Uuid,
        voter_id: &str,
        voter_mode: RoadmapVoterMode,
    ) -> Result<BoardVote>;

    /// Retract a board vote. Enforces the retraction window. See
    /// [`RetractOutcome`] for the three outcomes.
    async fn retract(
        &self,
        scope: &ProjectScope,
        feedback_id: Uuid,
        voter_id: &str,
        window: Duration,
    ) -> Result<RetractOutcome>;

    /// Single-feedback vote count. O(log n) via the `(feedback_id)` index.
    async fn vote_count_for_feedback(
        &self,
        scope: &ProjectScope,
        feedback_id: Uuid,
    ) -> Result<i64>;

    /// Whether a given voter has already voted for a feedback row.
    async fn has_voted(
        &self,
        scope: &ProjectScope,
        feedback_id: Uuid,
        voter_id: &str,
    ) -> Result<bool>;
}

#[derive(Clone)]
pub struct SqlxBoardVoteRepo {
    pool: PgPool,
}

impl SqlxBoardVoteRepo {
    /// Constructor — allowlisted as a structural mirror of
    /// `SqlxRoadmapVoteRepo::new`.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl BoardVoteRepo for SqlxBoardVoteRepo {
    async fn cast(
        &self,
        scope: &ProjectScope,
        feedback_id: Uuid,
        voter_id: &str,
        voter_mode: RoadmapVoterMode,
    ) -> Result<BoardVote> {
        // INSERT...SELECT validates the feedback belongs to this scope in one
        // round-trip (the FK alone only proves "some feedback with this id
        // exists" — we need the tenant/project scope check too). Mirrors
        // roadmap_votes::cast. The unique-violation on (feedback_id, voter_id)
        // maps to Conflict (inv #1).
        //
        // NOTE: this does NOT re-check moderation_status — the handler resolves
        // an approved-only id via resolve_approved_board_feedback_id BEFORE
        // calling cast (plan D2 structural gate).
        let row = match sqlx::query!(
            r#"
            INSERT INTO feedback_board_votes (tenant_id, project_id, feedback_id, voter_id, voter_mode)
            SELECT $1, $2, f.id, $4, $5
            FROM feedback AS f
            WHERE f.id = $3 AND f.tenant_id = $1 AND f.project_id = $2
            RETURNING id, tenant_id, project_id, feedback_id, voter_id, voter_mode, cast_at
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_id,
            voter_id,
            voter_mode.as_db_str(),
        )
        .fetch_optional(&self.pool)
        .await
        {
            Ok(Some(r)) => r,
            // No row -> INSERT...SELECT found no matching feedback in scope.
            Ok(None) => return Err(RepoError::NotFound),
            Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                return Err(RepoError::Conflict);
            }
            Err(e) => return Err(e.into()),
        };

        Ok(BoardVote {
            id: row.id,
            tenant_id: row.tenant_id,
            project_id: row.project_id,
            feedback_id: row.feedback_id,
            voter_id: row.voter_id,
            voter_mode: RoadmapVoterMode::from_db_str(&row.voter_mode),
            cast_at: row.cast_at,
        })
    }

    async fn retract(
        &self,
        scope: &ProjectScope,
        feedback_id: Uuid,
        voter_id: &str,
        window: Duration,
    ) -> Result<RetractOutcome> {
        // Two-step inside one txn so the window check is consistent: read the
        // cast_at FOR UPDATE, decide, then DELETE (or not). Mirrors
        // roadmap_votes::retract.
        let mut tx = self.pool.begin().await?;

        let row = sqlx::query!(
            r#"
            SELECT v.cast_at
            FROM feedback_board_votes AS v
            WHERE v.tenant_id = $1
              AND v.project_id = $2
              AND v.feedback_id = $3
              AND v.voter_id = $4
            FOR UPDATE
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_id,
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
        let window_chrono = chrono::Duration::from_std(window).unwrap_or(chrono::Duration::MAX);
        if elapsed > window_chrono {
            tx.rollback().await?;
            return Ok(RetractOutcome::WindowExpired { cast_at: row.cast_at });
        }

        sqlx::query!(
            r#"
            DELETE FROM feedback_board_votes
            WHERE tenant_id = $1
              AND project_id = $2
              AND feedback_id = $3
              AND voter_id = $4
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_id,
            voter_id,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(RetractOutcome::Removed { retracted_at: now })
    }

    async fn vote_count_for_feedback(
        &self,
        scope: &ProjectScope,
        feedback_id: Uuid,
    ) -> Result<i64> {
        let row = sqlx::query!(
            r#"
            SELECT count(*) AS "count!"
            FROM feedback_board_votes
            WHERE tenant_id = $1 AND project_id = $2 AND feedback_id = $3
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_id,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.count)
    }

    async fn has_voted(
        &self,
        scope: &ProjectScope,
        feedback_id: Uuid,
        voter_id: &str,
    ) -> Result<bool> {
        let row = sqlx::query!(
            r#"
            SELECT 1 AS "one!"
            FROM feedback_board_votes
            WHERE tenant_id = $1 AND project_id = $2 AND feedback_id = $3 AND voter_id = $4
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_id,
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
    use crate::feedback::{FeedbackRepo, SqlxFeedbackRepo};
    use crate::projects::{ProjectRepo, SqlxProjectRepo};
    use crate::tenants::{SqlxTenantRepo, TenantRepo};
    use feedbackmonk_core::FeedbackKind;

    async fn seed_project_scope(pool: &PgPool, email: &str) -> ProjectScope {
        let trepo = SqlxTenantRepo::new(pool.clone());
        let prepo = SqlxProjectRepo::new(pool.clone());
        let t = trepo.create(email, "h").await.unwrap();
        let scope = trepo.scope_for(t.id).await.unwrap();
        let p = prepo.create(&scope, "Proj", "proj").await.unwrap();
        prepo.open(&scope, p.id).await.unwrap()
    }

    /// Seed an APPROVED feedback row and return its internal id (what the board
    /// vote handler resolves to before casting).
    async fn seed_approved_feedback(pool: &PgPool, scope: &ProjectScope, body: &str) -> Uuid {
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let fb = repo
            .submit_anonymous(scope, &[7u8; 32], None, body, None, FeedbackKind::Other)
            .await
            .unwrap();
        let mut tx = pool.begin().await.unwrap();
        repo.moderate_in_executor(
            scope,
            &mut tx,
            &fb,
            feedbackmonk_core::ModerationStatus::Approved,
            None,
            scope.tenant_id(),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        repo.resolve_approved_board_feedback_id(scope, &fb)
            .await
            .unwrap()
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn cast_round_trips(pool: PgPool) {
        let repo = SqlxBoardVoteRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "brt@example.com").await;
        let fb = seed_approved_feedback(&pool, &scope, "dark mode please").await;

        let vote = repo
            .cast(&scope, fb, "voter-1", RoadmapVoterMode::Jwt)
            .await
            .unwrap();
        assert_eq!(vote.voter_id, "voter-1");
        assert_eq!(vote.voter_mode, RoadmapVoterMode::Jwt);
        assert_eq!(vote.feedback_id, fb);

        let count = repo.vote_count_for_feedback(&scope, fb).await.unwrap();
        assert_eq!(count, 1);
        assert!(repo.has_voted(&scope, fb, "voter-1").await.unwrap());
        assert!(!repo.has_voted(&scope, fb, "voter-2").await.unwrap());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn cast_duplicate_returns_conflict(pool: PgPool) {
        let repo = SqlxBoardVoteRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "bdup@example.com").await;
        let fb = seed_approved_feedback(&pool, &scope, "feature x").await;

        repo.cast(&scope, fb, "v1", RoadmapVoterMode::Anon)
            .await
            .unwrap();
        let err = repo
            .cast(&scope, fb, "v1", RoadmapVoterMode::Anon)
            .await
            .unwrap_err();
        assert!(matches!(err, RepoError::Conflict));

        // Count stayed at 1 — no silent upsert.
        assert_eq!(repo.vote_count_for_feedback(&scope, fb).await.unwrap(), 1);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn cast_cross_tenant_feedback_returns_not_found(pool: PgPool) {
        let repo = SqlxBoardVoteRepo::new(pool.clone());
        let s1 = seed_project_scope(&pool, "bowner1@example.com").await;
        let s2 = seed_project_scope(&pool, "bowner2@example.com").await;
        let fb_s1 = seed_approved_feedback(&pool, &s1, "s1 feedback").await;

        // s2 trying to vote on s1's feedback must NotFound.
        let err = repo
            .cast(&s2, fb_s1, "v1", RoadmapVoterMode::Jwt)
            .await
            .unwrap_err();
        assert!(matches!(err, RepoError::NotFound));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn retract_inside_window_removes_vote(pool: PgPool) {
        let repo = SqlxBoardVoteRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "bret@example.com").await;
        let fb = seed_approved_feedback(&pool, &scope, "retract me").await;

        repo.cast(&scope, fb, "v1", RoadmapVoterMode::Anon)
            .await
            .unwrap();

        let outcome = repo
            .retract(&scope, fb, "v1", crate::DEFAULT_RETRACTION_WINDOW)
            .await
            .unwrap();
        assert!(matches!(outcome, RetractOutcome::Removed { .. }));
        assert_eq!(repo.vote_count_for_feedback(&scope, fb).await.unwrap(), 0);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn retract_with_no_prior_vote_returns_not_found(pool: PgPool) {
        let repo = SqlxBoardVoteRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "bret-none@example.com").await;
        let fb = seed_approved_feedback(&pool, &scope, "never voted").await;

        let outcome = repo
            .retract(&scope, fb, "never-voted", crate::DEFAULT_RETRACTION_WINDOW)
            .await
            .unwrap();
        assert!(matches!(outcome, RetractOutcome::NotFound));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn retract_outside_window_keeps_vote(pool: PgPool) {
        let repo = SqlxBoardVoteRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "bret-late@example.com").await;
        let fb = seed_approved_feedback(&pool, &scope, "too late").await;

        repo.cast(&scope, fb, "v1", RoadmapVoterMode::Anon)
            .await
            .unwrap();

        // Window of 0s means any retract after cast is outside the window.
        let outcome = repo
            .retract(&scope, fb, "v1", Duration::from_secs(0))
            .await
            .unwrap();
        assert!(
            matches!(outcome, RetractOutcome::WindowExpired { .. }),
            "got {outcome:?}"
        );
        assert_eq!(repo.vote_count_for_feedback(&scope, fb).await.unwrap(), 1);
    }
}
