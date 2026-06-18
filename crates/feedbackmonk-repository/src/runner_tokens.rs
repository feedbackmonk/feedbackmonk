//! Runner-token lifecycle registry repository (Contract C25, P5b).
//!
//! Admin-visibility bookkeeping for issued runner write-tokens. A runner token
//! is a self-verifying short-TTL JWT (the customer mints it client-side with the
//! private half of a registered `runner`-class signing key — DEC-FBR-04); this
//! registry is OPTIONAL: it lets the admin UI list issued tokens (`jti`, label,
//! `expires_at`) and drive revocation. It is **not** consulted by the verify hot
//! path — the security-load-bearing table is the append-only
//! [`crate::runner_token_revocations`] denylist.
//!
//! `register` is an upsert keyed on `(project_id, jti)` so re-registering the
//! same jti refreshes its label/expiry rather than erroring.
//!
//! Lineage: Contract C25 (FROZEN), FR-FBR-24, DEC-FBR-04, DEC-FBR-03.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::error::Result;
use crate::scope::ProjectScope;

/// A registered runner token (registry view; revocation state is joined from
/// [`crate::runner_token_revocations`] at the handler layer).
#[derive(Debug, Clone)]
pub struct RunnerTokenRecord {
    pub jti: String,
    pub label: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// What the admin supplies when registering an issued token for visibility.
#[derive(Debug, Clone)]
pub struct NewRunnerToken<'a> {
    pub jti: &'a str,
    pub label: &'a str,
    pub expires_at: Option<DateTime<Utc>>,
}

#[async_trait]
pub trait RunnerTokenRepo: Send + Sync {
    /// Register (or refresh) an issued runner token for admin visibility.
    /// Upsert on `(project_id, jti)`.
    async fn register(&self, scope: &ProjectScope, token: NewRunnerToken<'_>) -> Result<()>;

    /// All registered runner tokens for the project, newest first.
    async fn list(&self, scope: &ProjectScope) -> Result<Vec<RunnerTokenRecord>>;
}

#[derive(Clone)]
pub struct SqlxRunnerTokenRepo {
    pool: PgPool,
}

impl SqlxRunnerTokenRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl RunnerTokenRepo for SqlxRunnerTokenRepo {
    async fn register(&self, scope: &ProjectScope, token: NewRunnerToken<'_>) -> Result<()> {
        sqlx::query!(
            r#"
            INSERT INTO runner_tokens (tenant_id, project_id, jti, label, expires_at)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (project_id, jti)
            DO UPDATE SET label = EXCLUDED.label, expires_at = EXCLUDED.expires_at
            "#,
            scope.tenant_id(),
            scope.project_id(),
            token.jti,
            token.label,
            token.expires_at,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list(&self, scope: &ProjectScope) -> Result<Vec<RunnerTokenRecord>> {
        let rows = sqlx::query!(
            r#"
            SELECT jti, label, expires_at, created_at
            FROM runner_tokens
            WHERE project_id = $1
            ORDER BY created_at DESC
            "#,
            scope.project_id(),
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| RunnerTokenRecord {
                jti: r.jti,
                label: r.label,
                expires_at: r.expires_at,
                created_at: r.created_at,
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
    async fn register_then_list_and_upsert(pool: PgPool) {
        let repo = SqlxRunnerTokenRepo::new(pool.clone());
        let scope = seed_scope(&pool, "rt1@example.com").await;

        repo.register(
            &scope,
            NewRunnerToken { jti: "jti-1", label: "ci", expires_at: None },
        )
        .await
        .unwrap();
        let listed = repo.list(&scope).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].jti, "jti-1");
        assert_eq!(listed[0].label, "ci");

        // Re-register same jti => upsert (refresh label), not a duplicate row.
        repo.register(
            &scope,
            NewRunnerToken { jti: "jti-1", label: "ci-renamed", expires_at: None },
        )
        .await
        .unwrap();
        let listed = repo.list(&scope).await.unwrap();
        assert_eq!(listed.len(), 1, "upsert must not duplicate");
        assert_eq!(listed[0].label, "ci-renamed");
    }
}
