//! Feedback-cluster repository (Contract C23 backing methods).
//!
//! Owns CRUD on `feedback_clusters` (migration 00013). Every method takes
//! `&ProjectScope` first — the SQL filters by both `tenant_id` and
//! `project_id`, so a sibling tenant can neither read nor write through this
//! surface (DEC-FBR-03 multi-tenant isolation).
//!
//! Stage 0 ships the foundation: create + get + list + member-count
//! maintenance. Worker B (Stage 1) builds the FR-FBR-19 clustering-on-submit
//! assignment, merge/split, and priority/status updates on top of this trait.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use feedbackmonk_core::FeedbackKind;

use crate::error::{RepoError, Result};
use crate::scope::ProjectScope;

/// A canonical grouping of near-duplicate feedback (FR-FBR-19).
///
/// `priority` / `status` / `created_by` are held as `String` (mirroring
/// `Tenant.tier`); the DB CHECK constraints pin the legal value sets. `kind`
/// is the typed `FeedbackKind`. `merged_into_id` is the survivor pointer for
/// merged clusters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedbackCluster {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub label: String,
    pub summary: Option<String>,
    pub kind: FeedbackKind,
    pub priority: String,
    pub priority_rationale: Option<String>,
    pub status: String,
    pub merged_into_id: Option<Uuid>,
    pub member_count: i32,
    pub last_swept_at: Option<DateTime<Utc>>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[async_trait]
pub trait ClusterRepo: Send + Sync {
    /// Open a new cluster. `created_by` is `'agent'` (deterministic
    /// clustering / sweep) or `'admin'` (manual). New clusters start
    /// `priority='none'`, `status='open'`, `member_count=0`.
    async fn create(
        &self,
        scope: &ProjectScope,
        label: &str,
        summary: Option<&str>,
        kind: FeedbackKind,
        created_by: &str,
    ) -> Result<FeedbackCluster>;

    /// Fetch one cluster by id within scope. `NotFound` if absent or owned by
    /// a sibling tenant.
    async fn get(&self, scope: &ProjectScope, cluster_id: Uuid) -> Result<FeedbackCluster>;

    /// List clusters for the project, newest-first. Cross-tenant scopes see an
    /// empty Vec (not an error).
    async fn list(&self, scope: &ProjectScope) -> Result<Vec<FeedbackCluster>>;

    /// Adjust `member_count` by `delta` (e.g. +1 on assign, -1 on un-assign)
    /// and bump `updated_at`. Clamped at zero by the DB CHECK; the caller is
    /// responsible for not driving it negative. Returns the new count.
    async fn adjust_member_count(
        &self,
        scope: &ProjectScope,
        cluster_id: Uuid,
        delta: i32,
    ) -> Result<i32>;

    // ==== Stage 1 (CLAUDE-B) — additive executor variants ====================
    // New methods only — the four Stage-0 signatures above are FROZEN. These
    // let Worker B's clustering-on-submit + merge/split do their multi-step
    // work atomically inside one caller-supplied transaction (announced in
    // collab messages.md MSG-003 Q2). No new constructor → no
    // `multi-tenant-isolation-check` allowlist entry needed.

    /// Same-transaction variant of [`create`]. Opens a new cluster on a
    /// caller-supplied connection so clustering-on-submit can create the
    /// cluster, set `feedback.cluster_id`, and bump `member_count` atomically.
    async fn create_in_executor(
        &self,
        scope: &ProjectScope,
        conn: &mut sqlx::PgConnection,
        label: &str,
        summary: Option<&str>,
        kind: FeedbackKind,
        created_by: &str,
    ) -> Result<FeedbackCluster>;

    /// Same-transaction variant of [`adjust_member_count`].
    async fn adjust_member_count_in_executor(
        &self,
        scope: &ProjectScope,
        conn: &mut sqlx::PgConnection,
        cluster_id: Uuid,
        delta: i32,
    ) -> Result<i32>;

    /// Mark `cluster_id` merged into `survivor_id` within the caller's txn:
    /// `status='merged'`, `merged_into_id=survivor_id`, `member_count=0`. The
    /// survivor MUST belong to `scope` (resolved within scope first; `NotFound`
    /// otherwise) and differ from the merged cluster (the DB
    /// `feedback_clusters_no_self_merge` CHECK is the backstop). Returns the
    /// updated (merged) row. The caller is responsible for re-pointing members
    /// (`FeedbackRepo::repoint_cluster_members_in_executor`) and bumping the
    /// survivor's count in the same txn.
    async fn mark_merged_in_executor(
        &self,
        scope: &ProjectScope,
        conn: &mut sqlx::PgConnection,
        cluster_id: Uuid,
        survivor_id: Uuid,
    ) -> Result<FeedbackCluster>;
}

#[derive(Clone)]
pub struct SqlxClusterRepo {
    pool: PgPool,
}

impl SqlxClusterRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ClusterRepo for SqlxClusterRepo {
    async fn create(
        &self,
        scope: &ProjectScope,
        label: &str,
        summary: Option<&str>,
        kind: FeedbackKind,
        created_by: &str,
    ) -> Result<FeedbackCluster> {
        let row = sqlx::query!(
            r#"
            INSERT INTO feedback_clusters
                (tenant_id, project_id, label, summary, kind, created_by)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id, tenant_id, project_id, label, summary, kind,
                      priority, priority_rationale, status, merged_into_id,
                      member_count, last_swept_at, created_by, created_at, updated_at
            "#,
            scope.tenant_id(),
            scope.project_id(),
            label,
            summary,
            kind.as_str(),
            created_by,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(FeedbackCluster {
            id: row.id,
            tenant_id: row.tenant_id,
            project_id: row.project_id,
            label: row.label,
            summary: row.summary,
            kind: FeedbackKind::from_db_str(&row.kind),
            priority: row.priority,
            priority_rationale: row.priority_rationale,
            status: row.status,
            merged_into_id: row.merged_into_id,
            member_count: row.member_count,
            last_swept_at: row.last_swept_at,
            created_by: row.created_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }

    async fn get(&self, scope: &ProjectScope, cluster_id: Uuid) -> Result<FeedbackCluster> {
        let row = sqlx::query!(
            r#"
            SELECT id, tenant_id, project_id, label, summary, kind,
                   priority, priority_rationale, status, merged_into_id,
                   member_count, last_swept_at, created_by, created_at, updated_at
            FROM feedback_clusters
            WHERE tenant_id = $1 AND project_id = $2 AND id = $3
            "#,
            scope.tenant_id(),
            scope.project_id(),
            cluster_id,
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(RepoError::NotFound)?;

        Ok(FeedbackCluster {
            id: row.id,
            tenant_id: row.tenant_id,
            project_id: row.project_id,
            label: row.label,
            summary: row.summary,
            kind: FeedbackKind::from_db_str(&row.kind),
            priority: row.priority,
            priority_rationale: row.priority_rationale,
            status: row.status,
            merged_into_id: row.merged_into_id,
            member_count: row.member_count,
            last_swept_at: row.last_swept_at,
            created_by: row.created_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }

    async fn list(&self, scope: &ProjectScope) -> Result<Vec<FeedbackCluster>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, tenant_id, project_id, label, summary, kind,
                   priority, priority_rationale, status, merged_into_id,
                   member_count, last_swept_at, created_by, created_at, updated_at
            FROM feedback_clusters
            WHERE tenant_id = $1 AND project_id = $2
            ORDER BY created_at DESC
            "#,
            scope.tenant_id(),
            scope.project_id(),
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| FeedbackCluster {
                id: row.id,
                tenant_id: row.tenant_id,
                project_id: row.project_id,
                label: row.label,
                summary: row.summary,
                kind: FeedbackKind::from_db_str(&row.kind),
                priority: row.priority,
                priority_rationale: row.priority_rationale,
                status: row.status,
                merged_into_id: row.merged_into_id,
                member_count: row.member_count,
                last_swept_at: row.last_swept_at,
                created_by: row.created_by,
                created_at: row.created_at,
                updated_at: row.updated_at,
            })
            .collect())
    }

    async fn adjust_member_count(
        &self,
        scope: &ProjectScope,
        cluster_id: Uuid,
        delta: i32,
    ) -> Result<i32> {
        let row = sqlx::query!(
            r#"
            UPDATE feedback_clusters
            SET member_count = GREATEST(member_count + $4, 0),
                updated_at = now()
            WHERE tenant_id = $1 AND project_id = $2 AND id = $3
            RETURNING member_count
            "#,
            scope.tenant_id(),
            scope.project_id(),
            cluster_id,
            delta,
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(RepoError::NotFound)?;

        Ok(row.member_count)
    }

    // ==== Stage 1 (CLAUDE-B) — additive executor variants ====================

    async fn create_in_executor(
        &self,
        scope: &ProjectScope,
        conn: &mut sqlx::PgConnection,
        label: &str,
        summary: Option<&str>,
        kind: FeedbackKind,
        created_by: &str,
    ) -> Result<FeedbackCluster> {
        let row = sqlx::query!(
            r#"
            INSERT INTO feedback_clusters
                (tenant_id, project_id, label, summary, kind, created_by)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id, tenant_id, project_id, label, summary, kind,
                      priority, priority_rationale, status, merged_into_id,
                      member_count, last_swept_at, created_by, created_at, updated_at
            "#,
            scope.tenant_id(),
            scope.project_id(),
            label,
            summary,
            kind.as_str(),
            created_by,
        )
        .fetch_one(&mut *conn)
        .await?;

        Ok(FeedbackCluster {
            id: row.id,
            tenant_id: row.tenant_id,
            project_id: row.project_id,
            label: row.label,
            summary: row.summary,
            kind: FeedbackKind::from_db_str(&row.kind),
            priority: row.priority,
            priority_rationale: row.priority_rationale,
            status: row.status,
            merged_into_id: row.merged_into_id,
            member_count: row.member_count,
            last_swept_at: row.last_swept_at,
            created_by: row.created_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }

    async fn adjust_member_count_in_executor(
        &self,
        scope: &ProjectScope,
        conn: &mut sqlx::PgConnection,
        cluster_id: Uuid,
        delta: i32,
    ) -> Result<i32> {
        let row = sqlx::query!(
            r#"
            UPDATE feedback_clusters
            SET member_count = GREATEST(member_count + $4, 0),
                updated_at = now()
            WHERE tenant_id = $1 AND project_id = $2 AND id = $3
            RETURNING member_count
            "#,
            scope.tenant_id(),
            scope.project_id(),
            cluster_id,
            delta,
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(RepoError::NotFound)?;

        Ok(row.member_count)
    }

    async fn mark_merged_in_executor(
        &self,
        scope: &ProjectScope,
        conn: &mut sqlx::PgConnection,
        cluster_id: Uuid,
        survivor_id: Uuid,
    ) -> Result<FeedbackCluster> {
        // Resolve the survivor WITHIN scope first so a cross-tenant survivor_id
        // cannot be stamped onto this tenant's merged cluster (the FK alone
        // would permit any existing cluster id).
        sqlx::query!(
            r#"
            SELECT id FROM feedback_clusters
            WHERE tenant_id = $1 AND project_id = $2 AND id = $3
            "#,
            scope.tenant_id(),
            scope.project_id(),
            survivor_id,
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(RepoError::NotFound)?;

        let row = sqlx::query!(
            r#"
            UPDATE feedback_clusters
            SET status = 'merged',
                merged_into_id = $4,
                member_count = 0,
                updated_at = now()
            WHERE tenant_id = $1 AND project_id = $2 AND id = $3
            RETURNING id, tenant_id, project_id, label, summary, kind,
                      priority, priority_rationale, status, merged_into_id,
                      member_count, last_swept_at, created_by, created_at, updated_at
            "#,
            scope.tenant_id(),
            scope.project_id(),
            cluster_id,
            survivor_id,
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(RepoError::NotFound)?;

        Ok(FeedbackCluster {
            id: row.id,
            tenant_id: row.tenant_id,
            project_id: row.project_id,
            label: row.label,
            summary: row.summary,
            kind: FeedbackKind::from_db_str(&row.kind),
            priority: row.priority,
            priority_rationale: row.priority_rationale,
            status: row.status,
            merged_into_id: row.merged_into_id,
            member_count: row.member_count,
            last_swept_at: row.last_swept_at,
            created_by: row.created_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::{ProjectRepo, SqlxProjectRepo};
    use crate::tenants::{SqlxTenantRepo, TenantRepo};

    async fn seed_project_scope(pool: &PgPool, email: &str) -> ProjectScope {
        let trepo = SqlxTenantRepo::new(pool.clone());
        let prepo = SqlxProjectRepo::new(pool.clone());
        let t = trepo.create(email, "h").await.unwrap();
        let scope = trepo.scope_for(t.id).await.unwrap();
        let p = prepo.create(&scope, "Proj", "proj").await.unwrap();
        prepo.open(&scope, p.id).await.unwrap()
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn create_get_list_round_trip(pool: PgPool) {
        let repo = SqlxClusterRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "cluster-rt@example.com").await;

        let c = repo
            .create(&scope, "Login is broken", Some("many reports"), FeedbackKind::Bug, "agent")
            .await
            .unwrap();
        assert_eq!(c.label, "Login is broken");
        assert_eq!(c.kind, FeedbackKind::Bug);
        assert_eq!(c.priority, "none");
        assert_eq!(c.status, "open");
        assert_eq!(c.member_count, 0);

        let got = repo.get(&scope, c.id).await.unwrap();
        assert_eq!(got, c);

        let list = repo.list(&scope).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, c.id);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn adjust_member_count_clamps_at_zero(pool: PgPool) {
        let repo = SqlxClusterRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "cluster-count@example.com").await;
        let c = repo
            .create(&scope, "Dark mode", None, FeedbackKind::Feature, "agent")
            .await
            .unwrap();

        assert_eq!(repo.adjust_member_count(&scope, c.id, 3).await.unwrap(), 3);
        assert_eq!(repo.adjust_member_count(&scope, c.id, -1).await.unwrap(), 2);
        // Clamped at zero, never negative.
        assert_eq!(repo.adjust_member_count(&scope, c.id, -10).await.unwrap(), 0);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn cross_tenant_get_returns_not_found(pool: PgPool) {
        let repo = SqlxClusterRepo::new(pool.clone());
        let s1 = seed_project_scope(&pool, "owner1-cl@example.com").await;
        let s2 = seed_project_scope(&pool, "owner2-cl@example.com").await;
        let c = repo
            .create(&s1, "S1 cluster", None, FeedbackKind::Other, "agent")
            .await
            .unwrap();

        // s2 cannot read s1's cluster.
        let err = repo.get(&s2, c.id).await.unwrap_err();
        assert!(matches!(err, RepoError::NotFound));
        // s2's list is empty; s1's is not.
        assert!(repo.list(&s2).await.unwrap().is_empty());
        assert_eq!(repo.list(&s1).await.unwrap().len(), 1);
    }

    // ==== Stage 1 (CLAUDE-B) — additive executor variant tests ===============

    #[sqlx::test(migrations = "../../migrations")]
    async fn create_and_adjust_in_executor_round_trip(pool: PgPool) {
        let repo = SqlxClusterRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "cluster-exec@example.com").await;

        let mut tx = pool.begin().await.unwrap();
        let c = repo
            .create_in_executor(&scope, &mut tx, "Login broken", None, FeedbackKind::Bug, "agent")
            .await
            .unwrap();
        assert_eq!(c.member_count, 0);
        let n = repo
            .adjust_member_count_in_executor(&scope, &mut tx, c.id, 3)
            .await
            .unwrap();
        assert_eq!(n, 3);
        tx.commit().await.unwrap();

        // Committed: visible through the pool-based reads.
        let got = repo.get(&scope, c.id).await.unwrap();
        assert_eq!(got.member_count, 3);
        assert_eq!(got.created_by, "agent");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn create_in_executor_rolls_back_with_the_txn(pool: PgPool) {
        let repo = SqlxClusterRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "cluster-rollback@example.com").await;

        let mut tx = pool.begin().await.unwrap();
        repo.create_in_executor(&scope, &mut tx, "Ephemeral", None, FeedbackKind::Other, "agent")
            .await
            .unwrap();
        tx.rollback().await.unwrap();

        // Nothing persisted — the executor variant honors the caller's txn.
        assert!(repo.list(&scope).await.unwrap().is_empty());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn mark_merged_sets_survivor_and_zeroes_count(pool: PgPool) {
        let repo = SqlxClusterRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "cluster-merge@example.com").await;
        let survivor = repo
            .create(&scope, "Survivor", None, FeedbackKind::Bug, "agent")
            .await
            .unwrap();
        let merged = repo
            .create(&scope, "Merged", None, FeedbackKind::Bug, "agent")
            .await
            .unwrap();
        repo.adjust_member_count(&scope, merged.id, 5).await.unwrap();

        let mut tx = pool.begin().await.unwrap();
        let after = repo
            .mark_merged_in_executor(&scope, &mut tx, merged.id, survivor.id)
            .await
            .unwrap();
        tx.commit().await.unwrap();

        assert_eq!(after.status, "merged");
        assert_eq!(after.merged_into_id, Some(survivor.id));
        assert_eq!(after.member_count, 0);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn mark_merged_rejects_cross_tenant_survivor(pool: PgPool) {
        let repo = SqlxClusterRepo::new(pool.clone());
        let s1 = seed_project_scope(&pool, "merge-owner1@example.com").await;
        let s2 = seed_project_scope(&pool, "merge-owner2@example.com").await;
        let merged = repo
            .create(&s1, "S1 merged", None, FeedbackKind::Bug, "agent")
            .await
            .unwrap();
        let foreign_survivor = repo
            .create(&s2, "S2 survivor", None, FeedbackKind::Bug, "agent")
            .await
            .unwrap();

        // s1 cannot point its cluster at s2's survivor.
        let mut tx = pool.begin().await.unwrap();
        let err = repo
            .mark_merged_in_executor(&s1, &mut tx, merged.id, foreign_survivor.id)
            .await
            .unwrap_err();
        assert!(matches!(err, RepoError::NotFound));
        tx.rollback().await.ok();
    }
}
