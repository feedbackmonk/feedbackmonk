//! Runner-token revocation denylist repository (Contract C25, P5b).
//!
//! The **append-only, load-bearing security table**: `verify_runner_token`
//! consults [`RunnerTokenRevocationRepo::is_revoked`] before honoring a runner
//! write-token, so an owner can kill a specific token (by its `jti` claim)
//! BEFORE its short `exp`. Append-only — no UPDATE/DELETE methods — mirroring
//! the `work_order_events` / `feedback_status_history` ledger discipline: a
//! revocation is permanent.
//!
//! A `jti` can be revoked WITHOUT prior registration in `runner_tokens`
//! (revoke-before-register), so this denylist is independent of the lifecycle
//! registry. `revoke` is idempotent (`ON CONFLICT DO NOTHING`).
//!
//! Lineage: Contract C25 (FROZEN), FR-FBR-24, DEC-FBR-04, DEC-FBR-03 (sole
//! query path — `multi-tenant-isolation-check` auto-covers this table).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::error::Result;
use crate::scope::ProjectScope;

/// One revoked runner-token record (admin visibility / audit).
#[derive(Debug, Clone)]
pub struct RunnerTokenRevocation {
    pub jti: String,
    pub label: Option<String>,
    pub revoked_at: DateTime<Utc>,
}

#[async_trait]
pub trait RunnerTokenRevocationRepo: Send + Sync {
    /// Revoke a runner token by `jti` (idempotent). `label` is copied for audit
    /// when known (e.g. from the registry); `None` when revoking an
    /// unregistered jti. Append-only: a second revoke of the same jti is a
    /// no-op, never an error.
    async fn revoke(&self, scope: &ProjectScope, jti: &str, label: Option<&str>) -> Result<()>;

    /// The verify hot path: is this `(project, jti)` on the denylist?
    async fn is_revoked(&self, scope: &ProjectScope, jti: &str) -> Result<bool>;

    /// All revocations for the project, newest first (admin audit / GET join).
    async fn list(&self, scope: &ProjectScope) -> Result<Vec<RunnerTokenRevocation>>;
}

#[derive(Clone)]
pub struct SqlxRunnerTokenRevocationRepo {
    pool: PgPool,
}

impl SqlxRunnerTokenRevocationRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl RunnerTokenRevocationRepo for SqlxRunnerTokenRevocationRepo {
    async fn revoke(&self, scope: &ProjectScope, jti: &str, label: Option<&str>) -> Result<()> {
        sqlx::query!(
            r#"
            INSERT INTO runner_token_revocations (tenant_id, project_id, jti, label)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (project_id, jti) DO NOTHING
            "#,
            scope.tenant_id(),
            scope.project_id(),
            jti,
            label,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn is_revoked(&self, scope: &ProjectScope, jti: &str) -> Result<bool> {
        let row = sqlx::query!(
            r#"
            SELECT 1 AS hit
            FROM runner_token_revocations
            WHERE project_id = $1 AND jti = $2
            "#,
            scope.project_id(),
            jti,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.is_some())
    }

    async fn list(&self, scope: &ProjectScope) -> Result<Vec<RunnerTokenRevocation>> {
        let rows = sqlx::query!(
            r#"
            SELECT jti, label, revoked_at
            FROM runner_token_revocations
            WHERE project_id = $1
            ORDER BY revoked_at DESC
            "#,
            scope.project_id(),
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| RunnerTokenRevocation {
                jti: r.jti,
                label: r.label,
                revoked_at: r.revoked_at,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::{ProjectRepo, SqlxProjectRepo};
    use crate::tenants::{SqlxTenantRepo, TenantRepo};

    async fn seed_scope(pool: &PgPool, email: &str) -> ProjectScope {
        let trepo = SqlxTenantRepo::new(pool.clone());
        let prepo = SqlxProjectRepo::new(pool.clone());
        let t = trepo.create(email, "h").await.unwrap();
        let scope = trepo.scope_for(t.id).await.unwrap();
        let p = prepo.create(&scope, "P", "p-slug").await.unwrap();
        prepo.open(&scope, p.id).await.unwrap()
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn revoke_then_is_revoked_round_trip(pool: PgPool) {
        let repo = SqlxRunnerTokenRevocationRepo::new(pool.clone());
        let scope = seed_scope(&pool, "rev1@example.com").await;

        assert!(!repo.is_revoked(&scope, "jti-1").await.unwrap());
        repo.revoke(&scope, "jti-1", Some("ci-runner")).await.unwrap();
        assert!(repo.is_revoked(&scope, "jti-1").await.unwrap());

        // Idempotent: a second revoke is a no-op (append-only, never an error).
        repo.revoke(&scope, "jti-1", None).await.unwrap();
        let listed = repo.list(&scope).await.unwrap();
        assert_eq!(listed.len(), 1, "double-revoke must not duplicate");
        assert_eq!(listed[0].jti, "jti-1");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn revocation_is_project_scoped(pool: PgPool) {
        let repo = SqlxRunnerTokenRevocationRepo::new(pool.clone());
        let a = seed_scope(&pool, "rev-a@example.com").await;
        let b = seed_scope(&pool, "rev-b@example.com").await;

        repo.revoke(&a, "shared-jti", None).await.unwrap();
        // The same jti string under a sibling project is NOT revoked.
        assert!(repo.is_revoked(&a, "shared-jti").await.unwrap());
        assert!(!repo.is_revoked(&b, "shared-jti").await.unwrap());
    }
}
