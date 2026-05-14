//! Tenant repository -- the only place tenant rows are created or read.
//!
//! `create` and `find_by_email` are the documented pre-authentication exceptions
//! to the `&TenantScope`-first-arg discipline (no scope can exist before a
//! tenant is identified). Both are listed in
//! `.claude/oracles/multi-tenant-isolation-check/allowlist.toml` with rationale.

use async_trait::async_trait;
use sqlx::PgPool;

use feedbackmonk_core::Tenant;

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

    /// Read the email-template brand parameters for `scope` (Contract C10).
    ///
    /// We add `get_brand(&scope)` rather than widening `find_by_email` to
    /// include brand fields: `find_by_email` is allow-listed as a pre-auth
    /// exception (no scope exists at lookup time), and exposing brand
    /// columns through it would unnecessarily widen the pre-auth surface.
    /// `get_brand` requires a `&TenantScope`, so the multi-tenant-isolation
    /// invariant holds the same shape as the rest of the post-auth surface.
    async fn get_brand(&self, scope: &TenantScope) -> Result<EmailTenantBrand>;

    /// Update the brand parameters for `scope`. Stage 2 Worker A wires a
    /// PATCH endpoint on top of this; Stage 1 ships only the repo surface.
    async fn update_brand(&self, scope: &TenantScope, brand: &EmailTenantBrand) -> Result<()>;
}

/// Tenant email-template brand parameters (Contract C10).
///
/// `sender_display_name` is COMPUTED (`"{brand_name} via feedbackmonk"`) and
/// therefore lives in the constructor below, not in the DB columns. All
/// other fields map 1:1 onto migration 00005's columns.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EmailTenantBrand {
    pub brand_name: String,
    pub email_subject_prefix: String,
    pub support_email: String,
    pub unsubscribe_url: Option<String>,
    pub footer_signature: String,
    pub sender_display_name: String,
}

impl EmailTenantBrand {
    /// Build from raw DB column values; derives `sender_display_name`.
    #[must_use]
    pub fn from_db(
        brand_name: String,
        email_subject_prefix: String,
        support_email: String,
        unsubscribe_url: Option<String>,
        footer_signature: String,
    ) -> Self {
        let sender_display_name = format!("{brand_name} via feedbackmonk");
        Self {
            brand_name,
            email_subject_prefix,
            support_email,
            unsubscribe_url,
            footer_signature,
            sender_display_name,
        }
    }
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
        // Brand-column defaults are derived from the email local-part the
        // same way migration 00005 backfilled existing rows: keeps NEW signups
        // (post-00005) and OLD signups (pre-00005) byte-identical. Tenants
        // can override later via `update_brand`.
        let local_part = email.split('@').next().unwrap_or("admin");
        let footer = format!("— The {local_part} team");
        let row = sqlx::query!(
            r#"
            INSERT INTO tenants (
                email, password_hash,
                brand_name, email_subject_prefix, support_email, footer_signature
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id, email, password_hash, verified_at, tier, created_at, updated_at
            "#,
            email,
            password_hash,
            local_part,
            local_part,
            email,
            footer,
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

    async fn get_brand(&self, scope: &TenantScope) -> Result<EmailTenantBrand> {
        let row = sqlx::query!(
            r#"
            SELECT brand_name, email_subject_prefix, support_email,
                   unsubscribe_url, footer_signature
            FROM tenants
            WHERE id = $1
            "#,
            scope.tenant_id(),
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(RepoError::NotFound)?;

        Ok(EmailTenantBrand::from_db(
            row.brand_name,
            row.email_subject_prefix,
            row.support_email,
            row.unsubscribe_url,
            row.footer_signature,
        ))
    }

    async fn update_brand(&self, scope: &TenantScope, brand: &EmailTenantBrand) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE tenants
            SET brand_name           = $2,
                email_subject_prefix = $3,
                support_email        = $4,
                unsubscribe_url      = $5,
                footer_signature     = $6,
                updated_at           = now()
            WHERE id = $1
            "#,
            scope.tenant_id(),
            brand.brand_name,
            brand.email_subject_prefix,
            brand.support_email,
            brand.unsubscribe_url,
            brand.footer_signature,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
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
    async fn get_brand_returns_backfilled_defaults(pool: PgPool) {
        let repo = SqlxTenantRepo::new(pool.clone());
        let t = repo.create("brand@example.com", "h").await.unwrap();
        let scope = repo.scope_for(t.id).await.unwrap();
        let brand = repo.get_brand(&scope).await.unwrap();
        // Migration 00005's backfill: local-part of email -> brand_name.
        assert_eq!(brand.brand_name, "brand");
        assert_eq!(brand.email_subject_prefix, "brand");
        assert_eq!(brand.support_email, "brand@example.com");
        assert_eq!(brand.unsubscribe_url, None);
        assert!(brand.footer_signature.contains("brand"));
        assert_eq!(brand.sender_display_name, "brand via feedbackmonk");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn update_brand_round_trips(pool: PgPool) {
        let repo = SqlxTenantRepo::new(pool.clone());
        let t = repo.create("update@example.com", "h").await.unwrap();
        let scope = repo.scope_for(t.id).await.unwrap();
        let updated = EmailTenantBrand::from_db(
            "Acme".into(),
            "ACME".into(),
            "help@acme.example".into(),
            Some("https://acme.example/unsub".into()),
            "— The Acme team".into(),
        );
        repo.update_brand(&scope, &updated).await.unwrap();

        let read_back = repo.get_brand(&scope).await.unwrap();
        assert_eq!(read_back.brand_name, "Acme");
        assert_eq!(read_back.email_subject_prefix, "ACME");
        assert_eq!(read_back.support_email, "help@acme.example");
        assert_eq!(
            read_back.unsubscribe_url.as_deref(),
            Some("https://acme.example/unsub")
        );
        assert_eq!(read_back.footer_signature, "— The Acme team");
        assert_eq!(read_back.sender_display_name, "Acme via feedbackmonk");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn get_brand_cross_tenant_negative(pool: PgPool) {
        let repo = SqlxTenantRepo::new(pool.clone());
        let t1 = repo.create("a@example.com", "h").await.unwrap();
        let t2 = repo.create("b@example.com", "h").await.unwrap();
        let scope1 = repo.scope_for(t1.id).await.unwrap();
        let scope2 = repo.scope_for(t2.id).await.unwrap();
        // Each scope sees only its own brand.
        let b1 = repo.get_brand(&scope1).await.unwrap();
        let b2 = repo.get_brand(&scope2).await.unwrap();
        assert_ne!(b1.brand_name, b2.brand_name);
        assert_eq!(b1.brand_name, "a");
        assert_eq!(b2.brand_name, "b");
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
