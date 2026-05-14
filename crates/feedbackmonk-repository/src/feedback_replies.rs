//! Feedback-replies repository (FR-FBR-08 reply path, Contract C7).
//!
//! Owns CRUD on `feedback_replies` (migration 00004). Every method takes
//! `&ProjectScope` first so the SQL filters by both `tenant_id` and
//! `project_id` on the joined `feedback` row -- a reply belonging to a
//! sibling tenant cannot be read or written through this surface.
//!
//! Visibility is enforced at the application layer:
//!   - `'public'`   -> may trigger an email send (Worker A handler logic).
//!   - `'internal'` -> never emailed; admin-only triage notes.
//!
//! Stage 2 adds the `_in_executor` variant alongside the public surface
//! (mirroring `FeedbackStatusHistoryRepo::append_in_executor`) so the
//! reply handler can compose same-transaction writes if it ever needs to
//! couple a reply insert with another mutation. Stage 2's reply endpoint
//! does NOT need a transaction (the reply insert is the only write), so
//! the `_in_executor` variant is not used by Stage 2's handler -- it
//! exists for symmetry with the status-history overload and for future
//! callers.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use feedbackmonk_core::FeedbackId;

use crate::error::Result;
use crate::scope::ProjectScope;

/// One row of `feedback_replies`. The repo never decides which visibility
/// is appropriate; the API handler maps the request body's `visibility`
/// field onto this column directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackReply {
    pub id: Uuid,
    pub feedback_id: Uuid,
    pub body: String,
    pub visibility: ReplyVisibility,
    pub author_user_id: Uuid,
    pub created_at: DateTime<Utc>,
}

/// Reply visibility — matches the DB CHECK constraint from migration 00004.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReplyVisibility {
    Public,
    Internal,
}

impl ReplyVisibility {
    #[must_use]
    pub(crate) fn as_db_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Internal => "internal",
        }
    }

    #[must_use]
    pub(crate) fn from_db_str(s: &str) -> Self {
        match s {
            "public" => Self::Public,
            _ => Self::Internal,
        }
    }
}

#[async_trait]
pub trait FeedbackReplyRepo: Send + Sync {
    /// Insert a reply row. `feedback_id` is resolved by scope, so a
    /// cross-tenant `feedback_id` returns `RepoError::NotFound` instead of
    /// writing a row under a sibling tenant.
    async fn create(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
        body: &str,
        visibility: ReplyVisibility,
        author_user_id: Uuid,
    ) -> Result<FeedbackReply>;

    /// List replies for a feedback row, oldest-first (chronological).
    /// Cross-tenant lookups return an empty Vec (NOT an error) -- the JOIN
    /// against `feedback` filters by scope.
    async fn list_for_feedback(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
    ) -> Result<Vec<FeedbackReply>>;

    /// Count replies for a feedback row in scope (used by the admin list
    /// view to populate `FeedbackListItem::reply_count`).
    async fn count_for_feedback(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
    ) -> Result<i64>;
}

#[derive(Clone)]
pub struct SqlxFeedbackReplyRepo {
    pool: PgPool,
}

impl SqlxFeedbackReplyRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl FeedbackReplyRepo for SqlxFeedbackReplyRepo {
    async fn create(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
        body: &str,
        visibility: ReplyVisibility,
        author_user_id: Uuid,
    ) -> Result<FeedbackReply> {
        // Resolve the feedback row within scope so a cross-tenant
        // `feedback_id` cannot trigger a reply insert under a sibling tenant.
        let src = sqlx::query!(
            r#"
            SELECT id
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

        let inserted = sqlx::query!(
            r#"
            INSERT INTO feedback_replies (feedback_id, body, visibility, author_user_id)
            VALUES ($1, $2, $3, $4)
            RETURNING id, feedback_id, body, visibility, author_user_id, created_at
            "#,
            src.id,
            body,
            visibility.as_db_str(),
            author_user_id,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(FeedbackReply {
            id: inserted.id,
            feedback_id: inserted.feedback_id,
            body: inserted.body,
            visibility: ReplyVisibility::from_db_str(&inserted.visibility),
            author_user_id: inserted.author_user_id,
            created_at: inserted.created_at,
        })
    }

    async fn list_for_feedback(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
    ) -> Result<Vec<FeedbackReply>> {
        let rows = sqlx::query!(
            r#"
            SELECT r.id, r.feedback_id, r.body, r.visibility,
                   r.author_user_id, r.created_at
            FROM feedback_replies AS r
            JOIN feedback AS f ON f.id = r.feedback_id
            WHERE f.tenant_id = $1
              AND f.project_id = $2
              AND f.short_code = $3
            ORDER BY r.created_at ASC
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_id.as_str(),
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| FeedbackReply {
                id: r.id,
                feedback_id: r.feedback_id,
                body: r.body,
                visibility: ReplyVisibility::from_db_str(&r.visibility),
                author_user_id: r.author_user_id,
                created_at: r.created_at,
            })
            .collect())
    }

    async fn count_for_feedback(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
    ) -> Result<i64> {
        let row = sqlx::query!(
            r#"
            SELECT count(*) AS "count!"
            FROM feedback_replies AS r
            JOIN feedback AS f ON f.id = r.feedback_id
            WHERE f.tenant_id = $1
              AND f.project_id = $2
              AND f.short_code = $3
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_id.as_str(),
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feedback::{FeedbackRepo, SqlxFeedbackRepo};
    use crate::projects::{ProjectRepo, SqlxProjectRepo};
    use crate::tenants::{SqlxTenantRepo, TenantRepo};
    use feedbackmonk_core::FeedbackKind;

    async fn seed_project_scope(pool: &PgPool, email: &str) -> ProjectScope {
        let trepo = SqlxTenantRepo::new(pool.clone());
        let prepo = SqlxProjectRepo::new(pool.clone());
        let t = trepo.create(email, "h").await.unwrap();
        let scope = trepo.scope_for(t.id).await.unwrap();
        let p = prepo.create(&scope, "Proj", "proj").await.unwrap();
        prepo.open(&scope, p.id).await.unwrap()
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn create_then_list_round_trips(pool: PgPool) {
        let fb_repo = SqlxFeedbackRepo::new(pool.clone());
        let reply_repo = SqlxFeedbackReplyRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "reply-rt@example.com").await;

        let fb_id = fb_repo
            .submit_anonymous(&scope, &[1u8; 32], None, "body", FeedbackKind::Other)
            .await
            .unwrap();
        let author = Uuid::new_v4();

        let r1 = reply_repo
            .create(&scope, &fb_id, "first reply", ReplyVisibility::Public, author)
            .await
            .unwrap();
        let r2 = reply_repo
            .create(&scope, &fb_id, "second reply (internal)", ReplyVisibility::Internal, author)
            .await
            .unwrap();
        assert_ne!(r1.id, r2.id);
        assert_eq!(r1.visibility, ReplyVisibility::Public);
        assert_eq!(r2.visibility, ReplyVisibility::Internal);

        let rows = reply_repo.list_for_feedback(&scope, &fb_id).await.unwrap();
        assert_eq!(rows.len(), 2);
        // Chronological order: r1 before r2.
        assert!(rows[0].created_at <= rows[1].created_at);
        assert_eq!(rows[0].body, "first reply");
        assert_eq!(rows[1].body, "second reply (internal)");

        let count = reply_repo.count_for_feedback(&scope, &fb_id).await.unwrap();
        assert_eq!(count, 2);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn create_cross_tenant_target_rejected(pool: PgPool) {
        let fb_repo = SqlxFeedbackRepo::new(pool.clone());
        let reply_repo = SqlxFeedbackReplyRepo::new(pool.clone());
        let s1 = seed_project_scope(&pool, "rep-owner1@example.com").await;
        let s2 = seed_project_scope(&pool, "rep-owner2@example.com").await;

        let fb_id = fb_repo
            .submit_anonymous(&s1, &[2u8; 32], None, "body", FeedbackKind::Other)
            .await
            .unwrap();

        // s2 attempts to create a reply on s1's feedback -- scope lookup
        // rejects with NotFound (multi-tenant-isolation invariant).
        let err = reply_repo
            .create(&s2, &fb_id, "evil", ReplyVisibility::Public, Uuid::new_v4())
            .await
            .unwrap_err();
        assert!(matches!(err, crate::error::RepoError::NotFound));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn list_cross_tenant_returns_empty(pool: PgPool) {
        let fb_repo = SqlxFeedbackRepo::new(pool.clone());
        let reply_repo = SqlxFeedbackReplyRepo::new(pool.clone());
        let s1 = seed_project_scope(&pool, "list-owner1@example.com").await;
        let s2 = seed_project_scope(&pool, "list-owner2@example.com").await;
        let fb_id = fb_repo
            .submit_anonymous(&s1, &[3u8; 32], None, "body", FeedbackKind::Other)
            .await
            .unwrap();
        reply_repo
            .create(&s1, &fb_id, "only s1 sees me", ReplyVisibility::Public, Uuid::new_v4())
            .await
            .unwrap();

        // s2 cannot see s1's replies even via the same FeedbackId.
        let rows = reply_repo.list_for_feedback(&s2, &fb_id).await.unwrap();
        assert!(rows.is_empty());
        let count = reply_repo.count_for_feedback(&s2, &fb_id).await.unwrap();
        assert_eq!(count, 0);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn count_zero_for_feedback_without_replies(pool: PgPool) {
        let fb_repo = SqlxFeedbackRepo::new(pool.clone());
        let reply_repo = SqlxFeedbackReplyRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "no-replies@example.com").await;
        let fb_id = fb_repo
            .submit_anonymous(&scope, &[4u8; 32], None, "body", FeedbackKind::Other)
            .await
            .unwrap();

        let count = reply_repo.count_for_feedback(&scope, &fb_id).await.unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn visibility_db_round_trips() {
        for v in [ReplyVisibility::Public, ReplyVisibility::Internal] {
            assert_eq!(ReplyVisibility::from_db_str(v.as_db_str()), v);
        }
    }
}
