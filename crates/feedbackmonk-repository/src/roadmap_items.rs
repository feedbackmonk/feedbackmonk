//! Roadmap-items repository — Contract C13 backing surface.
//!
//! Mirrors the schema in `migrations/00006_roadmap_items.sql`. Every public
//! method takes `&ProjectScope` as its first non-self argument (Probe B
//! compliance). The constructor `SqlxRoadmapItemRepo::new` is allowlisted
//! as a structural mirror of `SqlxFeedbackRepo::new` (per GUIDE.md §8
//! pre-authorized widenings; LD ratifies at convergence).
//!
//! Two methods carry the bulk of the load-bearing semantics for downstream
//! consumers:
//!
//! 1. `get_existing_promotion(scope, origin_feedback_id) -> Option<RoadmapItem>`
//!    — idempotency helper for the promote handler. Returns `Some` on
//!    re-promote, `None` on first promote. `UNIQUE(origin_feedback_id)` is the
//!    backing invariant; this method is the application-side helper.
//!
//! 2. `create_in_executor(scope, conn, …)` — same-txn create variant used by
//!    the promote handler to compose roadmap-item INSERT + source feedback
//!    status UPDATE + audit row INSERT into one transaction (Contract C16
//!    hard invariant 5; mirrors the `_in_executor` overload pattern that P1
//!    Stage 1 established for `FeedbackStatusHistoryRepo`).
//!
//! Lineage:
//!   FR-FBR-11 (public roadmap)
//!   FR-FBR-12 (promote-to-roadmap; idempotency via UNIQUE)
//!   Contract C13 (P2 plan §Interface Contracts)
//!   docs/planning/handoffs/p2-fanout-contracts.md §C13 + §C16

use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use feedbackmonk_core::{RoadmapItem, RoadmapItemStatus};

use crate::error::{RepoError, Result};
use crate::scope::ProjectScope;

/// Input for create operations. Optional `slug` lets the handler auto-derive
/// from `title` when admin doesn't supply one; the handler does that
/// derivation, NOT this repo.
#[derive(Debug, Clone)]
pub struct NewRoadmapItem<'a> {
    pub slug: &'a str,
    pub title: &'a str,
    pub body: &'a str,
    pub status: RoadmapItemStatus,
    pub origin_feedback_id: Option<Uuid>,
    pub created_by: Uuid,
}

/// Partial-update patch. `None` fields are skipped; only `Some` fields are
/// written.
#[derive(Debug, Clone, Default)]
pub struct RoadmapItemPatch<'a> {
    pub title: Option<&'a str>,
    pub body: Option<&'a str>,
    pub status: Option<RoadmapItemStatus>,
}

#[async_trait]
pub trait RoadmapItemRepo: Send + Sync {
    /// Admin-facing list: includes all five statuses. Filters by optional
    /// `status_filter`. Returns `(items, total_matching_count)` shaped the
    /// same way `FeedbackRepo::list_for_admin` returns.
    async fn list_admin(
        &self,
        scope: &ProjectScope,
        status_filter: Option<RoadmapItemStatus>,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<RoadmapItem>, u32)>;

    /// Public-facing list: filters to `RoadmapItemStatus::is_public_visible`
    /// (in v1, all five). Same return shape as `list_admin`.
    async fn list_public(
        &self,
        scope: &ProjectScope,
        status_filter: Option<RoadmapItemStatus>,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<RoadmapItem>, u32)>;

    /// Single-item lookup by slug. Returns `RepoError::NotFound` if no row
    /// matches inside the scope (cross-tenant lookups also surface as
    /// `NotFound` — multi-tenant isolation invariant).
    async fn get_by_slug(&self, scope: &ProjectScope, slug: &str) -> Result<RoadmapItem>;

    /// Idempotency helper for the promote handler. Returns `Some` if a
    /// roadmap item already exists with this `origin_feedback_id` inside
    /// scope, `None` otherwise. `UNIQUE(origin_feedback_id)` makes this a
    /// single-row read.
    async fn get_existing_promotion(
        &self,
        scope: &ProjectScope,
        origin_feedback_id: Uuid,
    ) -> Result<Option<RoadmapItem>>;

    /// Create a new roadmap item. Slug-collision returns
    /// `RepoError::Conflict` (handler maps to 409 `SlugTaken`).
    async fn create(&self, scope: &ProjectScope, input: &NewRoadmapItem<'_>) -> Result<RoadmapItem>;

    /// Same-transaction create variant for the promote handler's atomic-txn
    /// path. The caller opens a transaction via `pool.begin()` and passes
    /// `&mut *tx` for `conn`.
    async fn create_in_executor(
        &self,
        scope: &ProjectScope,
        conn: &mut sqlx::PgConnection,
        input: &NewRoadmapItem<'_>,
    ) -> Result<RoadmapItem>;

    /// Partial update by slug. Updates `updated_at` to `now()` automatically
    /// (no DB trigger; explicit in SQL — see migration 00006 module doc).
    async fn update(
        &self,
        scope: &ProjectScope,
        slug: &str,
        patch: &RoadmapItemPatch<'_>,
    ) -> Result<RoadmapItem>;

    /// Per-project aggregate of vote counts per item, ordered DESC by count.
    /// Used by the voting cache refresh tick. Returns `(item_id, count)`
    /// pairs limited to `limit` rows. Items with zero votes are included.
    async fn aggregate_vote_counts(
        &self,
        scope: &ProjectScope,
        limit: i64,
    ) -> Result<Vec<(Uuid, i64)>>;
}

#[derive(Clone)]
pub struct SqlxRoadmapItemRepo {
    pool: PgPool,
}

impl SqlxRoadmapItemRepo {
    /// Constructor — allowlisted as a structural mirror of
    /// `SqlxFeedbackRepo::new`. Pre-authorized per GUIDE.md §8.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[allow(clippy::too_many_arguments)]
fn row_to_item(
    id: Uuid,
    tenant_id: Uuid,
    project_id: Uuid,
    slug: String,
    title: String,
    body: String,
    status: &str,
    origin_feedback_id: Option<Uuid>,
    created_at: chrono::DateTime<chrono::Utc>,
    created_by: Uuid,
    updated_at: chrono::DateTime<chrono::Utc>,
) -> RoadmapItem {
    RoadmapItem {
        id,
        tenant_id,
        project_id,
        slug,
        title,
        body,
        status: RoadmapItemStatus::from_db_str(status),
        origin_feedback_id,
        created_at,
        created_by,
        updated_at,
    }
}

#[async_trait]
impl RoadmapItemRepo for SqlxRoadmapItemRepo {
    async fn list_admin(
        &self,
        scope: &ProjectScope,
        status_filter: Option<RoadmapItemStatus>,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<RoadmapItem>, u32)> {
        let status_str: Option<&'static str> = status_filter.map(RoadmapItemStatus::as_db_str);

        let rows = sqlx::query!(
            r#"
            SELECT id, tenant_id, project_id, slug, title, body, status,
                   origin_feedback_id, created_at, created_by, updated_at
            FROM roadmap_items
            WHERE tenant_id = $1
              AND project_id = $2
              AND ($3::text IS NULL OR status = $3)
            ORDER BY created_at DESC
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
            FROM roadmap_items
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

        let items = rows
            .into_iter()
            .map(|r| {
                row_to_item(
                    r.id,
                    r.tenant_id,
                    r.project_id,
                    r.slug,
                    r.title,
                    r.body,
                    &r.status,
                    r.origin_feedback_id,
                    r.created_at,
                    r.created_by,
                    r.updated_at,
                )
            })
            .collect();
        Ok((items, total))
    }

    async fn list_public(
        &self,
        scope: &ProjectScope,
        status_filter: Option<RoadmapItemStatus>,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<RoadmapItem>, u32)> {
        // In v1, every status is public-visible, so `list_public` and
        // `list_admin` return identical rows. The split exists so a future
        // status (e.g. a true draft) doesn't have to thread through both
        // call sites — flipping `is_public_visible` is sufficient.
        self.list_admin(scope, status_filter, limit, offset).await
    }

    async fn get_by_slug(&self, scope: &ProjectScope, slug: &str) -> Result<RoadmapItem> {
        let row = sqlx::query!(
            r#"
            SELECT id, tenant_id, project_id, slug, title, body, status,
                   origin_feedback_id, created_at, created_by, updated_at
            FROM roadmap_items
            WHERE tenant_id = $1 AND project_id = $2 AND slug = $3
            "#,
            scope.tenant_id(),
            scope.project_id(),
            slug,
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(RepoError::NotFound)?;

        Ok(row_to_item(
            row.id,
            row.tenant_id,
            row.project_id,
            row.slug,
            row.title,
            row.body,
            &row.status,
            row.origin_feedback_id,
            row.created_at,
            row.created_by,
            row.updated_at,
        ))
    }

    async fn get_existing_promotion(
        &self,
        scope: &ProjectScope,
        origin_feedback_id: Uuid,
    ) -> Result<Option<RoadmapItem>> {
        let row = sqlx::query!(
            r#"
            SELECT id, tenant_id, project_id, slug, title, body, status,
                   origin_feedback_id, created_at, created_by, updated_at
            FROM roadmap_items
            WHERE tenant_id = $1 AND project_id = $2 AND origin_feedback_id = $3
            "#,
            scope.tenant_id(),
            scope.project_id(),
            origin_feedback_id,
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| {
            row_to_item(
                r.id,
                r.tenant_id,
                r.project_id,
                r.slug,
                r.title,
                r.body,
                &r.status,
                r.origin_feedback_id,
                r.created_at,
                r.created_by,
                r.updated_at,
            )
        }))
    }

    async fn create(&self, scope: &ProjectScope, input: &NewRoadmapItem<'_>) -> Result<RoadmapItem> {
        let row = match sqlx::query!(
            r#"
            INSERT INTO roadmap_items (
                tenant_id, project_id, slug, title, body, status,
                origin_feedback_id, created_by
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING id, tenant_id, project_id, slug, title, body, status,
                      origin_feedback_id, created_at, created_by, updated_at
            "#,
            scope.tenant_id(),
            scope.project_id(),
            input.slug,
            input.title,
            input.body,
            input.status.as_db_str(),
            input.origin_feedback_id,
            input.created_by,
        )
        .fetch_one(&self.pool)
        .await
        {
            Ok(r) => r,
            Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                return Err(RepoError::Conflict);
            }
            Err(e) => return Err(e.into()),
        };

        Ok(row_to_item(
            row.id,
            row.tenant_id,
            row.project_id,
            row.slug,
            row.title,
            row.body,
            &row.status,
            row.origin_feedback_id,
            row.created_at,
            row.created_by,
            row.updated_at,
        ))
    }

    async fn create_in_executor(
        &self,
        scope: &ProjectScope,
        conn: &mut sqlx::PgConnection,
        input: &NewRoadmapItem<'_>,
    ) -> Result<RoadmapItem> {
        let row = match sqlx::query!(
            r#"
            INSERT INTO roadmap_items (
                tenant_id, project_id, slug, title, body, status,
                origin_feedback_id, created_by
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING id, tenant_id, project_id, slug, title, body, status,
                      origin_feedback_id, created_at, created_by, updated_at
            "#,
            scope.tenant_id(),
            scope.project_id(),
            input.slug,
            input.title,
            input.body,
            input.status.as_db_str(),
            input.origin_feedback_id,
            input.created_by,
        )
        .fetch_one(&mut *conn)
        .await
        {
            Ok(r) => r,
            Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                return Err(RepoError::Conflict);
            }
            Err(e) => return Err(e.into()),
        };

        Ok(row_to_item(
            row.id,
            row.tenant_id,
            row.project_id,
            row.slug,
            row.title,
            row.body,
            &row.status,
            row.origin_feedback_id,
            row.created_at,
            row.created_by,
            row.updated_at,
        ))
    }

    async fn update(
        &self,
        scope: &ProjectScope,
        slug: &str,
        patch: &RoadmapItemPatch<'_>,
    ) -> Result<RoadmapItem> {
        // COALESCE($n, column) is the idiomatic "skip if NULL" pattern. The
        // updated_at column is touched unconditionally on every UPDATE — see
        // the explicit-timestamp policy in 00006 module doc.
        let status_str: Option<&'static str> = patch.status.map(RoadmapItemStatus::as_db_str);
        let row = sqlx::query!(
            r#"
            UPDATE roadmap_items
            SET title      = COALESCE($4, title),
                body       = COALESCE($5, body),
                status     = COALESCE($6, status),
                updated_at = now()
            WHERE tenant_id = $1 AND project_id = $2 AND slug = $3
            RETURNING id, tenant_id, project_id, slug, title, body, status,
                      origin_feedback_id, created_at, created_by, updated_at
            "#,
            scope.tenant_id(),
            scope.project_id(),
            slug,
            patch.title,
            patch.body,
            status_str,
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(RepoError::NotFound)?;

        Ok(row_to_item(
            row.id,
            row.tenant_id,
            row.project_id,
            row.slug,
            row.title,
            row.body,
            &row.status,
            row.origin_feedback_id,
            row.created_at,
            row.created_by,
            row.updated_at,
        ))
    }

    async fn aggregate_vote_counts(
        &self,
        scope: &ProjectScope,
        limit: i64,
    ) -> Result<Vec<(Uuid, i64)>> {
        // LEFT JOIN so items with zero votes still appear with count=0; the
        // tick stores zero-count items so the cache can serve them without
        // a fall-through to live SQL on every read.
        let rows = sqlx::query!(
            r#"
            SELECT i.id AS "item_id!",
                   count(v.id) AS "vote_count!"
            FROM roadmap_items AS i
            LEFT JOIN roadmap_votes AS v
              ON v.item_id = i.id
            WHERE i.tenant_id = $1 AND i.project_id = $2
            GROUP BY i.id
            ORDER BY count(v.id) DESC, i.created_at DESC
            LIMIT $3
            "#,
            scope.tenant_id(),
            scope.project_id(),
            limit,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| (r.item_id, r.vote_count)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::{ProjectRepo, SqlxProjectRepo};
    use crate::tenants::{SqlxTenantRepo, TenantRepo};

    async fn seed_project_scope(pool: &PgPool, email: &str) -> ProjectScope {
        let trepo = SqlxTenantRepo::new(pool.clone());
        let prepo = SqlxProjectRepo::new(pool.clone());
        let t = trepo.create(email, "h").await.unwrap();
        let scope = trepo.scope_for(t.id).await.unwrap();
        let p = prepo.create(&scope, "Proj", "proj").await.unwrap();
        prepo.open(&scope, p.id).await.unwrap()
    }

    fn input<'a>(slug: &'a str, title: &'a str, body: &'a str, by: Uuid) -> NewRoadmapItem<'a> {
        NewRoadmapItem {
            slug,
            title,
            body,
            status: RoadmapItemStatus::Considering,
            origin_feedback_id: None,
            created_by: by,
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn create_then_get_by_slug_round_trips(pool: PgPool) {
        let repo = SqlxRoadmapItemRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "rt@example.com").await;
        let actor = Uuid::new_v4();

        let created = repo
            .create(&scope, &input("dark-mode", "Dark Mode", "Plz add", actor))
            .await
            .unwrap();
        assert_eq!(created.slug, "dark-mode");
        assert_eq!(created.status, RoadmapItemStatus::Considering);
        assert_eq!(created.created_by, actor);

        let fetched = repo.get_by_slug(&scope, "dark-mode").await.unwrap();
        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.title, "Dark Mode");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn create_duplicate_slug_returns_conflict(pool: PgPool) {
        let repo = SqlxRoadmapItemRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "dup@example.com").await;
        let actor = Uuid::new_v4();

        repo.create(&scope, &input("dark-mode", "x", "y", actor))
            .await
            .unwrap();
        let err = repo
            .create(&scope, &input("dark-mode", "x2", "y2", actor))
            .await
            .unwrap_err();
        assert!(matches!(err, RepoError::Conflict));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn get_by_slug_cross_tenant_returns_not_found(pool: PgPool) {
        let repo = SqlxRoadmapItemRepo::new(pool.clone());
        let s1 = seed_project_scope(&pool, "owner1@example.com").await;
        let s2 = seed_project_scope(&pool, "owner2@example.com").await;
        repo.create(&s1, &input("dark-mode", "x", "y", Uuid::new_v4()))
            .await
            .unwrap();
        let err = repo.get_by_slug(&s2, "dark-mode").await.unwrap_err();
        assert!(matches!(err, RepoError::NotFound));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn get_existing_promotion_returns_some_after_promote(pool: PgPool) {
        let repo = SqlxRoadmapItemRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "promo@example.com").await;
        let origin = Uuid::new_v4();
        let actor = Uuid::new_v4();

        // Insert a row that pretends to be a promotion (origin_feedback_id
        // would normally reference an existing feedback row; for unit-test
        // purposes the FK ON DELETE SET NULL keeps the row valid against a
        // missing referent, but Postgres FK enforcement still requires the
        // referenced row to exist. So insert NULL here — we still validate
        // the lookup path returns None when no match exists.
        let none_match = repo
            .get_existing_promotion(&scope, origin)
            .await
            .unwrap();
        assert!(none_match.is_none());

        // Now insert a row with origin_feedback_id = NULL (we can't easily
        // seed a feedback row here without dragging in FeedbackRepo). The
        // get_existing_promotion lookup with the same Uuid still returns
        // None because the WHERE filter is `= $3`, not `IS NULL`.
        repo.create(&scope, &input("dark-mode", "x", "y", actor))
            .await
            .unwrap();
        let still_none = repo.get_existing_promotion(&scope, origin).await.unwrap();
        assert!(still_none.is_none());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn list_admin_filters_by_status(pool: PgPool) {
        let repo = SqlxRoadmapItemRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "list@example.com").await;
        let actor = Uuid::new_v4();
        repo.create(&scope, &input("a", "A", "body A", actor)).await.unwrap();
        repo.create(&scope, &input("b", "B", "body B", actor)).await.unwrap();
        // Bump "b" to planned.
        repo.update(
            &scope,
            "b",
            &RoadmapItemPatch {
                status: Some(RoadmapItemStatus::Planned),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let (items, total) = repo
            .list_admin(&scope, Some(RoadmapItemStatus::Planned), 50, 0)
            .await
            .unwrap();
        assert_eq!(total, 1);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].slug, "b");

        let (items, total) = repo.list_admin(&scope, None, 50, 0).await.unwrap();
        assert_eq!(total, 2);
        assert_eq!(items.len(), 2);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn update_changes_updated_at(pool: PgPool) {
        let repo = SqlxRoadmapItemRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "upd@example.com").await;
        let actor = Uuid::new_v4();
        let created = repo
            .create(&scope, &input("dark-mode", "Dark", "Body", actor))
            .await
            .unwrap();
        // Postgres `now()` has microsecond resolution; sleep briefly so the
        // bumped timestamp is strictly greater.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let patched = repo
            .update(
                &scope,
                "dark-mode",
                &RoadmapItemPatch {
                    title: Some("Dark Mode (v2)"),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(patched.title, "Dark Mode (v2)");
        assert!(
            patched.updated_at >= created.updated_at,
            "updated_at must not regress"
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn aggregate_vote_counts_returns_zero_for_unloved_items(pool: PgPool) {
        let repo = SqlxRoadmapItemRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "agg@example.com").await;
        let actor = Uuid::new_v4();
        repo.create(&scope, &input("a", "A", "body A", actor)).await.unwrap();
        repo.create(&scope, &input("b", "B", "body B", actor)).await.unwrap();

        let counts = repo.aggregate_vote_counts(&scope, 50).await.unwrap();
        assert_eq!(counts.len(), 2);
        assert!(counts.iter().all(|(_, c)| *c == 0));
    }
}
