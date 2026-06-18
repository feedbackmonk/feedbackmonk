//! Signing-key registration repository (FR-FBR-05, Contract C4).
//!
//! Stores Ed25519 public keys per project. The JWT verifier (Stage 2 Worker B)
//! consumes `list_active` to enumerate candidate keys for verification.

use async_trait::async_trait;
use chrono::Utc;
use sqlx::PgPool;

use feedbackmonk_core::{KeyClass, SigningKey, SigningKeyId};

use crate::error::{RepoError, Result};
use crate::scope::ProjectScope;

#[async_trait]
pub trait SigningKeyRepo: Send + Sync {
    /// Register a new end-user IDENTITY signing key for the project (the
    /// default class). `public_key` MUST be 32 raw Ed25519 public-key bytes;
    /// the schema column is BYTEA. Convenience wrapper over
    /// [`SigningKeyRepo::register_with_class`] preserved so existing callers
    /// (and the end-user submission key path) are unchanged.
    async fn register(&self, scope: &ProjectScope, public_key: &[u8; 32], label: &str) -> Result<SigningKeyId>;

    /// Register a signing key of an explicit [`KeyClass`] (Contract C25, P5b).
    /// Registering a [`KeyClass::Runner`] key is how a customer enables runner
    /// minting; an `identity` key backs end-user submission JWTs.
    async fn register_with_class(
        &self,
        scope: &ProjectScope,
        public_key: &[u8; 32],
        label: &str,
        key_class: KeyClass,
    ) -> Result<SigningKeyId>;

    /// All active keys for this project regardless of class, in registration
    /// order. Used for admin listing/echo only — **never** the verify path
    /// (which MUST class-filter via [`SigningKeyRepo::list_active_for_class`]).
    async fn list_active(&self, scope: &ProjectScope) -> Result<Vec<SigningKey>>;

    /// Active keys of a given [`KeyClass`] for this project, in registration
    /// order (Contract C25 — the privilege-separation chokepoint). The JWT
    /// verifier tries each in turn and returns the first success. End-user
    /// verification passes [`KeyClass::Identity`]; runner-token verification
    /// passes [`KeyClass::Runner`]. A runner key can never verify an end-user
    /// JWT and vice-versa.
    async fn list_active_for_class(
        &self,
        scope: &ProjectScope,
        key_class: KeyClass,
    ) -> Result<Vec<SigningKey>>;

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
        self.register_with_class(scope, public_key, label, KeyClass::Identity)
            .await
    }

    async fn register_with_class(
        &self,
        scope: &ProjectScope,
        public_key: &[u8; 32],
        label: &str,
        key_class: KeyClass,
    ) -> Result<SigningKeyId> {
        let bytes: &[u8] = public_key.as_slice();
        let row = sqlx::query!(
            r#"
            INSERT INTO signing_keys (project_id, public_key, label, key_class)
            VALUES ($1, $2, $3, $4)
            RETURNING id
            "#,
            scope.project_id(),
            bytes,
            label,
            key_class.as_db_str()
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

    async fn list_active_for_class(
        &self,
        scope: &ProjectScope,
        key_class: KeyClass,
    ) -> Result<Vec<SigningKey>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, project_id, public_key, label, active, registered_at, deactivated_at
            FROM signing_keys
            WHERE project_id = $1 AND active = TRUE AND key_class = $2
            ORDER BY registered_at ASC
            "#,
            scope.project_id(),
            key_class.as_db_str()
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
    async fn class_selection_separates_identity_from_runner(pool: PgPool) {
        let repo = SqlxSigningKeyRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "kc@example.com", "KC").await;

        // Default register => identity class.
        let id_key = repo.register(&scope, &[3u8; 32], "id-key").await.unwrap();
        // Explicit runner class.
        let runner_key = repo
            .register_with_class(&scope, &[4u8; 32], "runner-key", KeyClass::Runner)
            .await
            .unwrap();

        // list_active (class-agnostic) sees both.
        assert_eq!(repo.list_active(&scope).await.unwrap().len(), 2);

        // Class selection is the privilege boundary: each class sees only its own.
        let identity = repo
            .list_active_for_class(&scope, KeyClass::Identity)
            .await
            .unwrap();
        assert_eq!(identity.len(), 1);
        assert_eq!(identity[0].id, id_key);

        let runner = repo
            .list_active_for_class(&scope, KeyClass::Runner)
            .await
            .unwrap();
        assert_eq!(runner.len(), 1);
        assert_eq!(runner[0].id, runner_key);
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
