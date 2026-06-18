//! Recommendation repository (Contract C23 backing methods).
//!
//! Owns CRUD on `recommendations` (migration 00013) — the analyst's proposed
//! action for an actionable cluster (FR-FBR-20). Every method takes
//! `&ProjectScope` first (DEC-FBR-03).
//!
//! `source_refs` is a JSONB list of file/line/doc REFERENCES the analyst
//! inspected — grounding evidence, NEVER a dump of file contents (exfiltration
//! defense, C24 case f). The repository stores it as opaque `serde_json::Value`;
//! the ingestion API (Worker B) validates the shape.
//!
//! Stage 0 ships create + get + `list_for_cluster` + a status setter. Worker B
//! drives recommendation generation/ingestion; Worker A reads a recommendation
//! (via `get`) when creating a work order from it.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use feedbackmonk_core::ActionType;

use crate::error::{RepoError, Result};
use crate::scope::ProjectScope;

/// The analyst's recommended action for a cluster. 1:N from a cluster
/// (history); superseded recommendations are retained with `status='superseded'`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recommendation {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub cluster_id: Uuid,
    pub sweep_id: Option<Uuid>,
    pub action_type: ActionType,
    pub title: String,
    pub body: String,
    pub rationale: Option<String>,
    /// JSONB list of grounding references (file/line/doc). Never file dumps.
    pub source_refs: JsonValue,
    pub confidence: f64,
    pub status: String,
    pub generated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

/// Fields for [`RecommendationRepo::create`]. Grouped to keep the call site
/// readable (clippy `too_many_arguments`).
#[derive(Debug, Clone)]
pub struct NewRecommendation<'a> {
    pub cluster_id: Uuid,
    pub sweep_id: Option<Uuid>,
    pub action_type: ActionType,
    pub title: &'a str,
    pub body: &'a str,
    pub rationale: Option<&'a str>,
    pub source_refs: &'a JsonValue,
    pub confidence: f64,
}

#[async_trait]
pub trait RecommendationRepo: Send + Sync {
    /// Insert a `proposed` recommendation for a cluster. The `cluster_id` (and
    /// `sweep_id`, if present) MUST belong to `scope` — resolved within scope
    /// first so a cross-tenant id cannot anchor a recommendation under a
    /// sibling tenant's data (`NotFound` otherwise).
    async fn create(
        &self,
        scope: &ProjectScope,
        rec: NewRecommendation<'_>,
    ) -> Result<Recommendation>;

    /// Fetch one recommendation by id within scope.
    async fn get(&self, scope: &ProjectScope, recommendation_id: Uuid) -> Result<Recommendation>;

    /// List a cluster's recommendations, newest-first. Cross-tenant scopes see
    /// an empty Vec.
    async fn list_for_cluster(
        &self,
        scope: &ProjectScope,
        cluster_id: Uuid,
    ) -> Result<Vec<Recommendation>>;

    /// Set a recommendation's `status` (e.g. `approved`, `tweaked_approved`,
    /// `rejected`, `superseded`). Returns the updated row.
    async fn set_status(
        &self,
        scope: &ProjectScope,
        recommendation_id: Uuid,
        status: &str,
    ) -> Result<Recommendation>;
}

#[derive(Clone)]
pub struct SqlxRecommendationRepo {
    pool: PgPool,
}

impl SqlxRecommendationRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Internal row -> domain mapper (manual, to decode the `action_type` enum).
#[allow(clippy::similar_names)]
fn map_recommendation(
    id: Uuid,
    tenant_id: Uuid,
    project_id: Uuid,
    cluster_id: Uuid,
    sweep_id: Option<Uuid>,
    action_type: &str,
    title: String,
    body: String,
    rationale: Option<String>,
    source_refs: JsonValue,
    confidence: f64,
    status: String,
    generated_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
) -> Recommendation {
    Recommendation {
        id,
        tenant_id,
        project_id,
        cluster_id,
        sweep_id,
        action_type: ActionType::from_db_str(action_type),
        title,
        body,
        rationale,
        source_refs,
        confidence,
        status,
        generated_at,
        created_at,
    }
}

#[async_trait]
impl RecommendationRepo for SqlxRecommendationRepo {
    async fn create(
        &self,
        scope: &ProjectScope,
        rec: NewRecommendation<'_>,
    ) -> Result<Recommendation> {
        // Resolve the cluster WITHIN scope so a cross-tenant cluster_id cannot
        // anchor a recommendation under a sibling tenant (mirrors the
        // feedback_status_history within-scope guard).
        let cluster = sqlx::query!(
            r#"
            SELECT id FROM feedback_clusters
            WHERE tenant_id = $1 AND project_id = $2 AND id = $3
            "#,
            scope.tenant_id(),
            scope.project_id(),
            rec.cluster_id,
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(RepoError::NotFound)?;

        // Likewise the optional sweep_id must be within scope.
        if let Some(sweep_id) = rec.sweep_id {
            sqlx::query!(
                r#"
                SELECT id FROM analysis_sweeps
                WHERE tenant_id = $1 AND project_id = $2 AND id = $3
                "#,
                scope.tenant_id(),
                scope.project_id(),
                sweep_id,
            )
            .fetch_optional(&self.pool)
            .await?
            .ok_or(RepoError::NotFound)?;
        }

        let row = sqlx::query!(
            r#"
            INSERT INTO recommendations
                (tenant_id, project_id, cluster_id, sweep_id, action_type,
                 title, body, rationale, source_refs, confidence)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            RETURNING id, tenant_id, project_id, cluster_id, sweep_id,
                      action_type, title, body, rationale, source_refs,
                      confidence, status, generated_at, created_at
            "#,
            scope.tenant_id(),
            scope.project_id(),
            cluster.id,
            rec.sweep_id,
            rec.action_type.as_db_str(),
            rec.title,
            rec.body,
            rec.rationale,
            rec.source_refs,
            rec.confidence,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(map_recommendation(
            row.id, row.tenant_id, row.project_id, row.cluster_id, row.sweep_id,
            &row.action_type, row.title, row.body, row.rationale, row.source_refs,
            row.confidence, row.status, row.generated_at, row.created_at,
        ))
    }

    async fn get(&self, scope: &ProjectScope, recommendation_id: Uuid) -> Result<Recommendation> {
        let row = sqlx::query!(
            r#"
            SELECT id, tenant_id, project_id, cluster_id, sweep_id,
                   action_type, title, body, rationale, source_refs,
                   confidence, status, generated_at, created_at
            FROM recommendations
            WHERE tenant_id = $1 AND project_id = $2 AND id = $3
            "#,
            scope.tenant_id(),
            scope.project_id(),
            recommendation_id,
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(RepoError::NotFound)?;

        Ok(map_recommendation(
            row.id, row.tenant_id, row.project_id, row.cluster_id, row.sweep_id,
            &row.action_type, row.title, row.body, row.rationale, row.source_refs,
            row.confidence, row.status, row.generated_at, row.created_at,
        ))
    }

    async fn list_for_cluster(
        &self,
        scope: &ProjectScope,
        cluster_id: Uuid,
    ) -> Result<Vec<Recommendation>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, tenant_id, project_id, cluster_id, sweep_id,
                   action_type, title, body, rationale, source_refs,
                   confidence, status, generated_at, created_at
            FROM recommendations
            WHERE tenant_id = $1 AND project_id = $2 AND cluster_id = $3
            ORDER BY generated_at DESC
            "#,
            scope.tenant_id(),
            scope.project_id(),
            cluster_id,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| {
                map_recommendation(
                    row.id, row.tenant_id, row.project_id, row.cluster_id, row.sweep_id,
                    &row.action_type, row.title, row.body, row.rationale, row.source_refs,
                    row.confidence, row.status, row.generated_at, row.created_at,
                )
            })
            .collect())
    }

    async fn set_status(
        &self,
        scope: &ProjectScope,
        recommendation_id: Uuid,
        status: &str,
    ) -> Result<Recommendation> {
        let row = sqlx::query!(
            r#"
            UPDATE recommendations
            SET status = $4
            WHERE tenant_id = $1 AND project_id = $2 AND id = $3
            RETURNING id, tenant_id, project_id, cluster_id, sweep_id,
                      action_type, title, body, rationale, source_refs,
                      confidence, status, generated_at, created_at
            "#,
            scope.tenant_id(),
            scope.project_id(),
            recommendation_id,
            status,
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(RepoError::NotFound)?;

        Ok(map_recommendation(
            row.id, row.tenant_id, row.project_id, row.cluster_id, row.sweep_id,
            &row.action_type, row.title, row.body, row.rationale, row.source_refs,
            row.confidence, row.status, row.generated_at, row.created_at,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clusters::{ClusterRepo, SqlxClusterRepo};
    use crate::projects::{ProjectRepo, SqlxProjectRepo};
    use crate::tenants::{SqlxTenantRepo, TenantRepo};
    use feedbackmonk_core::FeedbackKind;
    use serde_json::json;

    async fn seed_project_scope(pool: &PgPool, email: &str) -> ProjectScope {
        let trepo = SqlxTenantRepo::new(pool.clone());
        let prepo = SqlxProjectRepo::new(pool.clone());
        let t = trepo.create(email, "h").await.unwrap();
        let scope = trepo.scope_for(t.id).await.unwrap();
        let p = prepo.create(&scope, "Proj", "proj").await.unwrap();
        prepo.open(&scope, p.id).await.unwrap()
    }

    async fn seed_cluster(pool: &PgPool, scope: &ProjectScope) -> Uuid {
        let crepo = SqlxClusterRepo::new(pool.clone());
        crepo
            .create(scope, "Cluster", None, FeedbackKind::Bug, "agent")
            .await
            .unwrap()
            .id
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn create_get_list_round_trip(pool: PgPool) {
        let repo = SqlxRecommendationRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "rec-rt@example.com").await;
        let cluster_id = seed_cluster(&pool, &scope).await;

        let refs = json!([{"path": "src/auth.rs", "lines": "10-20"}]);
        let rec = repo
            .create(
                &scope,
                NewRecommendation {
                    cluster_id,
                    sweep_id: None,
                    action_type: ActionType::BugFix,
                    title: "Fix the auth check",
                    body: "Restore the missing guard",
                    rationale: Some("Many users locked out"),
                    source_refs: &refs,
                    confidence: 0.8,
                },
            )
            .await
            .unwrap();
        assert_eq!(rec.action_type, ActionType::BugFix);
        assert_eq!(rec.status, "proposed");
        assert_eq!(rec.source_refs, refs);

        let got = repo.get(&scope, rec.id).await.unwrap();
        assert_eq!(got, rec);

        let list = repo.list_for_cluster(&scope, cluster_id).await.unwrap();
        assert_eq!(list.len(), 1);

        let approved = repo.set_status(&scope, rec.id, "tweaked_approved").await.unwrap();
        assert_eq!(approved.status, "tweaked_approved");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn create_rejects_cross_tenant_cluster(pool: PgPool) {
        let repo = SqlxRecommendationRepo::new(pool.clone());
        let s1 = seed_project_scope(&pool, "owner1-rec@example.com").await;
        let s2 = seed_project_scope(&pool, "owner2-rec@example.com").await;
        let s1_cluster = seed_cluster(&pool, &s1).await;

        // s2 tries to anchor a recommendation on s1's cluster -> NotFound.
        let refs = json!([]);
        let err = repo
            .create(
                &s2,
                NewRecommendation {
                    cluster_id: s1_cluster,
                    sweep_id: None,
                    action_type: ActionType::NoAction,
                    title: "x",
                    body: "y",
                    rationale: None,
                    source_refs: &refs,
                    confidence: 0.0,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, RepoError::NotFound));
    }
}
