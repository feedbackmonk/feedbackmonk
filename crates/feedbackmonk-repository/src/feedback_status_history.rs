//! Feedback status-history repository (Contract C6 backing methods).
//!
//! Owns CRUD on `feedback_status_history` (migration 00003). Every method
//! takes `&ProjectScope` first — the SQL filters by both `tenant_id` and
//! `project_id` on the joined `feedback` row, so a `transitioned_by` UUID
//! belonging to a sibling tenant cannot read or write through this surface.
//!
//! Stage 1 ships read + insert. Stage 2 Worker A's transition handler calls
//! `append` inside the same DB transaction as the `feedback.status` column
//! UPDATE (Contract C6 Hard Invariant #4). The append signature takes a
//! `&mut PgConnection` via the existing `&self.pool` extension is
//! intentionally NOT provided here — Worker A composes the same-transaction
//! pair via an `Executor`-aware variant in Stage 2 (pre-authorized
//! widening per PODS Coordination Protocol §Pre-authorized widenings:
//! "additional optional method overloads, schema-backwards-compatible").

use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use feedbackmonk_core::{FeedbackId, FeedbackStatus};

use crate::error::Result;
use crate::feedback::StatusHistoryRow;
use crate::scope::ProjectScope;

#[async_trait]
pub trait FeedbackStatusHistoryRepo: Send + Sync {
    /// Append one audit row. Stage 1 ships the non-transactional shape;
    /// Stage 2 Worker A composes a same-transaction variant alongside the
    /// feedback.status UPDATE per Contract C6 Hard Invariant #4.
    ///
    /// Returns the inserted row's `id` (UUID PK).
    async fn append(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
        from_status: FeedbackStatus,
        to_status: FeedbackStatus,
        reason_note: Option<&str>,
        duplicate_of: Option<&FeedbackId>,
        transitioned_by: Uuid,
    ) -> Result<Uuid>;

    /// Same-transaction variant of `append`. Required by Contract C6 Hard
    /// Invariant #4 (audit row + `feedback.status` UPDATE land atomically).
    /// The caller opens a transaction via `pool.begin()` and passes
    /// `&mut *tx` for `conn`.
    ///
    /// Pre-authorized widening per Stage 1->2 handoff doc:
    /// `self_mediated=true; ratification_pending=true;
    ///  matches_spec_at=docs/planning/handoffs/p1-stage1-to-stage2.md#pre-authorized`.
    async fn append_in_executor(
        &self,
        scope: &ProjectScope,
        conn: &mut sqlx::PgConnection,
        feedback_id: &FeedbackId,
        from_status: FeedbackStatus,
        to_status: FeedbackStatus,
        reason_note: Option<&str>,
        duplicate_of: Option<&FeedbackId>,
        transitioned_by: Uuid,
    ) -> Result<Uuid>;

    /// List the full status history for a feedback row, newest-first.
    /// Cross-tenant lookups return an empty Vec (NOT an error) — the JOIN
    /// against `feedback` filters by scope, so a sibling tenant's row
    /// simply has no rows to return.
    async fn list_for_feedback(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
    ) -> Result<Vec<StatusHistoryRow>>;
}

#[derive(Clone)]
pub struct SqlxFeedbackStatusHistoryRepo {
    pool: PgPool,
}

impl SqlxFeedbackStatusHistoryRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl FeedbackStatusHistoryRepo for SqlxFeedbackStatusHistoryRepo {
    async fn append(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
        from_status: FeedbackStatus,
        to_status: FeedbackStatus,
        reason_note: Option<&str>,
        duplicate_of: Option<&FeedbackId>,
        transitioned_by: Uuid,
    ) -> Result<Uuid> {
        // Resolve the source feedback row by short_code WITHIN scope so a
        // cross-tenant `feedback_id` can't trigger an audit-row insert under
        // a sibling tenant's data.
        let src = sqlx::query!(
            r#"
            SELECT id
            FROM feedback
            WHERE tenant_id = $1 AND project_id = $2 AND short_code = $3
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_id.as_str(),
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(crate::error::RepoError::NotFound)?;

        // Same for the optional duplicate_of target — must be within scope.
        let duplicate_of_uuid: Option<Uuid> = if let Some(target) = duplicate_of {
            let row = sqlx::query!(
                r#"
                SELECT id
                FROM feedback
                WHERE tenant_id = $1 AND project_id = $2 AND short_code = $3
                "#,
                scope.tenant_id(),
                scope.project_id(),
                target.as_str(),
            )
            .fetch_optional(&self.pool)
            .await?
            .ok_or(crate::error::RepoError::NotFound)?;
            Some(row.id)
        } else {
            None
        };

        let inserted = sqlx::query!(
            r#"
            INSERT INTO feedback_status_history (
                feedback_id, from_status, to_status,
                reason_note, duplicate_of_feedback_id, transitioned_by
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id
            "#,
            src.id,
            from_status.as_db_str(),
            to_status.as_db_str(),
            reason_note,
            duplicate_of_uuid,
            transitioned_by,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(inserted.id)
    }

    async fn append_in_executor(
        &self,
        scope: &ProjectScope,
        conn: &mut sqlx::PgConnection,
        feedback_id: &FeedbackId,
        from_status: FeedbackStatus,
        to_status: FeedbackStatus,
        reason_note: Option<&str>,
        duplicate_of: Option<&FeedbackId>,
        transitioned_by: Uuid,
    ) -> Result<Uuid> {
        let src = sqlx::query!(
            r#"
            SELECT id
            FROM feedback
            WHERE tenant_id = $1 AND project_id = $2 AND short_code = $3
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_id.as_str(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(crate::error::RepoError::NotFound)?;

        let duplicate_of_uuid: Option<Uuid> = if let Some(target) = duplicate_of {
            let row = sqlx::query!(
                r#"
                SELECT id
                FROM feedback
                WHERE tenant_id = $1 AND project_id = $2 AND short_code = $3
                "#,
                scope.tenant_id(),
                scope.project_id(),
                target.as_str(),
            )
            .fetch_optional(&mut *conn)
            .await?
            .ok_or(crate::error::RepoError::NotFound)?;
            Some(row.id)
        } else {
            None
        };

        let inserted = sqlx::query!(
            r#"
            INSERT INTO feedback_status_history (
                feedback_id, from_status, to_status,
                reason_note, duplicate_of_feedback_id, transitioned_by
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id
            "#,
            src.id,
            from_status.as_db_str(),
            to_status.as_db_str(),
            reason_note,
            duplicate_of_uuid,
            transitioned_by,
        )
        .fetch_one(&mut *conn)
        .await?;

        Ok(inserted.id)
    }

    async fn list_for_feedback(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
    ) -> Result<Vec<StatusHistoryRow>> {
        // JOIN through feedback so the scope filter applies. Cross-tenant
        // returns 0 rows, not an error (multi-tenant-isolation invariant).
        let rows = sqlx::query!(
            r#"
            SELECT h.id,
                   h.feedback_id,
                   h.from_status,
                   h.to_status,
                   h.reason_note,
                   h.duplicate_of_feedback_id,
                   h.transitioned_by,
                   h.transitioned_at
            FROM feedback_status_history AS h
            JOIN feedback AS f ON f.id = h.feedback_id
            WHERE f.tenant_id = $1
              AND f.project_id = $2
              AND f.short_code = $3
            ORDER BY h.transitioned_at DESC
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_id.as_str(),
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| StatusHistoryRow {
                id: r.id,
                feedback_id: r.feedback_id,
                from_status: FeedbackStatus::from_db_str(&r.from_status),
                to_status: FeedbackStatus::from_db_str(&r.to_status),
                reason_note: r.reason_note,
                duplicate_of_feedback_id: r.duplicate_of_feedback_id,
                transitioned_by: r.transitioned_by,
                transitioned_at: r.transitioned_at,
            })
            .collect())
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

    #[sqlx::test(migrations = "../../migrations")]
    async fn append_then_list_round_trip(pool: PgPool) {
        let feedback_repo = SqlxFeedbackRepo::new(pool.clone());
        let history_repo = SqlxFeedbackStatusHistoryRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "history-rt@example.com").await;

        let fb_id = feedback_repo
            .submit_anonymous(&scope, &[8u8; 32], None, "body", FeedbackKind::Other)
            .await
            .unwrap();

        let actor = Uuid::new_v4();
        history_repo
            .append(
                &scope,
                &fb_id,
                FeedbackStatus::Submitted,
                FeedbackStatus::Triaged,
                Some("looks legit"),
                None,
                actor,
            )
            .await
            .unwrap();

        let rows = history_repo.list_for_feedback(&scope, &fb_id).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].from_status, FeedbackStatus::Submitted);
        assert_eq!(rows[0].to_status, FeedbackStatus::Triaged);
        assert_eq!(rows[0].reason_note.as_deref(), Some("looks legit"));
        assert_eq!(rows[0].transitioned_by, actor);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn append_cross_tenant_target_rejected(pool: PgPool) {
        let feedback_repo = SqlxFeedbackRepo::new(pool.clone());
        let history_repo = SqlxFeedbackStatusHistoryRepo::new(pool.clone());
        let s1 = seed_project_scope(&pool, "owner1-h@example.com").await;
        let s2 = seed_project_scope(&pool, "owner2-h@example.com").await;
        let fb_id = feedback_repo
            .submit_anonymous(&s1, &[9u8; 32], None, "body", FeedbackKind::Other)
            .await
            .unwrap();

        // Append via s2's scope for a feedback row that belongs to s1 must
        // fail with NotFound (scope filter rejects the source lookup).
        let err = history_repo
            .append(
                &s2,
                &fb_id,
                FeedbackStatus::Submitted,
                FeedbackStatus::Triaged,
                None,
                None,
                Uuid::new_v4(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, crate::error::RepoError::NotFound));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn list_for_feedback_cross_tenant_returns_empty(pool: PgPool) {
        let feedback_repo = SqlxFeedbackRepo::new(pool.clone());
        let history_repo = SqlxFeedbackStatusHistoryRepo::new(pool.clone());
        let s1 = seed_project_scope(&pool, "owner1-list@example.com").await;
        let s2 = seed_project_scope(&pool, "owner2-list@example.com").await;
        let fb_id = feedback_repo
            .submit_anonymous(&s1, &[2u8; 32], None, "body", FeedbackKind::Other)
            .await
            .unwrap();
        history_repo
            .append(
                &s1,
                &fb_id,
                FeedbackStatus::Submitted,
                FeedbackStatus::Triaged,
                None,
                None,
                Uuid::new_v4(),
            )
            .await
            .unwrap();

        // s2's scope sees 0 rows for s1's feedback. NOT an error.
        let rows = history_repo.list_for_feedback(&s2, &fb_id).await.unwrap();
        assert!(rows.is_empty());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn duplicate_of_cross_tenant_rejected(pool: PgPool) {
        let feedback_repo = SqlxFeedbackRepo::new(pool.clone());
        let history_repo = SqlxFeedbackStatusHistoryRepo::new(pool.clone());
        let s1 = seed_project_scope(&pool, "dup-source@example.com").await;
        let s2 = seed_project_scope(&pool, "dup-target@example.com").await;

        let fb_a = feedback_repo
            .submit_anonymous(&s1, &[4u8; 32], None, "a", FeedbackKind::Other)
            .await
            .unwrap();
        let fb_b = feedback_repo
            .submit_anonymous(&s2, &[5u8; 32], None, "b in other tenant", FeedbackKind::Other)
            .await
            .unwrap();

        // s1 marks fb_a as duplicate of fb_b — but fb_b lives in s2.
        // duplicate_of resolution scoped to s1 returns NotFound.
        let err = history_repo
            .append(
                &s1,
                &fb_a,
                FeedbackStatus::Submitted,
                FeedbackStatus::Duplicate,
                None,
                Some(&fb_b),
                Uuid::new_v4(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, crate::error::RepoError::NotFound));
    }
}
