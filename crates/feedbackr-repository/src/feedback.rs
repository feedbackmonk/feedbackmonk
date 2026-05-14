//! Feedback repository (FR-FBR-01, Contract C1).
//!
//! Two submission methods, mirroring the auth-mode/anonymous-mode split in
//! Contract C3. The schema enforces the XOR invariant via a CHECK constraint
//! (exactly one of `end_user_sub` / `anon_token_hash` is non-NULL).

use async_trait::async_trait;
use serde_json::Value as JsonValue;
use sqlx::PgPool;

use feedbackr_core::{Feedback, FeedbackId, FeedbackKind, FeedbackStatus};

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

    /// Admin listing — paged + status-filtered (Contract C6 backing method).
    /// Returns `(items, total_matching_count)`. `total` reflects the row count
    /// matching the optional status filter, NOT the page slice size.
    async fn list_for_admin(
        &self,
        scope: &ProjectScope,
        status_filter: Option<FeedbackStatus>,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<FeedbackListItem>, u32)>;

    /// Single-feedback view used by the admin drawer (Contract C8). Pairs
    /// the full feedback row with its complete status history newest-first.
    /// Cross-tenant lookups return `NotFound` rather than an error — Stage 2
    /// Worker A maps the `Result` onto HTTP 404 vs 500.
    async fn get_with_history(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
    ) -> Result<(Feedback, Vec<StatusHistoryRow>)>;
}

/// Trimmed list item — the columns the admin list page renders, plus the
/// `reply_count` that Stage 2 Worker A's `/admin/feedback` endpoint exposes
/// in its JSON shape (Contract C8). `reply_count` is hard-zero in Stage 1
/// because the `feedback_replies` table doesn't exist yet (Stage 2 Worker A
/// migration 00004 adds it). Worker A widens this SQL when the table lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackListItem {
    pub feedback_id: FeedbackId,
    pub kind: FeedbackKind,
    pub status: FeedbackStatus,
    /// First 200 chars of the body. The admin UI fetches the full body via
    /// `get_with_history` when the user opens the drawer.
    pub body_excerpt: String,
    pub submitted_at: chrono::DateTime<chrono::Utc>,
    /// Hash of (auth-mode email | anon mode marker). Worker A's HTTP layer
    /// turns this into a display string like `"alice@example.com"` or
    /// `"anonymous"`.
    pub submitter_email: Option<String>,
    pub is_anonymous: bool,
    /// Stage 1 always zero. Stage 2 Worker A wires this to
    /// `feedback_replies` once that table exists.
    pub reply_count: i64,
}

/// One row of `feedback_status_history`. Stage 2 Worker A's HTTP layer
/// joins `transitioned_by` against the future `tenant_users` table to
/// derive a human-readable label; Stage 1 returns the raw UUID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusHistoryRow {
    pub id: uuid::Uuid,
    pub feedback_id: uuid::Uuid,
    pub from_status: FeedbackStatus,
    pub to_status: FeedbackStatus,
    pub reason_note: Option<String>,
    pub duplicate_of_feedback_id: Option<uuid::Uuid>,
    pub transitioned_by: uuid::Uuid,
    pub transitioned_at: chrono::DateTime<chrono::Utc>,
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
                   external_metadata, anon_token_hash, body, kind, accepted_at, status
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
                status: FeedbackStatus::from_db_str(&r.status),
            })
            .collect())
    }

    async fn list_for_admin(
        &self,
        scope: &ProjectScope,
        status_filter: Option<FeedbackStatus>,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<FeedbackListItem>, u32)> {
        // `Option<&str>` lets sqlx bind a nullable parameter; the WHERE
        // clause's `$3::text IS NULL OR status = $3` collapses to "no filter"
        // when the caller passes `None`.
        let status_str: Option<&'static str> = status_filter.map(FeedbackStatus::as_db_str);

        let items = sqlx::query!(
            r#"
            SELECT short_code,
                   kind,
                   status,
                   left(body, 200) AS body_excerpt,
                   end_user_email,
                   anon_token_hash IS NOT NULL AS is_anonymous,
                   accepted_at
            FROM feedback
            WHERE tenant_id = $1
              AND project_id = $2
              AND ($3::text IS NULL OR status = $3)
            ORDER BY accepted_at DESC
            LIMIT $4
            OFFSET $5
            "#,
            scope.tenant_id(),
            scope.project_id(),
            status_str,
            i64::from(limit),
            i64::from(offset),
        )
        .fetch_all(&self.pool)
        .await?;

        let total_row = sqlx::query!(
            r#"
            SELECT count(*) AS "count!"
            FROM feedback
            WHERE tenant_id = $1
              AND project_id = $2
              AND ($3::text IS NULL OR status = $3)
            "#,
            scope.tenant_id(),
            scope.project_id(),
            status_str,
        )
        .fetch_one(&self.pool)
        .await?;
        let total: u32 = total_row.count.try_into().unwrap_or(u32::MAX);

        let list = items
            .into_iter()
            .map(|r| FeedbackListItem {
                feedback_id: FeedbackId::from(r.short_code),
                kind: FeedbackKind::from_db_str(&r.kind),
                status: FeedbackStatus::from_db_str(&r.status),
                body_excerpt: r.body_excerpt.unwrap_or_default(),
                submitted_at: r.accepted_at,
                submitter_email: r.end_user_email,
                is_anonymous: r.is_anonymous.unwrap_or(false),
                // Stage 2 Worker A's migration 00004 (feedback_replies) and
                // their handler widening surface the real count. Stage 1
                // returns hard-zero per the brief's scope discipline.
                reply_count: 0,
            })
            .collect();

        Ok((list, total))
    }

    async fn get_with_history(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
    ) -> Result<(Feedback, Vec<StatusHistoryRow>)> {
        let row = sqlx::query!(
            r#"
            SELECT id, short_code, project_id, tenant_id,
                   end_user_sub, end_user_email, end_user_name,
                   external_metadata, anon_token_hash, body, kind, accepted_at, status
            FROM feedback
            WHERE tenant_id = $1 AND project_id = $2 AND short_code = $3
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_id.as_str(),
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(crate::error::RepoError::NotFound)?;

        let feedback = Feedback {
            id: row.id,
            short_code: FeedbackId::from(row.short_code),
            project_id: row.project_id,
            tenant_id: row.tenant_id,
            end_user_sub: row.end_user_sub,
            end_user_email: row.end_user_email,
            end_user_name: row.end_user_name,
            external_metadata: row.external_metadata,
            anon_token_hash: row.anon_token_hash,
            body: row.body,
            kind: FeedbackKind::from_db_str(&row.kind),
            accepted_at: row.accepted_at,
            status: FeedbackStatus::from_db_str(&row.status),
        };

        let history_rows = sqlx::query!(
            r#"
            SELECT id, feedback_id, from_status, to_status, reason_note,
                   duplicate_of_feedback_id, transitioned_by, transitioned_at
            FROM feedback_status_history
            WHERE feedback_id = $1
            ORDER BY transitioned_at DESC
            "#,
            feedback.id,
        )
        .fetch_all(&self.pool)
        .await?;

        let history = history_rows
            .into_iter()
            .map(|r| StatusHistoryRow {
                id: r.id,
                feedback_id: r.feedback_id,
                from_status: FeedbackStatus::from_db_str(&r.from_status),
                to_status: FeedbackStatus::from_db_str(&r.to_status),
                reason_note: r.reason_note,
                duplicate_of_feedback_id: r.duplicate_of_feedback_id,
                transitioned_by: r.transitioned_by,
                transitioned_at: r.transitioned_at,
            })
            .collect();

        Ok((feedback, history))
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
    async fn list_for_admin_returns_paged_results_with_total(pool: PgPool) {
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "admin@example.com").await;

        // Seed three submissions.
        for body in ["one", "two", "three"] {
            repo.submit_anonymous(&scope, &[7u8; 32], None, body, FeedbackKind::Other)
                .await
                .unwrap();
        }

        let (page, total) = repo.list_for_admin(&scope, None, 2, 0).await.unwrap();
        assert_eq!(page.len(), 2);
        assert_eq!(total, 3);

        let (page2, total2) = repo.list_for_admin(&scope, None, 2, 2).await.unwrap();
        assert_eq!(page2.len(), 1);
        assert_eq!(total2, 3);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn list_for_admin_cross_tenant_negative(pool: PgPool) {
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let s1 = seed_project_scope(&pool, "owner1-admin@example.com").await;
        let s2 = seed_project_scope(&pool, "owner2-admin@example.com").await;
        repo.submit_anonymous(&s1, &[1u8; 32], None, "from s1", FeedbackKind::Other)
            .await
            .unwrap();

        // Querying from s2's scope must return 0 rows for s1's feedback,
        // NOT an error. This is the multi-tenant-isolation invariant.
        let (page, total) = repo.list_for_admin(&s2, None, 10, 0).await.unwrap();
        assert!(page.is_empty());
        assert_eq!(total, 0);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn list_for_admin_status_filter_returns_matching_rows(pool: PgPool) {
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "status-filter@example.com").await;
        repo.submit_anonymous(&scope, &[5u8; 32], None, "row", FeedbackKind::Other)
            .await
            .unwrap();

        // New rows are 'submitted' by default; filtering by Triaged returns 0.
        let (page, total) = repo
            .list_for_admin(&scope, Some(FeedbackStatus::Triaged), 10, 0)
            .await
            .unwrap();
        assert_eq!(page.len(), 0);
        assert_eq!(total, 0);

        // Filtering by Submitted returns the row.
        let (page, total) = repo
            .list_for_admin(&scope, Some(FeedbackStatus::Submitted), 10, 0)
            .await
            .unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(total, 1);
        assert_eq!(page[0].status, FeedbackStatus::Submitted);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn get_with_history_returns_feedback_and_empty_history_initially(pool: PgPool) {
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "history@example.com").await;
        let id = repo
            .submit_anonymous(&scope, &[3u8; 32], None, "row body", FeedbackKind::Bug)
            .await
            .unwrap();

        let (fb, history) = repo.get_with_history(&scope, &id).await.unwrap();
        assert_eq!(fb.short_code, id);
        assert_eq!(fb.status, FeedbackStatus::Submitted);
        // No transitions yet (Stage 2 Worker A's handler writes these).
        assert!(history.is_empty());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn get_with_history_cross_tenant_negative(pool: PgPool) {
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let s1 = seed_project_scope(&pool, "owner1-history@example.com").await;
        let s2 = seed_project_scope(&pool, "owner2-history@example.com").await;
        let id = repo
            .submit_anonymous(&s1, &[6u8; 32], None, "cross-tenant target", FeedbackKind::Other)
            .await
            .unwrap();

        // Reading s1's feedback through s2's scope must NotFound, NOT error.
        let err = repo.get_with_history(&s2, &id).await.unwrap_err();
        assert!(matches!(err, crate::error::RepoError::NotFound));
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
