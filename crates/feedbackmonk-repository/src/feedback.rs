//! Feedback repository (FR-FBR-01, Contract C1).
//!
//! Two submission methods, mirroring the auth-mode/anonymous-mode split in
//! Contract C3. The schema enforces the XOR invariant via a CHECK constraint
//! (exactly one of `end_user_sub` / `anon_token_hash` is non-NULL).

use async_trait::async_trait;
use serde_json::Value as JsonValue;
use sqlx::PgPool;

use feedbackmonk_core::{Feedback, FeedbackId, FeedbackKind, FeedbackStatus};

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
        // crash_event_id — external crash-event correlation key (parity Gap #2;
        // migration 00010). A FIRST-CLASS column, deliberately NOT smuggled
        // through `external_metadata` (collaboration decisions.md). `None` when
        // not crash-linked. Persisted atomically in the same INSERT so a
        // crash-linked submit can never land without its link.
        crash_event_id: Option<&str>,
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

    /// Admin full-text search (GitCellar parity gap #3). Tenant + project
    /// scoped FTS over `feedback.body_tsv` (migration 00011) using
    /// `websearch_to_tsquery` for forgiving Google-style query syntax.
    /// Returns `(items, total_matching_count)` exactly like `list_for_admin`
    /// so the admin UI reuses the same row shape. Results are ordered by
    /// `ts_rank` (relevance) then `accepted_at DESC` as a stable tiebreak.
    ///
    /// A blank/whitespace `query` yields zero rows (the handler short-circuits
    /// before calling this, but the SQL is defensive: an empty
    /// `websearch_to_tsquery` matches nothing).
    async fn search_for_admin(
        &self,
        scope: &ProjectScope,
        query: &str,
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

    /// Same-transaction `feedback.status` UPDATE. Companion to
    /// `FeedbackStatusHistoryRepo::append_in_executor` for Contract C6
    /// Hard Invariant #4 -- the transition handler updates the status
    /// column and inserts the audit row inside one transaction; both
    /// roll back together on any failure.
    ///
    /// Pre-authorized widening per Stage 1->2 handoff doc:
    /// `self_mediated=true; ratification_pending=true;
    ///  matches_spec_at=docs/planning/handoffs/p1-stage1-to-stage2.md#pre-authorized`.
    ///
    /// Returns the previous status so the audit row's `from_status` field
    /// reflects the actual pre-write state (defends against TOCTOU between
    /// the handler's `get_with_history` read and this UPDATE).
    async fn update_status_in_executor(
        &self,
        scope: &ProjectScope,
        conn: &mut sqlx::PgConnection,
        feedback_id: &FeedbackId,
        new_status: FeedbackStatus,
    ) -> Result<FeedbackStatus>;

    // ==== Gap #4 (DELTA) — end-user (JWT-sub-scoped) read surface ===========
    // GitCellar customer-#1 parity gap #4. No schema change. These methods
    // back the public `/me/feedback` + `/me/feedback/:fb/thread` routes. They
    // return the NARROW `EndUserFeedback` projection (never the full
    // `Feedback` model) so the end-user surface cannot leak internal columns
    // (anon_token_hash, external_metadata, other users' email) and stays
    // decoupled from sibling-worker additions to the `Feedback` struct.

    /// List the CALLER'S OWN feedback, newest-first, paged. Filtered by
    /// `(tenant, project, end_user_sub)`. Anonymous rows (`end_user_sub IS
    /// NULL`) are structurally excluded by the `end_user_sub = $sub`
    /// predicate. Returns `(page, total_matching)`; `total` counts all of the
    /// caller's rows, not the page slice.
    async fn list_for_end_user(
        &self,
        scope: &ProjectScope,
        end_user_sub: &str,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<EndUserFeedback>, u32)>;

    /// Fetch ONE feedback row that belongs to the caller. Scoped by
    /// `(tenant, project, short_code, end_user_sub)` — a `short_code` that
    /// exists but belongs to a different `end_user_sub` (or is anonymous, or
    /// is in another tenant/project) returns `NotFound`, never another user's
    /// data. Backs the `/thread` endpoint's status header.
    async fn get_for_end_user(
        &self,
        scope: &ProjectScope,
        end_user_sub: &str,
        feedback_id: &FeedbackId,
    ) -> Result<EndUserFeedback>;
}

/// Narrow projection of a feedback row for the end-user (JWT) read surface
/// (Gap #4). Deliberately omits every internal/other-party column — the
/// end-user only ever sees their own submission's public-facing fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndUserFeedback {
    pub feedback_id: FeedbackId,
    pub kind: FeedbackKind,
    pub status: FeedbackStatus,
    pub body: String,
    pub submitted_at: chrono::DateTime<chrono::Utc>,
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
        crash_event_id: Option<&str>,
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
                external_metadata, crash_event_id, body, kind
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
            short_code.as_str(),
            scope.project_id(),
            scope.tenant_id(),
            end_user_sub,
            end_user_email,
            end_user_name,
            external_metadata,
            crash_event_id,
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
                   external_metadata, crash_event_id, anon_token_hash, body, kind, accepted_at, status
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
                crash_event_id: r.crash_event_id,
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

    async fn search_for_admin(
        &self,
        scope: &ProjectScope,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<FeedbackListItem>, u32)> {
        // Same `(tenant_id, project_id)` scope clause as every other feedback
        // read (DEC-FBR-03 sole-query-path). `websearch_to_tsquery` parses the
        // raw admin query forgivingly (quoted phrases, `-exclude`, `or`) and
        // never raises a parse error, so a malformed/blank query simply matches
        // nothing. Ordering: relevance first, then newest-first as a stable
        // tiebreak so equal-rank rows page deterministically.
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
              AND body_tsv @@ websearch_to_tsquery('english', $3)
            ORDER BY ts_rank(body_tsv, websearch_to_tsquery('english', $3)) DESC,
                     accepted_at DESC
            LIMIT $4
            OFFSET $5
            "#,
            scope.tenant_id(),
            scope.project_id(),
            query,
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
              AND body_tsv @@ websearch_to_tsquery('english', $3)
            "#,
            scope.tenant_id(),
            scope.project_id(),
            query,
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
                // Mirrors list_for_admin: the HTTP layer enriches the real
                // reply_count per row (the repository's search method stays a
                // pure feedback read).
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
                   external_metadata, crash_event_id, anon_token_hash, body, kind, accepted_at, status
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
            crash_event_id: row.crash_event_id,
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

    async fn update_status_in_executor(
        &self,
        scope: &ProjectScope,
        conn: &mut sqlx::PgConnection,
        feedback_id: &FeedbackId,
        new_status: FeedbackStatus,
    ) -> Result<FeedbackStatus> {
        // The UPDATE...RETURNING gives us back the row we just updated; we
        // need the PRE-update status, so we read it inside the same txn
        // BEFORE the write. Scope filter on both reads/writes ensures a
        // cross-tenant feedback_id cannot be touched.
        let pre = sqlx::query!(
            r#"
            SELECT status
            FROM feedback
            WHERE tenant_id = $1 AND project_id = $2 AND short_code = $3
            FOR UPDATE
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_id.as_str(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(crate::error::RepoError::NotFound)?;
        let from_status = FeedbackStatus::from_db_str(&pre.status);

        sqlx::query!(
            r#"
            UPDATE feedback
            SET status = $1
            WHERE tenant_id = $2 AND project_id = $3 AND short_code = $4
            "#,
            new_status.as_db_str(),
            scope.tenant_id(),
            scope.project_id(),
            feedback_id.as_str(),
        )
        .execute(&mut *conn)
        .await?;

        Ok(from_status)
    }

    // ==== Gap #4 (DELTA) — end-user read surface impl =======================

    async fn list_for_end_user(
        &self,
        scope: &ProjectScope,
        end_user_sub: &str,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<EndUserFeedback>, u32)> {
        // `end_user_sub = $3` is the isolation predicate: a caller sees ONLY
        // their own rows, and anonymous rows (end_user_sub IS NULL) never
        // match. No internal columns are selected.
        let rows = sqlx::query!(
            r#"
            SELECT short_code, kind, status, body, accepted_at
            FROM feedback
            WHERE tenant_id = $1 AND project_id = $2 AND end_user_sub = $3
            ORDER BY accepted_at DESC
            LIMIT $4 OFFSET $5
            "#,
            scope.tenant_id(),
            scope.project_id(),
            end_user_sub,
            i64::from(limit),
            i64::from(offset),
        )
        .fetch_all(&self.pool)
        .await?;

        let total_row = sqlx::query!(
            r#"
            SELECT count(*) AS "count!"
            FROM feedback
            WHERE tenant_id = $1 AND project_id = $2 AND end_user_sub = $3
            "#,
            scope.tenant_id(),
            scope.project_id(),
            end_user_sub,
        )
        .fetch_one(&self.pool)
        .await?;
        let total: u32 = total_row.count.try_into().unwrap_or(u32::MAX);

        let items = rows
            .into_iter()
            .map(|r| EndUserFeedback {
                feedback_id: FeedbackId::from(r.short_code),
                kind: FeedbackKind::from_db_str(&r.kind),
                status: FeedbackStatus::from_db_str(&r.status),
                body: r.body,
                submitted_at: r.accepted_at,
            })
            .collect();

        Ok((items, total))
    }

    async fn get_for_end_user(
        &self,
        scope: &ProjectScope,
        end_user_sub: &str,
        feedback_id: &FeedbackId,
    ) -> Result<EndUserFeedback> {
        // The `AND end_user_sub = $4` clause is the load-bearing isolation
        // check: a short_code belonging to a DIFFERENT sub (or anonymous, or
        // another tenant/project) returns NotFound, never another user's row.
        let row = sqlx::query!(
            r#"
            SELECT short_code, kind, status, body, accepted_at
            FROM feedback
            WHERE tenant_id = $1 AND project_id = $2
              AND short_code = $3 AND end_user_sub = $4
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_id.as_str(),
            end_user_sub,
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(crate::error::RepoError::NotFound)?;

        Ok(EndUserFeedback {
            feedback_id: FeedbackId::from(row.short_code),
            kind: FeedbackKind::from_db_str(&row.kind),
            status: FeedbackStatus::from_db_str(&row.status),
            body: row.body,
            submitted_at: row.accepted_at,
        })
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
                None, // crash_event_id — not crash-linked here
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
        // Not crash-linked → crash_event_id is NULL.
        assert_eq!(recent[0].crash_event_id, None);
    }

    // ---- Gap #2 crash-event correlation (BRAVO) ----

    #[sqlx::test(migrations = "../../migrations")]
    async fn submit_authenticated_persists_crash_event_id(pool: PgPool) {
        // A crash-linked auth-mode submission stores crash_event_id as a
        // first-class column (NOT inside external_metadata) and round-trips
        // through both read paths (list_recent + get_with_history).
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "crash-link@example.com").await;

        let crash_id = "a1b2c3d4e5f60718293a4b5c6d7e8f90";
        let id = repo
            .submit_authenticated(
                &scope,
                "auth0|sub-crash",
                Some("dev@example.com"),
                Some("Dev"),
                None,
                Some(crash_id),
                "App panicked on save",
                FeedbackKind::Bug,
            )
            .await
            .unwrap();

        // list_recent read path.
        let recent = repo.list_recent(&scope, 10).await.unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].crash_event_id.as_deref(), Some(crash_id));
        // First-class column — must NOT have been smuggled into metadata.
        assert!(recent[0].external_metadata.is_none());

        // get_with_history read path.
        let (fb, _hist) = repo.get_with_history(&scope, &id).await.unwrap();
        assert_eq!(fb.crash_event_id.as_deref(), Some(crash_id));
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

    // ---- Gap #3 full-text search (Task Zero: isolation-first) ----

    #[sqlx::test(migrations = "../../migrations")]
    async fn search_for_admin_matches_body_terms(pool: PgPool) {
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "fts-match@example.com").await;

        repo.submit_anonymous(&scope, &[1u8; 32], None, "the checkout button is broken", FeedbackKind::Bug)
            .await
            .unwrap();
        repo.submit_anonymous(&scope, &[2u8; 32], None, "please add a dark theme", FeedbackKind::Feature)
            .await
            .unwrap();

        // Multi-term query: both lexemes present in the first row's body.
        let (hits, total) = repo.search_for_admin(&scope, "broken checkout", 20, 0).await.unwrap();
        assert_eq!(total, 1);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].body_excerpt.contains("checkout"));

        // Non-matching term returns nothing (not an error).
        let (none, none_total) = repo.search_for_admin(&scope, "nonexistentterm", 20, 0).await.unwrap();
        assert!(none.is_empty());
        assert_eq!(none_total, 0);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn search_for_admin_cross_tenant_negative(pool: PgPool) {
        // THE load-bearing invariant for gap #3: search must never leak
        // another tenant's feedback. Mirrors list_for_admin_cross_tenant_negative.
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let s1 = seed_project_scope(&pool, "fts-owner1@example.com").await;
        let s2 = seed_project_scope(&pool, "fts-owner2@example.com").await;
        repo.submit_anonymous(&s1, &[1u8; 32], None, "secret roadmap leak details", FeedbackKind::Other)
            .await
            .unwrap();

        // s2 searches for s1's distinctive term — must return 0 rows, not error.
        let (page, total) = repo.search_for_admin(&s2, "secret roadmap", 20, 0).await.unwrap();
        assert!(page.is_empty(), "cross-tenant FTS must not leak rows");
        assert_eq!(total, 0);

        // s1 (the owner) finds its own row.
        let (own, own_total) = repo.search_for_admin(&s1, "secret roadmap", 20, 0).await.unwrap();
        assert_eq!(own.len(), 1);
        assert_eq!(own_total, 1);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn search_for_admin_paginates_with_total(pool: PgPool) {
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "fts-page@example.com").await;
        for i in 0..3 {
            repo.submit_anonymous(
                &scope,
                &[u8::try_from(i).unwrap(); 32],
                None,
                "shared keyword in every row",
                FeedbackKind::Other,
            )
            .await
            .unwrap();
        }

        let (page1, total) = repo.search_for_admin(&scope, "keyword", 2, 0).await.unwrap();
        assert_eq!(page1.len(), 2);
        assert_eq!(total, 3);

        let (page2, total2) = repo.search_for_admin(&scope, "keyword", 2, 2).await.unwrap();
        assert_eq!(page2.len(), 1);
        assert_eq!(total2, 3);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn search_for_admin_blank_query_matches_nothing(pool: PgPool) {
        let repo = SqlxFeedbackRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "fts-blank@example.com").await;
        repo.submit_anonymous(&scope, &[1u8; 32], None, "some body text", FeedbackKind::Other)
            .await
            .unwrap();

        // websearch_to_tsquery('') yields an empty query that matches nothing.
        let (page, total) = repo.search_for_admin(&scope, "   ", 20, 0).await.unwrap();
        assert!(page.is_empty());
        assert_eq!(total, 0);
    }
}
