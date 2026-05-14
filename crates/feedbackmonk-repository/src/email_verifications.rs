//! Email-verification token repository (FR-FBR-02).
//!
//! Three methods:
//!   - `create`   -- post-signup, mint a verify token for a newly-created tenant.
//!   - `redeem`   -- pre-auth allowlisted; the token IS the credential. Returns
//!     the stored row (without authenticating a tenant first, which would be
//!     impossible -- the tenant isn't verified yet). Mirrors
//!     `TenantRepo::find_by_email` rationale.
//!   - `mark_used` -- post-redemption, scope-disciplined.
//!
//! Allowlist entry for `redeem`:
//! `.claude/oracles/multi-tenant-isolation-check/allowlist.toml`
//!   `[[methods]] trait = "EmailVerificationRepo" method = "redeem"`
//!   `rationale = "Pre-auth boundary: opaque token IS the credential."`

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{RepoError, Result};
use crate::scope::TenantScope;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redemption {
    pub tenant_id: Uuid,
    pub expires_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
}

#[async_trait]
pub trait EmailVerificationRepo: Send + Sync {
    async fn create(
        &self,
        scope: &TenantScope,
        token: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<()>;

    // allowlisted-pre-auth: token-as-credential lookup; no tenant scope can
    // exist yet because the tenant is in pending-verification state.
    async fn redeem(&self, token: &str) -> Result<Option<Redemption>>;

    async fn mark_used(&self, scope: &TenantScope, token: &str) -> Result<()>;
}

#[derive(Clone)]
pub struct SqlxEmailVerificationRepo {
    pool: PgPool,
}

impl SqlxEmailVerificationRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl EmailVerificationRepo for SqlxEmailVerificationRepo {
    async fn create(
        &self,
        scope: &TenantScope,
        token: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            INSERT INTO email_verifications (token, tenant_id, expires_at)
            VALUES ($1, $2, $3)
            "#,
            token,
            scope.tenant_id(),
            expires_at,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.is_unique_violation() => RepoError::Conflict,
            other => RepoError::Sqlx(other),
        })?;
        Ok(())
    }

    async fn redeem(&self, token: &str) -> Result<Option<Redemption>> {
        let row = sqlx::query!(
            r#"
            SELECT tenant_id, expires_at, used_at
            FROM email_verifications
            WHERE token = $1
            "#,
            token,
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| Redemption {
            tenant_id: r.tenant_id,
            expires_at: r.expires_at,
            used_at: r.used_at,
        }))
    }

    async fn mark_used(&self, scope: &TenantScope, token: &str) -> Result<()> {
        let result = sqlx::query!(
            r#"
            UPDATE email_verifications
            SET used_at = now()
            WHERE token = $1 AND tenant_id = $2 AND used_at IS NULL
            "#,
            token,
            scope.tenant_id(),
        )
        .execute(&self.pool)
        .await?;

        // No rows affected is OK on a double-redemption (idempotency); the
        // caller checked `used_at` already to decide replay-window policy.
        // Returning Ok regardless keeps the call idempotent.
        let _ = result;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tenants::{SqlxTenantRepo, TenantRepo};
    use chrono::Duration;
    use sqlx::PgPool;

    async fn seed_tenant(pool: &PgPool, email: &str) -> TenantScope {
        let trepo = SqlxTenantRepo::new(pool.clone());
        let t = trepo.create(email, "h").await.unwrap();
        trepo.scope_for(t.id).await.unwrap()
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn create_then_redeem_returns_unused_row(pool: PgPool) {
        let repo = SqlxEmailVerificationRepo::new(pool.clone());
        let scope = seed_tenant(&pool, "ver1@example.com").await;
        let expires = Utc::now() + Duration::hours(24);
        repo.create(&scope, "token-abc", expires).await.unwrap();

        let r = repo.redeem("token-abc").await.unwrap().unwrap();
        assert_eq!(r.tenant_id, scope.tenant_id());
        assert!(r.used_at.is_none());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn redeem_unknown_token_returns_none(pool: PgPool) {
        let repo = SqlxEmailVerificationRepo::new(pool);
        assert!(repo.redeem("never-issued").await.unwrap().is_none());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn mark_used_sets_used_at(pool: PgPool) {
        let repo = SqlxEmailVerificationRepo::new(pool.clone());
        let scope = seed_tenant(&pool, "ver2@example.com").await;
        let expires = Utc::now() + Duration::hours(24);
        repo.create(&scope, "token-def", expires).await.unwrap();

        repo.mark_used(&scope, "token-def").await.unwrap();
        let r = repo.redeem("token-def").await.unwrap().unwrap();
        assert!(r.used_at.is_some());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn create_duplicate_token_yields_conflict(pool: PgPool) {
        let repo = SqlxEmailVerificationRepo::new(pool.clone());
        let scope = seed_tenant(&pool, "ver3@example.com").await;
        let expires = Utc::now() + Duration::hours(24);
        repo.create(&scope, "dup-token", expires).await.unwrap();
        let err = repo.create(&scope, "dup-token", expires).await.unwrap_err();
        assert!(matches!(err, RepoError::Conflict));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn cascade_delete_when_tenant_deleted(pool: PgPool) {
        let repo = SqlxEmailVerificationRepo::new(pool.clone());
        let scope = seed_tenant(&pool, "cascade@example.com").await;
        let expires = Utc::now() + Duration::hours(24);
        repo.create(&scope, "tk", expires).await.unwrap();

        // FK is ON DELETE CASCADE; deleting the tenant must remove the token.
        sqlx::query!("DELETE FROM tenants WHERE id = $1", scope.tenant_id())
            .execute(&pool)
            .await
            .unwrap();

        assert!(repo.redeem("tk").await.unwrap().is_none());
    }
}
