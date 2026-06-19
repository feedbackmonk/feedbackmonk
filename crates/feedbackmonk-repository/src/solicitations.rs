//! Solicitation-state repository (Capability 2, FR-FBR-29).
//!
//! Pure storage for the durable per-(project, end-user) solicitation record.
//! The state machine + transition legality live in
//! `feedbackmonk-core::solicitation`; the cooldown/frequency-cap POLICY lives in
//! the API handler. This repo only reads the current record and upserts a new
//! one. Tenant isolation (DEC-FBR-03): every query filters on both `tenant_id`
//! and `project_id` from the [`ProjectScope`]; `end_user_sub` is the per-user
//! key.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use feedbackmonk_core::SolicitationStatus;

use crate::error::Result;
use crate::scope::ProjectScope;

/// A persisted solicitation record. Absence of a row is modeled at the call
/// site as `None` (meaning the default `eligible` state).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolicitationRecord {
    pub status: SolicitationStatus,
    pub prompt_count: i64,
    /// Timestamp of the most recent `prompted` event (drives the cooldown).
    pub prompted_at: Option<DateTime<Utc>>,
    pub last_event_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[async_trait]
pub trait SolicitationRepo: Send + Sync {
    /// Read the current record for `(scope, end_user_sub)`. `None` ⇒ no record
    /// yet (the caller treats this as the default `eligible` state).
    async fn get(
        &self,
        scope: &ProjectScope,
        end_user_sub: &str,
    ) -> Result<Option<SolicitationRecord>>;

    /// Upsert the record for `(scope, end_user_sub)` to the supplied state. The
    /// caller has already validated the transition (via
    /// `feedbackmonk_core::apply_solicitation_event`) and computed the new
    /// `prompt_count` / `prompted_at`. `last_event_at` + `updated_at` are
    /// stamped `now()` server-side. Returns the stored record.
    async fn upsert(
        &self,
        scope: &ProjectScope,
        end_user_sub: &str,
        status: SolicitationStatus,
        prompt_count: i64,
        prompted_at: Option<DateTime<Utc>>,
    ) -> Result<SolicitationRecord>;
}

#[derive(Clone)]
pub struct SqlxSolicitationRepo {
    pool: PgPool,
}

impl SqlxSolicitationRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SolicitationRepo for SqlxSolicitationRepo {
    async fn get(
        &self,
        scope: &ProjectScope,
        end_user_sub: &str,
    ) -> Result<Option<SolicitationRecord>> {
        let row = sqlx::query!(
            r#"
            SELECT status, prompt_count, prompted_at, last_event_at, created_at, updated_at
            FROM feedback_solicitations
            WHERE tenant_id = $1 AND project_id = $2 AND end_user_sub = $3
            "#,
            scope.tenant_id(),
            scope.project_id(),
            end_user_sub,
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| SolicitationRecord {
            status: SolicitationStatus::from_db_str(&r.status),
            prompt_count: i64::from(r.prompt_count),
            prompted_at: r.prompted_at,
            last_event_at: r.last_event_at,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }))
    }

    async fn upsert(
        &self,
        scope: &ProjectScope,
        end_user_sub: &str,
        status: SolicitationStatus,
        prompt_count: i64,
        prompted_at: Option<DateTime<Utc>>,
    ) -> Result<SolicitationRecord> {
        let status_str = status.as_db_str();
        let prompt_count_i32 = i32::try_from(prompt_count).unwrap_or(i32::MAX);
        let row = sqlx::query!(
            r#"
            INSERT INTO feedback_solicitations (
                tenant_id, project_id, end_user_sub, status, prompt_count, prompted_at,
                last_event_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, now(), now())
            ON CONFLICT (project_id, end_user_sub) DO UPDATE SET
                status        = EXCLUDED.status,
                prompt_count  = EXCLUDED.prompt_count,
                prompted_at   = EXCLUDED.prompted_at,
                last_event_at = now(),
                updated_at    = now()
            RETURNING status, prompt_count, prompted_at, last_event_at, created_at, updated_at
            "#,
            scope.tenant_id(),
            scope.project_id(),
            end_user_sub,
            status_str,
            prompt_count_i32,
            prompted_at,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(SolicitationRecord {
            status: SolicitationStatus::from_db_str(&row.status),
            prompt_count: i64::from(row.prompt_count),
            prompted_at: row.prompted_at,
            last_event_at: row.last_event_at,
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
    use sqlx::PgPool;

    async fn seed_project_scope(pool: &PgPool, email: &str) -> ProjectScope {
        let trepo = SqlxTenantRepo::new(pool.clone());
        let prepo = SqlxProjectRepo::new(pool.clone());
        let t = trepo.create(email, "h").await.unwrap();
        let scope = trepo.scope_for(t.id).await.unwrap();
        let p = prepo.create(&scope, "Proj", "proj").await.unwrap();
        prepo.open(&scope, p.id).await.unwrap()
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn get_absent_is_none(pool: PgPool) {
        let repo = SqlxSolicitationRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "solicit-none@example.com").await;
        assert!(repo.get(&scope, "auth0|nobody").await.unwrap().is_none());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn upsert_inserts_then_updates(pool: PgPool) {
        let repo = SqlxSolicitationRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "solicit-upsert@example.com").await;
        let sub = "auth0|sub-1";

        let prompted_at = Utc::now();
        let rec = repo
            .upsert(&scope, sub, SolicitationStatus::Prompted, 1, Some(prompted_at))
            .await
            .unwrap();
        assert_eq!(rec.status, SolicitationStatus::Prompted);
        assert_eq!(rec.prompt_count, 1);
        assert!(rec.prompted_at.is_some());

        // Second upsert (same key) updates in place — still one row.
        let rec2 = repo
            .upsert(&scope, sub, SolicitationStatus::Dismissed, 1, Some(prompted_at))
            .await
            .unwrap();
        assert_eq!(rec2.status, SolicitationStatus::Dismissed);
        assert_eq!(rec2.prompt_count, 1);

        let got = repo.get(&scope, sub).await.unwrap().unwrap();
        assert_eq!(got.status, SolicitationStatus::Dismissed);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn cross_tenant_isolation(pool: PgPool) {
        let repo = SqlxSolicitationRepo::new(pool.clone());
        let s1 = seed_project_scope(&pool, "solicit-ct1@example.com").await;
        let s2 = seed_project_scope(&pool, "solicit-ct2@example.com").await;
        let sub = "auth0|shared-sub"; // same sub string, different tenants/projects

        repo.upsert(&s1, sub, SolicitationStatus::OptedOut, 0, None)
            .await
            .unwrap();

        // s2 must NOT see s1's record for the same sub string.
        assert!(repo.get(&s2, sub).await.unwrap().is_none());
        // s1 still sees its own.
        assert_eq!(
            repo.get(&s1, sub).await.unwrap().unwrap().status,
            SolicitationStatus::OptedOut
        );
    }
}
