//! Password-reset token repository (security scrutiny P1-1).
//!
//! Sibling of [`crate::email_verifications`] with two deliberate differences:
//!   1. The stored key is `token_hash` (sha256 hex of the wire token), NOT the
//!      raw token — a read of this table cannot be replayed to reset an account
//!      (defense-in-depth, P2-16 sibling). Hashing lives in the API layer; this
//!      repo stores/looks up an opaque digest string.
//!   2. Short TTL (default 1h) — a reset link is far more sensitive than a
//!      verify link.
//!
//! Three methods:
//!   - `create`   -- post-request, mint a reset token for an existing (verified)
//!     tenant. Scope-disciplined.
//!   - `redeem`   -- pre-auth allowlisted; the token IS the credential (the
//!     tenant cannot be authenticated by password at reset time — they forgot
//!     it). Returns the stored row by digest. Mirrors
//!     `EmailVerificationRepo::redeem`.
//!   - `mark_used` -- post-confirm, scope-disciplined single-use marker.
//!
//! Allowlist entry for `redeem`:
//! `.claude/oracles/multi-tenant-isolation-check/allowlist.toml`
//!   `[[methods]] trait = "PasswordResetRepo" method = "redeem"`

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{RepoError, Result};
use crate::scope::TenantScope;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordReset {
    pub tenant_id: Uuid,
    pub expires_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
}

#[async_trait]
pub trait PasswordResetRepo: Send + Sync {
    async fn create(
        &self,
        scope: &TenantScope,
        token_hash: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<()>;

    // allowlisted-pre-auth: opaque reset token (by digest) IS the credential;
    // the tenant forgot their password so no TenantScope can exist yet.
    async fn redeem(&self, token_hash: &str) -> Result<Option<PasswordReset>>;

    async fn mark_used(&self, scope: &TenantScope, token_hash: &str) -> Result<()>;
}

#[derive(Clone)]
pub struct SqlxPasswordResetRepo {
    pool: PgPool,
}

impl SqlxPasswordResetRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PasswordResetRepo for SqlxPasswordResetRepo {
    async fn create(
        &self,
        scope: &TenantScope,
        token_hash: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            INSERT INTO password_resets (token_hash, tenant_id, expires_at)
            VALUES ($1, $2, $3)
            "#,
            token_hash,
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

    async fn redeem(&self, token_hash: &str) -> Result<Option<PasswordReset>> {
        let row = sqlx::query!(
            r#"
            SELECT tenant_id, expires_at, used_at
            FROM password_resets
            WHERE token_hash = $1
            "#,
            token_hash,
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| PasswordReset {
            tenant_id: r.tenant_id,
            expires_at: r.expires_at,
            used_at: r.used_at,
        }))
    }

    async fn mark_used(&self, scope: &TenantScope, token_hash: &str) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE password_resets
            SET used_at = now()
            WHERE token_hash = $1 AND tenant_id = $2 AND used_at IS NULL
            "#,
            token_hash,
            scope.tenant_id(),
        )
        .execute(&self.pool)
        .await?;
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
        let repo = SqlxPasswordResetRepo::new(pool.clone());
        let scope = seed_tenant(&pool, "reset1@example.com").await;
        let expires = Utc::now() + Duration::hours(1);
        repo.create(&scope, "digest-abc", expires).await.unwrap();

        let r = repo.redeem("digest-abc").await.unwrap().unwrap();
        assert_eq!(r.tenant_id, scope.tenant_id());
        assert!(r.used_at.is_none());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn redeem_unknown_digest_returns_none(pool: PgPool) {
        let repo = SqlxPasswordResetRepo::new(pool);
        assert!(repo.redeem("never-issued").await.unwrap().is_none());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn mark_used_sets_used_at(pool: PgPool) {
        let repo = SqlxPasswordResetRepo::new(pool.clone());
        let scope = seed_tenant(&pool, "reset2@example.com").await;
        let expires = Utc::now() + Duration::hours(1);
        repo.create(&scope, "digest-def", expires).await.unwrap();

        repo.mark_used(&scope, "digest-def").await.unwrap();
        let r = repo.redeem("digest-def").await.unwrap().unwrap();
        assert!(r.used_at.is_some());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn cascade_delete_when_tenant_deleted(pool: PgPool) {
        let repo = SqlxPasswordResetRepo::new(pool.clone());
        let scope = seed_tenant(&pool, "cascade-reset@example.com").await;
        let expires = Utc::now() + Duration::hours(1);
        repo.create(&scope, "digest-cascade", expires).await.unwrap();

        sqlx::query!("DELETE FROM tenants WHERE id = $1", scope.tenant_id())
            .execute(&pool)
            .await
            .unwrap();

        assert!(repo.redeem("digest-cascade").await.unwrap().is_none());
    }
}
