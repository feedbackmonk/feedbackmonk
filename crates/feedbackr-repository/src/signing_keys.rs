//! Signing-key registration repository (FR-FBR-05, Contract C4).
//!
//! Stores Ed25519 public keys per project. The JWT verifier (Stage 2 Worker B)
//! consumes `list_active` to enumerate candidate keys for verification.

use async_trait::async_trait;
use chrono::Utc;
use sqlx::PgPool;

use feedbackr_core::{SigningKey, SigningKeyId};

use crate::error::{RepoError, Result};
use crate::scope::ProjectScope;

#[async_trait]
pub trait SigningKeyRepo: Send + Sync {
    /// Register a new signing key for the project. `public_key` MUST be
    /// 32 raw Ed25519 public-key bytes; the schema column is BYTEA.
    async fn register(&self, scope: &ProjectScope, public_key: &[u8; 32], label: &str) -> Result<SigningKeyId>;

    /// Active keys for this project, in registration order. The JWT
    /// verifier tries each in turn and returns the first success.
    async fn list_active(&self, scope: &ProjectScope) -> Result<Vec<SigningKey>>;

    /// Mark a key inactive. The row is retained for audit; subsequent
    /// `list_active` calls exclude it.
    async fn deactivate(&self, scope: &ProjectScope, id: SigningKeyId) -> Result<()>;
}

#[derive(Clone)]
pub struct SqlxSigningKeyRepo {
    pool: PgPool,
}

impl SqlxSigningKeyRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SigningKeyRepo for SqlxSigningKeyRepo {
    async fn register(&self, scope: &ProjectScope, public_key: &[u8; 32], label: &str) -> Result<SigningKeyId> {
        let bytes: &[u8] = public_key.as_slice();
        let row = sqlx::query!(
            r#"
            INSERT INTO signing_keys (project_id, public_key, label)
            VALUES ($1, $2, $3)
            RETURNING id
            "#,
            scope.project_id(),
            bytes,
            label
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(SigningKeyId(row.id))
    }

    async fn list_active(&self, scope: &ProjectScope) -> Result<Vec<SigningKey>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, project_id, public_key, label, active, registered_at, deactivated_at
            FROM signing_keys
            WHERE project_id = $1 AND active = TRUE
            ORDER BY registered_at ASC
            "#,
            scope.project_id()
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| SigningKey {
                id: SigningKeyId(r.id),
                project_id: r.project_id,
                public_key: r.public_key,
                label: r.label,
                active: r.active,
                registered_at: r.registered_at,
                deactivated_at: r.deactivated_at,
            })
            .collect())
    }

    async fn deactivate(&self, scope: &ProjectScope, id: SigningKeyId) -> Result<()> {
        let now = Utc::now();
        let result = sqlx::query!(
            r#"
            UPDATE signing_keys
            SET active = FALSE, deactivated_at = $1
            WHERE id = $2 AND project_id = $3
            "#,
            now,
            id.into_uuid(),
            scope.project_id()
        )
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(RepoError::NotFound);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::{ProjectRepo, SqlxProjectRepo};
    use crate::tenants::{SqlxTenantRepo, TenantRepo};
    use sqlx::PgPool;

    async fn seed_project_scope(pool: &PgPool, email: &str, project_name: &str) -> ProjectScope {
        let trepo = SqlxTenantRepo::new(pool.clone());
        let prepo = SqlxProjectRepo::new(pool.clone());
        let t = trepo.create(email, "h").await.unwrap();
        let scope = trepo.scope_for(t.id).await.unwrap();
        let p = prepo.create(&scope, project_name, "slug-one").await.unwrap();
        prepo.open(&scope, p.id).await.unwrap()
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn register_then_list_active(pool: PgPool) {
        let repo = SqlxSigningKeyRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "k1@example.com", "K1").await;

        let key_bytes = [7u8; 32];
        let id = repo.register(&scope, &key_bytes, "primary").await.unwrap();

        let keys = repo.list_active(&scope).await.unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].id, id);
        assert_eq!(keys[0].public_key, key_bytes.to_vec());
        assert!(keys[0].active);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn deactivate_excludes_from_list_active(pool: PgPool) {
        let repo = SqlxSigningKeyRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "k2@example.com", "K2").await;

        let id1 = repo.register(&scope, &[1u8; 32], "key1").await.unwrap();
        let _id2 = repo.register(&scope, &[2u8; 32], "key2").await.unwrap();
        assert_eq!(repo.list_active(&scope).await.unwrap().len(), 2);

        repo.deactivate(&scope, id1).await.unwrap();
        let active = repo.list_active(&scope).await.unwrap();
        assert_eq!(active.len(), 1);
        assert_ne!(active[0].id, id1);
    }
}
