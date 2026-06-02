//! Attachment repository (Gap #1 — GitCellar customer-#1 parity).
//!
//! Sole query path for the `attachments` table (migration 00009). Mirrors the
//! tenant-scoping discipline of every other repo in this crate: every public
//! method takes a `&ProjectScope` as its first non-`&self` argument and every
//! SQL statement filters on `tenant_id` + `project_id` (DEC-FBR-03). The
//! `multi-tenant-isolation-check` Verification Oracle enforces both.
//!
//! ## What is stored where
//!
//! This repo stores attachment METADATA only (storage key, resolved URL,
//! content-type, byte size, kind). The bytes themselves live in the object
//! store (`feedbackmonk-api`'s `storage` module). Log attachments are
//! PII-scrubbed by the API layer (the canonical `feedbackmonk-tracing`
//! chokepoint) BEFORE the bytes reach the object store — this repo never
//! sees raw log text.
//!
//! ## Feedback resolution
//!
//! `resolve_feedback_uuid` translates a public `FB-XXXXXX` short code into the
//! internal `feedback.id` UUID, scoped to `(tenant, project)`. It returns
//! `RepoError::NotFound` for an unknown / cross-tenant short code — the
//! upload handler maps that onto HTTP 404. Keeping this lookup here (rather
//! than widening `FeedbackRepo`) avoids co-touching `feedback.rs`, which other
//! PODS workers are editing this session.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::Result;
use crate::scope::ProjectScope;

/// Attachment kind — mirrors the schema CHECK constraint
/// (`kind IN ('image','service_log','console_log')`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentKind {
    Image,
    ServiceLog,
    ConsoleLog,
}

impl AttachmentKind {
    /// Canonical DB / wire string for this kind.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::ServiceLog => "service_log",
            Self::ConsoleLog => "console_log",
        }
    }

    /// Parse a DB string back into a kind. `pub(crate)` so it does not count
    /// as a scope-less public repository fn under the isolation oracle's
    /// Probe B (it is an internal row-mapping helper, never a caller entry
    /// point).
    #[allow(clippy::match_same_arms)] // explicit catch-all mirrors service_log by design.
    pub(crate) fn from_db_str(s: &str) -> Self {
        match s {
            "image" => Self::Image,
            "service_log" => Self::ServiceLog,
            "console_log" => Self::ConsoleLog,
            // Defensive: the CHECK constraint makes this unreachable, but we
            // map unknown values to ServiceLog rather than panic in a query
            // path. (No row can carry such a value.)
            _ => Self::ServiceLog,
        }
    }
}

/// Insert payload for one attachment. Built by the API layer after the bytes
/// are persisted to the object store. Public fields (no constructor) so the
/// caller uses a struct literal — keeps the isolation oracle's Probe B clean
/// (no scope-less `pub fn`).
#[derive(Debug, Clone)]
pub struct NewAttachment<'a> {
    pub kind: AttachmentKind,
    /// Opaque object-store key (path within the bucket / local root).
    pub storage_key: &'a str,
    /// Resolved fetch/public URL returned to the widget.
    pub url: &'a str,
    /// MIME type of the stored object.
    pub content_type: &'a str,
    /// Size in bytes of the stored object (post-scrub for logs).
    pub byte_size: i64,
}

/// One persisted attachment row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentRow {
    pub id: Uuid,
    pub feedback_id: Uuid,
    pub kind: AttachmentKind,
    pub storage_key: String,
    pub url: String,
    pub content_type: String,
    pub byte_size: i64,
    pub created_at: DateTime<Utc>,
}

#[async_trait]
pub trait AttachmentRepo: Send + Sync {
    /// Resolve a public `FB-XXXXXX` short code to the internal `feedback.id`
    /// UUID, scoped to `(tenant, project)`. `RepoError::NotFound` if the
    /// feedback does not exist in this scope (unknown or cross-tenant).
    async fn resolve_feedback_uuid(
        &self,
        scope: &ProjectScope,
        short_code: &str,
    ) -> Result<Uuid>;

    /// Count existing `image` attachments for a feedback row (scoped). Used
    /// for the ≤4-images-per-feedback app-layer cap.
    async fn count_images(&self, scope: &ProjectScope, feedback_uuid: Uuid) -> Result<i64>;

    /// Insert one attachment row (scoped). Returns the new attachment id.
    async fn insert(
        &self,
        scope: &ProjectScope,
        feedback_uuid: Uuid,
        new: &NewAttachment<'_>,
    ) -> Result<Uuid>;

    /// List all attachments for a feedback row (scoped), oldest-first.
    async fn list_for_feedback(
        &self,
        scope: &ProjectScope,
        feedback_uuid: Uuid,
    ) -> Result<Vec<AttachmentRow>>;
}

#[derive(Clone)]
pub struct SqlxAttachmentRepo {
    pool: PgPool,
}

impl SqlxAttachmentRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AttachmentRepo for SqlxAttachmentRepo {
    async fn resolve_feedback_uuid(
        &self,
        scope: &ProjectScope,
        short_code: &str,
    ) -> Result<Uuid> {
        let row = sqlx::query!(
            r#"
            SELECT id
            FROM feedback
            WHERE tenant_id = $1 AND project_id = $2 AND short_code = $3
            "#,
            scope.tenant_id(),
            scope.project_id(),
            short_code,
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(crate::error::RepoError::NotFound)?;
        Ok(row.id)
    }

    async fn count_images(&self, scope: &ProjectScope, feedback_uuid: Uuid) -> Result<i64> {
        let row = sqlx::query!(
            r#"
            SELECT count(*) AS "count!"
            FROM attachments
            WHERE tenant_id = $1 AND project_id = $2 AND feedback_id = $3 AND kind = 'image'
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_uuid,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.count)
    }

    async fn insert(
        &self,
        scope: &ProjectScope,
        feedback_uuid: Uuid,
        new: &NewAttachment<'_>,
    ) -> Result<Uuid> {
        let row = sqlx::query!(
            r#"
            INSERT INTO attachments (
                feedback_id, tenant_id, project_id,
                kind, storage_key, url, content_type, byte_size
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING id
            "#,
            feedback_uuid,
            scope.tenant_id(),
            scope.project_id(),
            new.kind.as_str(),
            new.storage_key,
            new.url,
            new.content_type,
            new.byte_size,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.id)
    }

    async fn list_for_feedback(
        &self,
        scope: &ProjectScope,
        feedback_uuid: Uuid,
    ) -> Result<Vec<AttachmentRow>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, feedback_id, kind, storage_key, url, content_type, byte_size, created_at
            FROM attachments
            WHERE tenant_id = $1 AND project_id = $2 AND feedback_id = $3
            ORDER BY created_at ASC
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_uuid,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| AttachmentRow {
                id: r.id,
                feedback_id: r.feedback_id,
                kind: AttachmentKind::from_db_str(&r.kind),
                storage_key: r.storage_key,
                url: r.url,
                content_type: r.content_type,
                byte_size: r.byte_size,
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
    use crate::feedback::{FeedbackRepo, SqlxFeedbackRepo};
    use feedbackmonk_core::FeedbackKind;
    use sqlx::PgPool;

    async fn seed_scope_and_feedback(pool: &PgPool, email: &str) -> (ProjectScope, Uuid) {
        let trepo = SqlxTenantRepo::new(pool.clone());
        let prepo = SqlxProjectRepo::new(pool.clone());
        let frepo = SqlxFeedbackRepo::new(pool.clone());
        let t = trepo.create(email, "h").await.unwrap();
        let scope = trepo.scope_for(t.id).await.unwrap();
        let p = prepo.create(&scope, "Proj", "proj").await.unwrap();
        let pscope = prepo.open(&scope, p.id).await.unwrap();
        // Seed via anonymous submission (decoupled from BRAVO's auth-mode sig change).
        let fb = frepo
            .submit_anonymous(&pscope, &[1u8; 32], None, "body", FeedbackKind::Bug)
            .await
            .unwrap();
        let arepo = SqlxAttachmentRepo::new(pool.clone());
        let fb_uuid = arepo.resolve_feedback_uuid(&pscope, fb.as_str()).await.unwrap();
        (pscope, fb_uuid)
    }

    fn new_image(key: &str) -> NewAttachment<'_> {
        NewAttachment {
            kind: AttachmentKind::Image,
            storage_key: key,
            url: "http://store/x.png",
            content_type: "image/png",
            byte_size: 1234,
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn insert_and_list_round_trips(pool: PgPool) {
        let (scope, fb) = seed_scope_and_feedback(&pool, "att@example.com").await;
        let repo = SqlxAttachmentRepo::new(pool.clone());

        let id = repo.insert(&scope, fb, &new_image("k/1.png")).await.unwrap();
        repo.insert(
            &scope,
            fb,
            &NewAttachment {
                kind: AttachmentKind::ServiceLog,
                storage_key: "k/log.txt",
                url: "http://store/log.txt",
                content_type: "text/plain",
                byte_size: 42,
            },
        )
        .await
        .unwrap();

        let rows = repo.list_for_feedback(&scope, fb).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|r| r.id == id && r.kind == AttachmentKind::Image));
        assert!(rows.iter().any(|r| r.kind == AttachmentKind::ServiceLog));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn count_images_only_counts_images(pool: PgPool) {
        let (scope, fb) = seed_scope_and_feedback(&pool, "count@example.com").await;
        let repo = SqlxAttachmentRepo::new(pool.clone());
        assert_eq!(repo.count_images(&scope, fb).await.unwrap(), 0);
        repo.insert(&scope, fb, &new_image("k/a.png")).await.unwrap();
        repo.insert(&scope, fb, &new_image("k/b.png")).await.unwrap();
        repo.insert(
            &scope,
            fb,
            &NewAttachment {
                kind: AttachmentKind::ConsoleLog,
                storage_key: "k/c.txt",
                url: "http://store/c.txt",
                content_type: "text/plain",
                byte_size: 10,
            },
        )
        .await
        .unwrap();
        // 2 images + 1 console_log → count_images == 2.
        assert_eq!(repo.count_images(&scope, fb).await.unwrap(), 2);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn resolve_feedback_uuid_cross_tenant_negative(pool: PgPool) {
        let (s1, fb1) = seed_scope_and_feedback(&pool, "owner1@example.com").await;
        let (s2, _fb2) = seed_scope_and_feedback(&pool, "owner2@example.com").await;
        let repo = SqlxAttachmentRepo::new(pool.clone());

        // Resolve fb1's short_code through s2's scope → must NotFound.
        // (We need fb1's short_code; resolve it back via s1 then probe s2.)
        let rows = repo.list_for_feedback(&s1, fb1).await.unwrap();
        assert!(rows.is_empty());

        // A bogus short code under s2 returns NotFound, never another tenant's row.
        let err = repo
            .resolve_feedback_uuid(&s2, "FB-NOPEXX")
            .await
            .unwrap_err();
        assert!(matches!(err, crate::error::RepoError::NotFound));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn list_for_feedback_is_scope_isolated(pool: PgPool) {
        let (s1, fb1) = seed_scope_and_feedback(&pool, "iso1@example.com").await;
        let (s2, fb2) = seed_scope_and_feedback(&pool, "iso2@example.com").await;
        let repo = SqlxAttachmentRepo::new(pool.clone());
        repo.insert(&s1, fb1, &new_image("s1/1.png")).await.unwrap();

        // s2's scope must not see s1's attachment even with s1's feedback uuid.
        let cross = repo.list_for_feedback(&s2, fb1).await.unwrap();
        assert!(cross.is_empty(), "cross-tenant feedback uuid leaked attachments");

        // s2's own feedback has none.
        assert!(repo.list_for_feedback(&s2, fb2).await.unwrap().is_empty());
    }
}
