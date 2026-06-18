//! Analysis-sweep repository (Contract C23 backing methods).
//!
//! Owns CRUD on `analysis_sweeps` (migration 00013) — the provenance record
//! for each deep sweep (FR-FBR-20). Every method takes `&ProjectScope` first;
//! the SQL filters by `tenant_id` + `project_id` (DEC-FBR-03).
//!
//! Stage 0 ships create (a sweep opens in `running`) + get + list + a
//! `complete` updater. Worker B (Stage 1) drives the sweep orchestration +
//! digest generation + the analyst-ingestion API on top.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{RepoError, Result};
use crate::scope::ProjectScope;

/// Provenance for one deep sweep over the feedback corpus (FR-FBR-20).
/// `digest_summary` powers the "what changed since last time" review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisSweep {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub triggered_by: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub status: String,
    pub clusters_touched: i32,
    pub recommendations_emitted: i32,
    pub runner_id: Option<String>,
    pub agent_version: Option<String>,
    pub digest_summary: Option<String>,
}

/// Terminal outcome + tallies for [`AnalysisSweepRepo::complete`].
#[derive(Debug, Clone)]
pub struct SweepOutcome<'a> {
    /// Either `"completed"` or `"failed"`.
    pub status: &'a str,
    pub clusters_touched: i32,
    pub recommendations_emitted: i32,
    pub runner_id: Option<&'a str>,
    pub agent_version: Option<&'a str>,
    pub digest_summary: Option<&'a str>,
}

#[async_trait]
pub trait AnalysisSweepRepo: Send + Sync {
    /// Open a sweep in `running`. `triggered_by` is `'schedule'` or
    /// `'on_demand'`.
    async fn create(&self, scope: &ProjectScope, triggered_by: &str) -> Result<AnalysisSweep>;

    /// Fetch one sweep by id within scope.
    async fn get(&self, scope: &ProjectScope, sweep_id: Uuid) -> Result<AnalysisSweep>;

    /// List sweeps for the project, most-recent first.
    async fn list(&self, scope: &ProjectScope) -> Result<Vec<AnalysisSweep>>;

    /// Mark a running sweep terminal (`completed` / `failed`), stamping
    /// `completed_at = now()` and the tallies + digest. `NotFound` if the
    /// sweep is absent or out of scope.
    async fn complete(
        &self,
        scope: &ProjectScope,
        sweep_id: Uuid,
        outcome: SweepOutcome<'_>,
    ) -> Result<AnalysisSweep>;
}

#[derive(Clone)]
pub struct SqlxAnalysisSweepRepo {
    pool: PgPool,
}

impl SqlxAnalysisSweepRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn map_sweep(row: SweepRow) -> AnalysisSweep {
    AnalysisSweep {
        id: row.id,
        tenant_id: row.tenant_id,
        project_id: row.project_id,
        triggered_by: row.triggered_by,
        started_at: row.started_at,
        completed_at: row.completed_at,
        status: row.status,
        clusters_touched: row.clusters_touched,
        recommendations_emitted: row.recommendations_emitted,
        runner_id: row.runner_id,
        agent_version: row.agent_version,
        digest_summary: row.digest_summary,
    }
}

/// Internal row shape shared by every SELECT/RETURNING in this module.
struct SweepRow {
    id: Uuid,
    tenant_id: Uuid,
    project_id: Uuid,
    triggered_by: String,
    started_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
    status: String,
    clusters_touched: i32,
    recommendations_emitted: i32,
    runner_id: Option<String>,
    agent_version: Option<String>,
    digest_summary: Option<String>,
}

#[async_trait]
impl AnalysisSweepRepo for SqlxAnalysisSweepRepo {
    async fn create(&self, scope: &ProjectScope, triggered_by: &str) -> Result<AnalysisSweep> {
        let row = sqlx::query_as!(
            SweepRow,
            r#"
            INSERT INTO analysis_sweeps (tenant_id, project_id, triggered_by)
            VALUES ($1, $2, $3)
            RETURNING id, tenant_id, project_id, triggered_by, started_at,
                      completed_at, status, clusters_touched,
                      recommendations_emitted, runner_id, agent_version,
                      digest_summary
            "#,
            scope.tenant_id(),
            scope.project_id(),
            triggered_by,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(map_sweep(row))
    }

    async fn get(&self, scope: &ProjectScope, sweep_id: Uuid) -> Result<AnalysisSweep> {
        let row = sqlx::query_as!(
            SweepRow,
            r#"
            SELECT id, tenant_id, project_id, triggered_by, started_at,
                   completed_at, status, clusters_touched,
                   recommendations_emitted, runner_id, agent_version,
                   digest_summary
            FROM analysis_sweeps
            WHERE tenant_id = $1 AND project_id = $2 AND id = $3
            "#,
            scope.tenant_id(),
            scope.project_id(),
            sweep_id,
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(RepoError::NotFound)?;
        Ok(map_sweep(row))
    }

    async fn list(&self, scope: &ProjectScope) -> Result<Vec<AnalysisSweep>> {
        let rows = sqlx::query_as!(
            SweepRow,
            r#"
            SELECT id, tenant_id, project_id, triggered_by, started_at,
                   completed_at, status, clusters_touched,
                   recommendations_emitted, runner_id, agent_version,
                   digest_summary
            FROM analysis_sweeps
            WHERE tenant_id = $1 AND project_id = $2
            ORDER BY started_at DESC
            "#,
            scope.tenant_id(),
            scope.project_id(),
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(map_sweep).collect())
    }

    async fn complete(
        &self,
        scope: &ProjectScope,
        sweep_id: Uuid,
        outcome: SweepOutcome<'_>,
    ) -> Result<AnalysisSweep> {
        let row = sqlx::query_as!(
            SweepRow,
            r#"
            UPDATE analysis_sweeps
            SET status = $4,
                completed_at = now(),
                clusters_touched = $5,
                recommendations_emitted = $6,
                runner_id = $7,
                agent_version = $8,
                digest_summary = $9
            WHERE tenant_id = $1 AND project_id = $2 AND id = $3
            RETURNING id, tenant_id, project_id, triggered_by, started_at,
                      completed_at, status, clusters_touched,
                      recommendations_emitted, runner_id, agent_version,
                      digest_summary
            "#,
            scope.tenant_id(),
            scope.project_id(),
            sweep_id,
            outcome.status,
            outcome.clusters_touched,
            outcome.recommendations_emitted,
            outcome.runner_id,
            outcome.agent_version,
            outcome.digest_summary,
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(RepoError::NotFound)?;
        Ok(map_sweep(row))
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
    async fn create_then_complete_round_trip(pool: PgPool) {
        let repo = SqlxAnalysisSweepRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "sweep-rt@example.com").await;

        let s = repo.create(&scope, "on_demand").await.unwrap();
        assert_eq!(s.status, "running");
        assert!(s.completed_at.is_none());

        let done = repo
            .complete(
                &scope,
                s.id,
                SweepOutcome {
                    status: "completed",
                    clusters_touched: 4,
                    recommendations_emitted: 2,
                    runner_id: Some("runner-1"),
                    agent_version: Some("uldf-1.0"),
                    digest_summary: Some("2 new recommendations"),
                },
            )
            .await
            .unwrap();
        assert_eq!(done.status, "completed");
        assert!(done.completed_at.is_some());
        assert_eq!(done.clusters_touched, 4);
        assert_eq!(done.recommendations_emitted, 2);
        assert_eq!(done.digest_summary.as_deref(), Some("2 new recommendations"));

        let list = repo.list(&scope).await.unwrap();
        assert_eq!(list.len(), 1);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn cross_tenant_get_returns_not_found(pool: PgPool) {
        let repo = SqlxAnalysisSweepRepo::new(pool.clone());
        let s1 = seed_project_scope(&pool, "owner1-sw@example.com").await;
        let s2 = seed_project_scope(&pool, "owner2-sw@example.com").await;
        let sweep = repo.create(&s1, "schedule").await.unwrap();

        let err = repo.get(&s2, sweep.id).await.unwrap_err();
        assert!(matches!(err, RepoError::NotFound));
        assert!(repo.list(&s2).await.unwrap().is_empty());
    }
}
