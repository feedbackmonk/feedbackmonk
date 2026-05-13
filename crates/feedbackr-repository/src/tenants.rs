//! Tenant repository -- the only place tenant rows are created or read.
//!
//! `create` and `find_by_email` are the documented pre-authentication exceptions
//! to the `&TenantScope`-first-arg discipline (no scope can exist before a
//! tenant is identified). Both are listed in
//! `.claude/oracles/multi-tenant-isolation-check/allowlist.toml` with rationale.

use async_trait::async_trait;
use sqlx::PgPool;

use feedbackr_core::Tenant;

use crate::error::{RepoError, Result};
use crate::scope::TenantScope;

#[async_trait]
pub trait TenantRepo: Send + Sync {
    // allowlisted-pre-auth: signup creates the tenant before any scope exists.
    async fn create(&self, email: &str, password_hash: &str) -> Result<Tenant>;

    // allowlisted-pre-auth: login lookup runs before password verification.
    async fn find_by_email(&self, email: &str) -> Result<Option<Tenant>>;

    async fn get(&self, scope: &TenantScope) -> Result<Tenant>;

    async fn mark_verified(&self, scope: &TenantScope) -> Result<()>;

    /// Mint a `TenantScope` for the tenant identified by `id`. This is the
    /// SOLE bridge from a raw `Uuid` to a `TenantScope`; callers must have
    /// already authenticated the bearer (e.g. validated a session cookie or
    /// verified a password). Stage 2 Worker A wraps this behind login/session
    /// handlers; Stage 1 exposes it for the test harness.
    async fn scope_for(&self, id: uuid::Uuid) -> Result<TenantScope>;
}

#[derive(Clone)]
pub struct SqlxTenantRepo {
    pool: PgPool,
}

impl SqlxTenantRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TenantRepo for SqlxTenantRepo {
    async fn create(&self, email: &str, password_hash: &str) -> Result<Tenant> {
        let row = sqlx::query!(
            r#"
            INSERT INTO tenants (email, password_hash)
            VALUES ($1, $2)
            RETURNING id, email, password_hash, verified_at, tier, created_at, updated_at
            "#,
            email,
            password_hash
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.is_unique_violation() => RepoError::Conflict,
            other => RepoError::Sqlx(other),
        })?;

        Ok(Tenant {
            id: row.id,
            email: row.email,
            password_hash: row.password_hash,
            verified_at: row.verified_at,
            tier: row.tier,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }

    async fn find_by_email(&self, email: &str) -> Result<Option<Tenant>> {
        let row = sqlx::query!(
            r#"
            SELECT id, email, password_hash, verified_at, tier, created_at, updated_at
            FROM tenants WHERE email = $1
            "#,
            email
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| Tenant {
            id: r.id,
            email: r.email,
            password_hash: r.password_hash,
            verified_at: r.verified_at,
            tier: r.tier,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }))
    }

    async fn get(&self, scope: &TenantScope) -> Result<Tenant> {
        let row = sqlx::query!(
            r#"
            SELECT id, email, password_hash, verified_at, tier, created_at, updated_at
            FROM tenants WHERE id = $1
            "#,
            scope.tenant_id()
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(RepoError::NotFound)?;

        Ok(Tenant {
            id: row.id,
            email: row.email,
            password_hash: row.password_hash,
            verified_at: row.verified_at,
            tier: row.tier,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }

    async fn mark_verified(&self, scope: &TenantScope) -> Result<()> {
        sqlx::query!(
            "UPDATE tenants SET verified_at = now(), updated_at = now() WHERE id = $1",
            scope.tenant_id()
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn scope_for(&self, id: uuid::Uuid) -> Result<TenantScope> {
        let exists = sqlx::query!("SELECT id FROM tenants WHERE id = $1", id)
            .fetch_optional(&self.pool)
            .await?;
        if exists.is_none() {
            return Err(RepoError::NotFound);
        }
        Ok(TenantScope::new(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::PgPool;

    #[sqlx::test(migrations = "../../migrations")]
    async fn create_then_find_by_email_round_trip(pool: PgPool) {
        let repo = SqlxTenantRepo::new(pool);
        let t = repo.create("alice@example.com", "argon2-hash-stub").await.unwrap();
        assert_eq!(t.email, "alice@example.com");
        assert!(t.verified_at.is_none());

        let found = repo.find_by_email("alice@example.com").await.unwrap();
        assert_eq!(found.unwrap().id, t.id);

        let missing = repo.find_by_email("nobody@example.com").await.unwrap();
        assert!(missing.is_none());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn create_duplicate_email_yields_conflict(pool: PgPool) {
        let repo = SqlxTenantRepo::new(pool);
        repo.create("dup@example.com", "h").await.unwrap();
        let err = repo.create("dup@example.com", "h2").await.unwrap_err();
        assert!(matches!(err, RepoError::Conflict));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn scope_for_unknown_tenant_returns_not_found(pool: PgPool) {
        let repo = SqlxTenantRepo::new(pool);
        let err = repo.scope_for(uuid::Uuid::new_v4()).await.unwrap_err();
        assert!(matches!(err, RepoError::NotFound));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn mark_verified_sets_timestamp(pool: PgPool) {
        let repo = SqlxTenantRepo::new(pool.clone());
        let t = repo.create("verify@example.com", "h").await.unwrap();
        let scope = repo.scope_for(t.id).await.unwrap();
        repo.mark_verified(&scope).await.unwrap();
        let after = repo.get(&scope).await.unwrap();
        assert!(after.verified_at.is_some());
    }
}
