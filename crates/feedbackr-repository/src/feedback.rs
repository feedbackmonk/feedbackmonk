//! Feedback repository (FR-FBR-01, Contract C1).
//!
//! Two submission methods, mirroring the auth-mode/anonymous-mode split in
//! Contract C3. The schema enforces the XOR invariant via a CHECK constraint
//! (exactly one of `end_user_sub` / `anon_token_hash` is non-NULL).

use async_trait::async_trait;
use serde_json::Value as JsonValue;
use sqlx::PgPool;

use feedbackr_core::{Feedback, FeedbackId, FeedbackKind};

use crate::error::Result;
use crate::scope::ProjectScope;

#[async_trait]
pub trait FeedbackRepo: Send + Sync {
    async fn submit_authenticated(
        &self,
        scope: &ProjectScope,
        end_user_sub: &str,
        end_user_email: Option<&str>,
        end_user_name: Option<&str>,
        external_metadata: Option<&JsonValue>,
        body: &str,
        kind: FeedbackKind,
    ) -> Result<FeedbackId>;

    async fn submit_anonymous(
        &self,
        scope: &ProjectScope,
        anon_token_hash: &[u8; 32],
        optional_email: Option<&str>,
        body: &str,
        kind: FeedbackKind,
    ) -> Result<FeedbackId>;

    async fn list_recent(&self, scope: &ProjectScope, limit: i64) -> Result<Vec<Feedback>>;
}

#[derive(Clone)]
pub struct SqlxFeedbackRepo {
    pool: PgPool,
}

impl SqlxFeedbackRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl FeedbackRepo for SqlxFeedbackRepo {
    async fn submit_authenticated(
        &self,
        scope: &ProjectScope,
        end_user_sub: &str,
        end_user_email: Option<&str>,
        end_user_name: Option<&str>,
        external_metadata: Option<&JsonValue>,
        body: &str,
        kind: FeedbackKind,
    ) -> Result<FeedbackId> {
        let short_code = FeedbackId::generate();
        let kind_str = kind.as_str();
        sqlx::query!(
            r#"
            INSERT INTO feedback (
                short_code, project_id, tenant_id,
                end_user_sub, end_user_email, end_user_name,
                external_metadata, body, kind
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
            short_code.as_str(),
            scope.project_id(),
            scope.tenant_id(),
            end_user_sub,
            end_user_email,
            end_user_name,
            external_metadata,
            body,
            kind_str,
        )
        .execute(&self.pool)
        .await?;
        Ok(short_code)
    }

    async fn submit_anonymous(
        &self,
        scope: &ProjectScope,
        anon_token_hash: &[u8; 32],
        optional_email: Option<&str>,
        body: &str,
        kind: FeedbackKind,
    ) -> Result<FeedbackId> {
        let short_code = FeedbackId::generate();
        let kind_str = kind.as_str();
        let token: &[u8] = anon_token_hash.as_slice();
        sqlx::query!(
            r#"
            INSERT INTO feedback (
                short_code, project_id, tenant_id,
                end_user_email, anon_token_hash, body, kind
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
            short_code.as_str(),
            scope.project_id(),
            scope.tenant_id(),
            optional_email,
            token,
            body,
            kind_str,
        )
        .execute(&self.pool)
        .await?;

        // Upsert the anon_submissions counter (dedup tracking; FR-FBR-06).
        sqlx::query!(
            r#"
            INSERT INTO anon_submissions (anon_token_hash, project_id)
            VALUES ($1, $2)
            ON CONFLICT (anon_token_hash, project_id) DO UPDATE
              SET last_submission_at = now(),
                  submission_count = anon_submissions.submission_count + 1
            "#,
            token,
            scope.project_id(),
        )
        .execute(&self.pool)
        .await?;

        Ok(short_code)
    }

    async fn list_recent(&self, scope: &ProjectScope, limit: i64) -> Result<Vec<Feedback>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, short_code, project_id, tenant_id,
                   end_user_sub, end_user_email, end_user_name,
                   external_metadata, anon_token_hash, body, kind, accepted_at
            FROM feedback
            WHERE project_id = $1 AND tenant_id = $2
            ORDER BY accepted_at DESC
            LIMIT $3
            "#,
            scope.project_id(),
            scope.tenant_id(),
            limit,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| Feedback {
                id: r.id,
                short_code: FeedbackId::from(r.short_code),
                project_id: r.project_id,
                tenant_id: r.tenant_id,
                end_user_sub: r.end_user_sub,
                end_user_email: r.end_user_email,
                end_user_name: r.end_user_name,
                external_metadata: r.external_metadata,
                anon_token_hash: r.anon_token_hash,
                body: r.body,
                kind: FeedbackKind::from_db_str(&r.kind),
                accepted_at: r.accepted_at,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::{ProjectRepo, SqlxProjectRepo};
    use crate::tenants::{SqlxTenantRepo, TenantRepo};
    use serde_json::json;
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
    async fn submit_authenticated_round_trips(pool: PgPool) {
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "auth@example.com").await;

        let meta = json!({"user_id": "u-1", "plan": "pro"});
        let id = repo
            .submit_authenticated(
                &scope,
                "auth0|sub-123",
                Some("u@example.com"),
                Some("Alice"),
                Some(&meta),
                "It crashed when I clicked save",
                FeedbackKind::Bug,
            )
            .await
            .unwrap();
        assert!(id.as_str().starts_with("FB-"));

        let recent = repo.list_recent(&scope, 10).await.unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].short_code, id);
        assert_eq!(recent[0].kind, FeedbackKind::Bug);
        assert_eq!(recent[0].end_user_sub.as_deref(), Some("auth0|sub-123"));
        assert!(recent[0].anon_token_hash.is_none());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn submit_anonymous_round_trips_and_tracks_dedup(pool: PgPool) {
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "anon@example.com").await;

        let token = [9u8; 32];
        let id1 = repo
            .submit_anonymous(&scope, &token, None, "First note", FeedbackKind::Other)
            .await
            .unwrap();
        let id2 = repo
            .submit_anonymous(&scope, &token, Some("opt@in.com"), "Second", FeedbackKind::Feature)
            .await
            .unwrap();
        assert_ne!(id1.as_str(), id2.as_str());

        let recent = repo.list_recent(&scope, 10).await.unwrap();
        assert_eq!(recent.len(), 2);
        assert!(recent.iter().all(|f| f.end_user_sub.is_none()));
        assert!(recent.iter().all(|f| f.anon_token_hash.as_deref() == Some(token.as_slice())));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn list_recent_only_returns_scope_owner_rows(pool: PgPool) {
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let s1 = seed_project_scope(&pool, "owner1@example.com").await;
        let s2 = seed_project_scope(&pool, "owner2@example.com").await;

        repo.submit_anonymous(&s1, &[1u8; 32], None, "from s1", FeedbackKind::Other).await.unwrap();
        repo.submit_anonymous(&s2, &[2u8; 32], None, "from s2-a", FeedbackKind::Other).await.unwrap();
        repo.submit_anonymous(&s2, &[3u8; 32], None, "from s2-b", FeedbackKind::Other).await.unwrap();

        let s1_rows = repo.list_recent(&s1, 10).await.unwrap();
        let s2_rows = repo.list_recent(&s2, 10).await.unwrap();
        assert_eq!(s1_rows.len(), 1);
        assert_eq!(s2_rows.len(), 2);

        // Cross-tenant invariant: s1's rows do not appear in s2's list and vice versa.
        let s1_bodies: Vec<&str> = s1_rows.iter().map(|f| f.body.as_str()).collect();
        let s2_bodies: Vec<&str> = s2_rows.iter().map(|f| f.body.as_str()).collect();
        assert!(s1_bodies.contains(&"from s1"));
        assert!(!s1_bodies.iter().any(|b| b.starts_with("from s2")));
        assert!(s2_bodies.iter().all(|b| b.starts_with("from s2")));
    }
}
