//! Board signals repository — a signed-in viewer's private signals on the
//! public board: "not needed" marks (FR-FBR-48/49), the personal priority list
//! (FR-FBR-50/51), the per-viewer state on board reads (FR-FBR-46), the
//! kind-filtered board page (FR-FBR-42), and the owner's aggregate inputs
//! (FR-FBR-52..55). Contract C43 (public) / C44 (owner):
//! `docs/planning/handoffs/prioritization-contracts.md`.
//!
//! Reached through [`crate::BoardVoteRepo::signals`] — the board's end-user
//! store, which already sits on `AppState` — so no new state field or
//! constructor exemption is needed (see the handoff, "Where the new repos live").
//!
//! Hard invariants (migration 00035):
//!
//! 1. **Moderation gate before every write (FR-FBR-61).** [`set_not_needed`]
//!    and [`save_priority_list`] resolve their targets through
//!    `resolve_pool_board_items` — an approved-only, ranking-pool query — inside
//!    the write's own transaction, BEFORE the insert. A pending, rejected,
//!    out-of-pool or out-of-scope item is `NotFound` and nothing is written.
//! 2. **Ranking pool (FR-FBR-51).** Approved, kind `feature`, status not
//!    shipped/wontfix/duplicate (`feedbackmonk_core::in_ranking_pool`). Every
//!    read restricts list entries to the pool, so an item that leaves it stops
//!    counting at once; [`load_priority_list`] reports it once and drops it.
//! 3. **Exclusive with the upvote (FR-FBR-48).** [`set_not_needed`] deletes the
//!    user's upvote in the same transaction; `BoardVoteRepo::cast` (JWT mode)
//!    deletes the user's mark in its transaction.
//! 4. **Privacy (FR-FBR-49/56).** Per-viewer reads return the caller's OWN
//!    rows only. The owner input [`board_priority_inputs`] returns lists
//!    WITHOUT any user identity and not-needed as a count only.
//! 5. Every SQL literal that reads `feedback` hard-filters
//!    `moderation_status = 'approved'` (C29 inv. 1; the
//!    `public-board-moderation-gate` oracle pins every fn named `*board*`).
//!
//! [`set_not_needed`]: BoardSignalRepo::set_not_needed
//! [`save_priority_list`]: BoardSignalRepo::save_priority_list
//! [`load_priority_list`]: BoardSignalRepo::load_priority_list
//! [`board_priority_inputs`]: BoardSignalRepo::board_priority_inputs

use std::collections::HashSet;

use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use feedbackmonk_core::{FeedbackId, FeedbackKind, FeedbackStatus, ViewerItemState};

use crate::error::{RepoError, Result};
use crate::feedback::BoardItem;
use crate::scope::ProjectScope;

/// Board page filter (FR-FBR-42). `kind: None` is today's unfiltered board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BoardFilter {
    pub kind: Option<FeedbackKind>,
}

/// One board item plus the CALLER's own state (FR-FBR-46). `viewer` is `None`
/// exactly when the read had no signed-in viewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewerBoardItem {
    pub item: BoardItem,
    pub viewer: Option<ViewerItemState>,
}

/// What [`BoardSignalRepo::set_not_needed`] changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotNeededOutcome {
    /// `false` when the mark already existed (the call is idempotent).
    pub newly_marked: bool,
    /// The user's upvote on the item was removed in the same transaction
    /// (FR-FBR-48 exclusivity).
    pub upvote_cleared: bool,
}

/// A signed-in viewer's priority list as they see it (FR-FBR-50/51).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PriorityListView {
    /// The list, top priority first. Every entry is in the ranking pool.
    pub entries: Vec<BoardItem>,
    /// Pool items NOT in the list — shown as "not yet ranked" (FR-FBR-50).
    pub unranked: Vec<BoardItem>,
    /// Entries that left the pool but are still on the public board (shipped,
    /// wontfix, duplicate/promoted). Reported ONCE, then dropped (FR-FBR-51).
    pub removed: Vec<BoardItem>,
    /// Entries that left the pool AND the public board (un-approved). Counted
    /// only — their content is no longer public. Dropped like `removed`.
    pub removed_hidden: u32,
}

/// One ranking-pool item with the owner's side-by-side counts (FR-FBR-55).
/// Owner-only: `not_needed_count` never reaches a public surface (FR-FBR-49).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolItemStats {
    pub feedback_id: FeedbackId,
    /// First 200 chars of the body.
    pub body_excerpt: String,
    pub status: FeedbackStatus,
    pub accepted_at: chrono::DateTime<chrono::Utc>,
    pub vote_count: i64,
    pub not_needed_count: i64,
}

/// Input to the aggregate fit (FR-FBR-52..55). `lists` carries NO user
/// identity (FR-FBR-56): each inner list is one user's in-pool order, top
/// first, and the order of the outer list means nothing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PriorityInputs {
    pub pool: Vec<PoolItemStats>,
    pub lists: Vec<Vec<FeedbackId>>,
}

#[async_trait]
pub trait BoardSignalRepo: Send + Sync {
    /// The public board page (FR-FBR-42/46): approved-only, newest-first,
    /// optionally filtered by kind (`total` counts only matching items), with
    /// the CALLER's state on each item when `viewer_sub` is `Some`.
    async fn list_board_for_viewer(
        &self,
        scope: &ProjectScope,
        filter: BoardFilter,
        viewer_sub: Option<&str>,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<ViewerBoardItem>, u32)>;

    /// The caller's own state on one approved board item (FR-FBR-46), for the
    /// detail view. `NotFound` for a non-approved / out-of-scope item.
    async fn viewer_state_for_board_item(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
        viewer_sub: &str,
    ) -> Result<ViewerItemState>;

    /// Mark a ranking-pool item not needed (FR-FBR-48/61). Resolves the item
    /// approved-only and in-pool BEFORE the write (`NotFound` otherwise) and
    /// clears the user's upvote in the same transaction. Idempotent.
    async fn set_not_needed(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
        user_sub: &str,
    ) -> Result<NotNeededOutcome>;

    /// Clear the caller's not-needed mark. Allowed whatever the item's state
    /// (withdrawing a signal is never gated). Returns whether a mark existed.
    async fn clear_not_needed(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
        user_sub: &str,
    ) -> Result<bool>;

    /// Replace the caller's whole priority list with `items`, top first
    /// (FR-FBR-50). Every item must resolve approved-only and in the pool
    /// before anything is written (FR-FBR-61): one unresolvable item →
    /// `NotFound`, a repeated item → `Conflict`, and the stored list is
    /// unchanged in both cases. An empty slice clears the list.
    async fn save_priority_list(
        &self,
        scope: &ProjectScope,
        user_sub: &str,
        items: &[FeedbackId],
    ) -> Result<PriorityListView>;

    /// The caller's priority list (FR-FBR-50/51). Entries that have left the
    /// pool are reported in `removed` / `removed_hidden` on THIS read and
    /// dropped from the stored list in the same transaction, so the next read
    /// no longer shows them.
    async fn load_priority_list(
        &self,
        scope: &ProjectScope,
        user_sub: &str,
    ) -> Result<PriorityListView>;

    /// Owner input for the aggregate (FR-FBR-52..56): the ranking pool with
    /// vote and not-needed counts, and every user's in-pool list without the
    /// user. Deterministic order (pool by short code; lists by an internal key).
    async fn board_priority_inputs(&self, scope: &ProjectScope) -> Result<PriorityInputs>;
}

/// The Postgres implementation. Built by [`crate::SqlxBoardVoteRepo::new`] and
/// reached through [`crate::BoardVoteRepo::signals`].
#[derive(Clone)]
pub struct SqlxBoardSignalRepo {
    pool: PgPool,
}

impl SqlxBoardSignalRepo {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// One resolved ranking-pool item: the internal id behind a public short code.
struct PoolItem {
    id: Uuid,
    short_code: String,
}

/// THE moderation gate for every signal write (FR-FBR-61, migration 00035
/// inv. 1): resolve public short codes to internal ids ONLY for approved,
/// in-scope, ranking-pool rows. Runs on the caller's transaction so the gate
/// and the write see one snapshot. Returns only the codes that resolved; the
/// caller compares lengths. Selects ids and codes only — no submitter column.
async fn resolve_pool_board_items(
    conn: &mut sqlx::PgConnection,
    scope: &ProjectScope,
    short_codes: &[String],
) -> Result<Vec<PoolItem>> {
    let rows = sqlx::query!(
        r#"
        SELECT f.id, f.short_code
        FROM feedback AS f
        WHERE f.tenant_id = $1 AND f.project_id = $2
          AND f.short_code = ANY($3)
          AND f.moderation_status = 'approved'
          AND f.kind = 'feature'
          AND f.status NOT IN ('shipped', 'wontfix', 'duplicate')
        FOR SHARE
        "#,
        scope.tenant_id(),
        scope.project_id(),
        short_codes,
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| PoolItem {
            id: r.id,
            short_code: r.short_code,
        })
        .collect())
}

fn board_item(
    short_code: String,
    kind: &str,
    status: &str,
    body: Option<String>,
    vote_count: i64,
    accepted_at: chrono::DateTime<chrono::Utc>,
) -> BoardItem {
    BoardItem {
        feedback_id: FeedbackId::from(short_code),
        kind: FeedbackKind::from_db_str(kind),
        status: FeedbackStatus::from_db_str(status),
        body: body.unwrap_or_default(),
        vote_count,
        accepted_at,
    }
}

impl SqlxBoardSignalRepo {
    /// The view of one user's list, read on `conn` (a transaction the caller
    /// owns). Does not drop anything; [`BoardSignalRepo::load_priority_list`]
    /// does that after reading.
    async fn read_board_priority_view(
        conn: &mut sqlx::PgConnection,
        scope: &ProjectScope,
        user_sub: &str,
    ) -> Result<PriorityListView> {
        // In-pool entries, in list order.
        let entries = sqlx::query!(
            r#"
            SELECT
                f.short_code, f.kind, f.status, f.body, f.accepted_at,
                (SELECT count(*) FROM feedback_board_votes v WHERE v.feedback_id = f.id) AS "vote_count!"
            FROM board_priority_entries AS e
            JOIN feedback AS f ON f.id = e.feedback_id
            WHERE e.tenant_id = $1 AND e.project_id = $2 AND e.user_sub = $3
              AND f.tenant_id = $1 AND f.project_id = $2
              AND f.moderation_status = 'approved'
              AND f.kind = 'feature'
              AND f.status NOT IN ('shipped', 'wontfix', 'duplicate')
            ORDER BY e.position ASC
            "#,
            scope.tenant_id(),
            scope.project_id(),
            user_sub,
        )
        .fetch_all(&mut *conn)
        .await?;

        // Entries still on the public board but out of the pool.
        let removed = sqlx::query!(
            r#"
            SELECT
                f.short_code, f.kind, f.status, f.body, f.accepted_at,
                (SELECT count(*) FROM feedback_board_votes v WHERE v.feedback_id = f.id) AS "vote_count!"
            FROM board_priority_entries AS e
            JOIN feedback AS f ON f.id = e.feedback_id
            WHERE e.tenant_id = $1 AND e.project_id = $2 AND e.user_sub = $3
              AND f.tenant_id = $1 AND f.project_id = $2
              AND f.moderation_status = 'approved'
              AND NOT (f.kind = 'feature' AND f.status NOT IN ('shipped', 'wontfix', 'duplicate'))
            ORDER BY e.position ASC
            "#,
            scope.tenant_id(),
            scope.project_id(),
            user_sub,
        )
        .fetch_all(&mut *conn)
        .await?;

        let total = Self::count_priority_entries(&mut *conn, scope, user_sub).await?;

        // Pool items the user has not placed: "not yet ranked".
        let unranked = sqlx::query!(
            r#"
            SELECT
                f.short_code, f.kind, f.status, f.body, f.accepted_at,
                (SELECT count(*) FROM feedback_board_votes v WHERE v.feedback_id = f.id) AS "vote_count!"
            FROM feedback AS f
            WHERE f.tenant_id = $1 AND f.project_id = $2
              AND f.moderation_status = 'approved'
              AND f.kind = 'feature'
              AND f.status NOT IN ('shipped', 'wontfix', 'duplicate')
              AND NOT EXISTS (
                  SELECT 1 FROM board_priority_entries AS e
                  WHERE e.tenant_id = $1 AND e.project_id = $2
                    AND e.user_sub = $3 AND e.feedback_id = f.id
              )
            ORDER BY f.accepted_at DESC, f.short_code ASC
            "#,
            scope.tenant_id(),
            scope.project_id(),
            user_sub,
        )
        .fetch_all(&mut *conn)
        .await?;

        let shown = entries.len() + removed.len();
        let hidden = usize::try_from(total)
            .unwrap_or(usize::MAX)
            .saturating_sub(shown);

        Ok(PriorityListView {
            entries: entries
                .into_iter()
                .map(|r| board_item(r.short_code, &r.kind, &r.status, r.body, r.vote_count, r.accepted_at))
                .collect(),
            unranked: unranked
                .into_iter()
                .map(|r| board_item(r.short_code, &r.kind, &r.status, r.body, r.vote_count, r.accepted_at))
                .collect(),
            removed: removed
                .into_iter()
                .map(|r| board_item(r.short_code, &r.kind, &r.status, r.body, r.vote_count, r.accepted_at))
                .collect(),
            removed_hidden: u32::try_from(hidden).unwrap_or(u32::MAX),
        })
    }

    /// Every stored entry of one user's list, in the pool or not. Reads only
    /// `board_priority_entries` (no feedback row), so it is kept out of the
    /// board-read fns whose every SQL literal must carry the approved filter.
    async fn count_priority_entries(
        conn: &mut sqlx::PgConnection,
        scope: &ProjectScope,
        user_sub: &str,
    ) -> Result<i64> {
        let row = sqlx::query!(
            r#"
            SELECT count(*) AS "count!"
            FROM board_priority_entries
            WHERE tenant_id = $1 AND project_id = $2 AND user_sub = $3
            "#,
            scope.tenant_id(),
            scope.project_id(),
            user_sub,
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(row.count)
    }

    /// Serialize every rewrite of one user's list (critic C-004,
    /// collab-20261013-145347). Two concurrent rewrites of the same list would
    /// otherwise both delete, then both insert positions 0..n, and the second
    /// insert would fail on the `(project, user, position)` UNIQUE constraint as
    /// a raw database error. A transaction-scoped advisory lock keyed on
    /// (project, user) makes the second wait for the first to commit; then it
    /// sees the first's rows, deletes them and writes its own. Released at
    /// commit or rollback. Reads no `feedback` row.
    async fn lock_priority_list(
        conn: &mut sqlx::PgConnection,
        scope: &ProjectScope,
        user_sub: &str,
    ) -> Result<()> {
        let key = format!("board_priority_entries:{}:{user_sub}", scope.project_id());
        sqlx::query!(
            r#"
            SELECT 1::int4 AS "locked!"
            FROM pg_advisory_xact_lock(hashtextextended($1, 0))
            "#,
            key,
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(())
    }

    /// Rewrite one user's stored list as exactly `ids`, positions 0..n, on
    /// `conn`. Delete-then-insert keeps the `(project, user, position)` UNIQUE
    /// constraint satisfied at every row (FR-FBR-50: saved whole).
    async fn write_priority_entries(
        conn: &mut sqlx::PgConnection,
        scope: &ProjectScope,
        user_sub: &str,
        ids: &[Uuid],
    ) -> Result<()> {
        sqlx::query!(
            r#"
            DELETE FROM board_priority_entries
            WHERE tenant_id = $1 AND project_id = $2 AND user_sub = $3
            "#,
            scope.tenant_id(),
            scope.project_id(),
            user_sub,
        )
        .execute(&mut *conn)
        .await?;

        if ids.is_empty() {
            return Ok(());
        }
        let positions: Vec<i32> = (0..ids.len())
            .map(|i| i32::try_from(i).unwrap_or(i32::MAX))
            .collect();
        sqlx::query!(
            r#"
            INSERT INTO board_priority_entries (tenant_id, project_id, user_sub, feedback_id, position)
            SELECT $1, $2, $3, t.feedback_id, t.position
            FROM UNNEST($4::uuid[], $5::int4[]) AS t(feedback_id, position)
            "#,
            scope.tenant_id(),
            scope.project_id(),
            user_sub,
            ids,
            &positions,
        )
        .execute(&mut *conn)
        .await
        .map_err(|e| match e {
            // Unreachable while every caller holds `lock_priority_list`; kept
            // so a future caller that forgets the lock gets `Conflict`, never
            // a raw unique-violation error (critic C-004).
            sqlx::Error::Database(db) if db.is_unique_violation() => RepoError::Conflict,
            e => e.into(),
        })?;
        Ok(())
    }
}

#[async_trait]
impl BoardSignalRepo for SqlxBoardSignalRepo {
    async fn list_board_for_viewer(
        &self,
        scope: &ProjectScope,
        filter: BoardFilter,
        viewer_sub: Option<&str>,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<ViewerBoardItem>, u32)> {
        // Approved-only hard literal on BOTH feedback reads (C29 inv. 1). The
        // viewer CTE ranks the caller's in-pool entries densely, so a position
        // is the item's place in the list the user would see. With no viewer
        // ($4 NULL) every viewer column is false/NULL and is discarded below.
        let kind = filter.kind.map(FeedbackKind::as_str);
        let rows = sqlx::query!(
            r#"
            WITH mine AS (
                SELECT e.feedback_id,
                       (row_number() OVER (ORDER BY e.position) - 1)::int4 AS pos
                FROM board_priority_entries AS e
                JOIN feedback AS pf ON pf.id = e.feedback_id
                WHERE e.tenant_id = $1 AND e.project_id = $2 AND e.user_sub = $4
                  AND pf.moderation_status = 'approved'
                  AND pf.kind = 'feature'
                  AND pf.status NOT IN ('shipped', 'wontfix', 'duplicate')
            )
            SELECT
                f.short_code, f.kind, f.status, f.body, f.accepted_at,
                (SELECT count(*) FROM feedback_board_votes v WHERE v.feedback_id = f.id) AS "vote_count!",
                EXISTS (
                    SELECT 1 FROM feedback_board_votes v
                    WHERE v.feedback_id = f.id AND v.voter_id = $4
                ) AS "upvoted!",
                EXISTS (
                    SELECT 1 FROM feedback_not_needed n
                    WHERE n.feedback_id = f.id AND n.user_sub = $4
                ) AS "not_needed!",
                (SELECT m.pos FROM mine m WHERE m.feedback_id = f.id) AS "priority_position?"
            FROM feedback AS f
            WHERE f.tenant_id = $1 AND f.project_id = $2
              AND f.moderation_status = 'approved'
              AND ($3::text IS NULL OR f.kind = $3)
            ORDER BY f.accepted_at DESC, f.short_code ASC
            LIMIT $5 OFFSET $6
            "#,
            scope.tenant_id(),
            scope.project_id(),
            kind,
            viewer_sub,
            i64::from(limit),
            i64::from(offset),
        )
        .fetch_all(&self.pool)
        .await?;

        let total_row = sqlx::query!(
            r#"
            SELECT count(*) AS "count!"
            FROM feedback AS f
            WHERE f.tenant_id = $1 AND f.project_id = $2
              AND f.moderation_status = 'approved'
              AND ($3::text IS NULL OR f.kind = $3)
            "#,
            scope.tenant_id(),
            scope.project_id(),
            kind,
        )
        .fetch_one(&self.pool)
        .await?;
        let total: u32 = total_row.count.try_into().unwrap_or(u32::MAX);

        let signed_in = viewer_sub.is_some();
        let list = rows
            .into_iter()
            .map(|r| ViewerBoardItem {
                viewer: signed_in.then(|| ViewerItemState {
                    upvoted: r.upvoted,
                    not_needed: r.not_needed,
                    priority_position: r.priority_position.and_then(|p| u32::try_from(p).ok()),
                }),
                item: board_item(r.short_code, &r.kind, &r.status, r.body, r.vote_count, r.accepted_at),
            })
            .collect();
        Ok((list, total))
    }

    async fn viewer_state_for_board_item(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
        viewer_sub: &str,
    ) -> Result<ViewerItemState> {
        let row = sqlx::query!(
            r#"
            WITH mine AS (
                SELECT e.feedback_id,
                       (row_number() OVER (ORDER BY e.position) - 1)::int4 AS pos
                FROM board_priority_entries AS e
                JOIN feedback AS pf ON pf.id = e.feedback_id
                WHERE e.tenant_id = $1 AND e.project_id = $2 AND e.user_sub = $4
                  AND pf.moderation_status = 'approved'
                  AND pf.kind = 'feature'
                  AND pf.status NOT IN ('shipped', 'wontfix', 'duplicate')
            )
            SELECT
                EXISTS (
                    SELECT 1 FROM feedback_board_votes v
                    WHERE v.feedback_id = f.id AND v.voter_id = $4
                ) AS "upvoted!",
                EXISTS (
                    SELECT 1 FROM feedback_not_needed n
                    WHERE n.feedback_id = f.id AND n.user_sub = $4
                ) AS "not_needed!",
                (SELECT m.pos FROM mine m WHERE m.feedback_id = f.id) AS "priority_position?"
            FROM feedback AS f
            WHERE f.tenant_id = $1 AND f.project_id = $2 AND f.short_code = $3
              AND f.moderation_status = 'approved'
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_id.as_str(),
            viewer_sub,
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(RepoError::NotFound)?;

        Ok(ViewerItemState {
            upvoted: row.upvoted,
            not_needed: row.not_needed,
            priority_position: row.priority_position.and_then(|p| u32::try_from(p).ok()),
        })
    }

    async fn set_not_needed(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
        user_sub: &str,
    ) -> Result<NotNeededOutcome> {
        let mut tx = self.pool.begin().await?;

        // FR-FBR-61: the approved-only pool gate runs BEFORE the write, on the
        // write's own transaction.
        let resolved =
            resolve_pool_board_items(&mut tx, scope, &[feedback_id.as_str().to_string()]).await?;
        let Some(item) = resolved.into_iter().next() else {
            tx.rollback().await?;
            return Err(RepoError::NotFound);
        };

        // FR-FBR-48: the mark and the upvote are exclusive — clear the vote in
        // the same transaction. A JWT voter's voter_id IS the sub.
        let cleared = sqlx::query!(
            r#"
            DELETE FROM feedback_board_votes
            WHERE tenant_id = $1 AND project_id = $2 AND feedback_id = $3 AND voter_id = $4
            "#,
            scope.tenant_id(),
            scope.project_id(),
            item.id,
            user_sub,
        )
        .execute(&mut *tx)
        .await?;

        let inserted = sqlx::query!(
            r#"
            INSERT INTO feedback_not_needed (tenant_id, project_id, feedback_id, user_sub)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (feedback_id, user_sub) DO NOTHING
            "#,
            scope.tenant_id(),
            scope.project_id(),
            item.id,
            user_sub,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(NotNeededOutcome {
            newly_marked: inserted.rows_affected() > 0,
            upvote_cleared: cleared.rows_affected() > 0,
        })
    }

    async fn clear_not_needed(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
        user_sub: &str,
    ) -> Result<bool> {
        // Withdrawing is never gated on moderation: the scope + short code
        // identify the row, and only the caller's own mark is deleted.
        let result = sqlx::query!(
            r#"
            DELETE FROM feedback_not_needed AS n
            WHERE n.tenant_id = $1 AND n.project_id = $2 AND n.user_sub = $4
              AND n.feedback_id IN (
                  SELECT f.id FROM feedback AS f
                  WHERE f.tenant_id = $1 AND f.project_id = $2 AND f.short_code = $3
              )
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_id.as_str(),
            user_sub,
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn save_priority_list(
        &self,
        scope: &ProjectScope,
        user_sub: &str,
        items: &[FeedbackId],
    ) -> Result<PriorityListView> {
        let codes: Vec<String> = items.iter().map(|i| i.as_str().to_string()).collect();
        let mut seen = HashSet::with_capacity(codes.len());
        if !codes.iter().all(|c| seen.insert(c.as_str())) {
            return Err(RepoError::Conflict);
        }

        let mut tx = self.pool.begin().await?;
        Self::lock_priority_list(&mut tx, scope, user_sub).await?;

        // FR-FBR-61: every entry resolves approved-only and in-pool BEFORE the
        // list is written. One miss refuses the whole save (FR-FBR-50: the list
        // is saved whole, never partially).
        let resolved = resolve_pool_board_items(&mut tx, scope, &codes).await?;
        if resolved.len() != codes.len() {
            tx.rollback().await?;
            return Err(RepoError::NotFound);
        }
        let ids: Vec<Uuid> = codes
            .iter()
            .filter_map(|c| resolved.iter().find(|p| &p.short_code == c).map(|p| p.id))
            .collect();

        Self::write_priority_entries(&mut tx, scope, user_sub, &ids).await?;
        let view = Self::read_board_priority_view(&mut tx, scope, user_sub).await?;
        tx.commit().await?;
        Ok(view)
    }

    async fn load_priority_list(
        &self,
        scope: &ProjectScope,
        user_sub: &str,
    ) -> Result<PriorityListView> {
        let mut tx = self.pool.begin().await?;
        // The read may rewrite the list (dropping leavers), so it takes the
        // same per-list lock as a save (critic C-004).
        Self::lock_priority_list(&mut tx, scope, user_sub).await?;
        let view = Self::read_board_priority_view(&mut tx, scope, user_sub).await?;

        // FR-FBR-51: shown once as removed, then dropped. Rewrite the list as
        // its in-pool entries only (dense positions) when anything left.
        if !view.removed.is_empty() || view.removed_hidden > 0 {
            let codes: Vec<String> = view
                .entries
                .iter()
                .map(|i| i.feedback_id.as_str().to_string())
                .collect();
            let resolved = resolve_pool_board_items(&mut tx, scope, &codes).await?;
            let ids: Vec<Uuid> = codes
                .iter()
                .filter_map(|c| resolved.iter().find(|p| &p.short_code == c).map(|p| p.id))
                .collect();
            Self::write_priority_entries(&mut tx, scope, user_sub, &ids).await?;
        }

        tx.commit().await?;
        Ok(view)
    }

    async fn board_priority_inputs(&self, scope: &ProjectScope) -> Result<PriorityInputs> {
        let pool_rows = sqlx::query!(
            r#"
            SELECT
                f.short_code,
                left(f.body, 200) AS body_excerpt,
                f.status,
                f.accepted_at,
                (SELECT count(*) FROM feedback_board_votes v WHERE v.feedback_id = f.id) AS "vote_count!",
                (SELECT count(*) FROM feedback_not_needed n WHERE n.feedback_id = f.id) AS "not_needed_count!"
            FROM feedback AS f
            WHERE f.tenant_id = $1 AND f.project_id = $2
              AND f.moderation_status = 'approved'
              AND f.kind = 'feature'
              AND f.status NOT IN ('shipped', 'wontfix', 'duplicate')
            ORDER BY f.short_code ASC
            "#,
            scope.tenant_id(),
            scope.project_id(),
        )
        .fetch_all(&self.pool)
        .await?;

        // Every user's in-pool list. The user key is used ONLY to group rows
        // here and never leaves this function (FR-FBR-56).
        let entry_rows = sqlx::query!(
            r#"
            SELECT e.user_sub, f.short_code
            FROM board_priority_entries AS e
            JOIN feedback AS f ON f.id = e.feedback_id
            WHERE e.tenant_id = $1 AND e.project_id = $2
              AND f.tenant_id = $1 AND f.project_id = $2
              AND f.moderation_status = 'approved'
              AND f.kind = 'feature'
              AND f.status NOT IN ('shipped', 'wontfix', 'duplicate')
            ORDER BY e.user_sub ASC, e.position ASC
            "#,
            scope.tenant_id(),
            scope.project_id(),
        )
        .fetch_all(&self.pool)
        .await?;

        let mut lists: Vec<Vec<FeedbackId>> = Vec::new();
        let mut current: Option<String> = None;
        for r in entry_rows {
            if current.as_deref() != Some(r.user_sub.as_str()) {
                lists.push(Vec::new());
                current = Some(r.user_sub);
            }
            if let Some(list) = lists.last_mut() {
                list.push(FeedbackId::from(r.short_code));
            }
        }

        Ok(PriorityInputs {
            pool: pool_rows
                .into_iter()
                .map(|r| PoolItemStats {
                    feedback_id: FeedbackId::from(r.short_code),
                    body_excerpt: r.body_excerpt.unwrap_or_default(),
                    status: FeedbackStatus::from_db_str(&r.status),
                    accepted_at: r.accepted_at,
                    vote_count: r.vote_count,
                    not_needed_count: r.not_needed_count,
                })
                .collect(),
            lists,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board_votes::{BoardVoteRepo, SqlxBoardVoteRepo};
    use crate::feedback::{FeedbackRepo, SqlxFeedbackRepo};
    use crate::test_support::{
        count_rows, seed_item, seed_pool_item, seed_project_scope, set_moderation, set_status,
    };
    use feedbackmonk_core::{ModerationStatus, RoadmapVoterMode};

    fn codes(items: &[BoardItem]) -> Vec<String> {
        items.iter().map(|i| i.feedback_id.as_str().to_string()).collect()
    }

    fn strs(ids: &[&FeedbackId]) -> Vec<String> {
        ids.iter().map(|i| i.as_str().to_string()).collect()
    }

    async fn internal_id(pool: &PgPool, scope: &ProjectScope, fb: &FeedbackId) -> Uuid {
        SqlxFeedbackRepo::new(pool.clone())
            .resolve_approved_board_feedback_id(scope, fb)
            .await
            .unwrap()
    }

    // ---- FR-FBR-42: kind filter ------------------------------------------

    #[sqlx::test(migrations = "../../migrations")]
    async fn kind_filter_returns_only_that_kind_and_counts_only_those(pool: PgPool) {
        let repo = SqlxBoardVoteRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "kind@example.com").await;
        let f1 = seed_pool_item(&pool, &scope, "feature one").await;
        let f2 = seed_pool_item(&pool, &scope, "feature two").await;
        seed_item(&pool, &scope, "a bug", FeedbackKind::Bug, ModerationStatus::Approved).await;
        // A pending feature is never on the board, filtered or not.
        seed_item(&pool, &scope, "hidden", FeedbackKind::Feature, ModerationStatus::Pending).await;

        let (all, total_all) = repo
            .signals()
            .list_board_for_viewer(&scope, BoardFilter::default(), None, 50, 0)
            .await
            .unwrap();
        assert_eq!(total_all, 3);
        assert_eq!(all.len(), 3);

        let feature = BoardFilter { kind: Some(FeedbackKind::Feature) };
        let (features, total) = repo
            .signals()
            .list_board_for_viewer(&scope, feature, None, 50, 0)
            .await
            .unwrap();
        assert_eq!(total, 2, "total counts only the filtered kind");
        let mut got: Vec<String> = features
            .iter()
            .map(|i| i.item.feedback_id.as_str().to_string())
            .collect();
        got.sort();
        let mut want = strs(&[&f1, &f2]);
        want.sort();
        assert_eq!(got, want);
        assert!(features.iter().all(|i| i.item.kind == FeedbackKind::Feature));
        assert!(features.iter().all(|i| i.viewer.is_none()), "no viewer, no state");

        let question = BoardFilter { kind: Some(FeedbackKind::Question) };
        let (questions, qtotal) = repo
            .signals()
            .list_board_for_viewer(&scope, question, None, 50, 0)
            .await
            .unwrap();
        assert_eq!((questions.len(), qtotal), (0, 0));
    }

    // ---- FR-FBR-61: approved-only (and in-pool) before every write --------

    #[sqlx::test(migrations = "../../migrations")]
    async fn not_needed_resolves_approved_pool_item_before_write(pool: PgPool) {
        let repo = SqlxBoardVoteRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "gate-nn@example.com").await;
        let pending =
            seed_item(&pool, &scope, "pending", FeedbackKind::Feature, ModerationStatus::Pending).await;
        let rejected =
            seed_item(&pool, &scope, "rejected", FeedbackKind::Feature, ModerationStatus::Rejected).await;
        let bug = seed_item(&pool, &scope, "bug", FeedbackKind::Bug, ModerationStatus::Approved).await;
        let shipped = seed_pool_item(&pool, &scope, "shipped").await;
        set_status(&pool, &scope, &shipped, "shipped").await;
        let other = seed_project_scope(&pool, "gate-nn-other@example.com").await;
        let foreign = seed_pool_item(&pool, &other, "other tenant").await;

        for fb in [&pending, &rejected, &bug, &shipped, &foreign] {
            let err = repo.signals().set_not_needed(&scope, fb, "user-1").await.unwrap_err();
            assert!(matches!(err, RepoError::NotFound), "{fb:?} must not resolve: {err:?}");
        }
        assert_eq!(count_rows(&pool, "feedback_not_needed", &scope).await, 0, "nothing written");

        let open = seed_pool_item(&pool, &scope, "open feature").await;
        let out = repo.signals().set_not_needed(&scope, &open, "user-1").await.unwrap();
        assert!(out.newly_marked);
        // Idempotent: at most one mark per (item, user).
        let again = repo.signals().set_not_needed(&scope, &open, "user-1").await.unwrap();
        assert!(!again.newly_marked);
        assert_eq!(count_rows(&pool, "feedback_not_needed", &scope).await, 1);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn priority_save_resolves_every_item_before_write(pool: PgPool) {
        let repo = SqlxBoardVoteRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "gate-pl@example.com").await;
        let a = seed_pool_item(&pool, &scope, "a").await;
        let b = seed_pool_item(&pool, &scope, "b").await;
        repo.signals()
            .save_priority_list(&scope, "user-1", &[a.clone(), b.clone()])
            .await
            .unwrap();

        let pending =
            seed_item(&pool, &scope, "pending", FeedbackKind::Feature, ModerationStatus::Pending).await;
        let err = repo
            .signals()
            .save_priority_list(&scope, "user-1", &[pending.clone(), a.clone()])
            .await
            .unwrap_err();
        assert!(matches!(err, RepoError::NotFound), "{err:?}");

        // The refused save changed nothing: the old list stands whole.
        let view = repo.signals().load_priority_list(&scope, "user-1").await.unwrap();
        assert_eq!(codes(&view.entries), strs(&[&a, &b]));
    }

    // ---- FR-FBR-48: upvote and not-needed exclusive, one transaction -------

    #[sqlx::test(migrations = "../../migrations")]
    async fn upvote_and_not_needed_are_mutually_exclusive(pool: PgPool) {
        let repo = SqlxBoardVoteRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "excl@example.com").await;
        let fb = seed_pool_item(&pool, &scope, "exclusive").await;
        let id = internal_id(&pool, &scope, &fb).await;

        repo.cast(&scope, id, "user-1", RoadmapVoterMode::Jwt).await.unwrap();
        let out = repo.signals().set_not_needed(&scope, &fb, "user-1").await.unwrap();
        assert!(out.upvote_cleared, "setting not-needed clears the upvote");
        assert_eq!(repo.vote_count_for_feedback(&scope, id).await.unwrap(), 0);
        let state = repo
            .signals()
            .viewer_state_for_board_item(&scope, &fb, "user-1")
            .await
            .unwrap();
        assert!(state.not_needed && !state.upvoted);

        repo.cast(&scope, id, "user-1", RoadmapVoterMode::Jwt).await.unwrap();
        let state = repo
            .signals()
            .viewer_state_for_board_item(&scope, &fb, "user-1")
            .await
            .unwrap();
        assert!(state.upvoted && !state.not_needed, "upvoting clears the mark");
        assert_eq!(count_rows(&pool, "feedback_not_needed", &scope).await, 0);

        // Another user's signals are untouched by either write.
        repo.signals().set_not_needed(&scope, &fb, "user-2").await.unwrap();
        assert_eq!(repo.vote_count_for_feedback(&scope, id).await.unwrap(), 1);
        assert!(repo.signals().clear_not_needed(&scope, &fb, "user-2").await.unwrap());
        assert!(!repo.signals().clear_not_needed(&scope, &fb, "user-2").await.unwrap());
    }

    // ---- FR-FBR-50: the list is saved whole -------------------------------

    #[sqlx::test(migrations = "../../migrations")]
    async fn priority_list_is_saved_whole(pool: PgPool) {
        let repo = SqlxBoardVoteRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "whole@example.com").await;
        let a = seed_pool_item(&pool, &scope, "a").await;
        let b = seed_pool_item(&pool, &scope, "b").await;
        let c = seed_pool_item(&pool, &scope, "c").await;

        let v = repo
            .signals()
            .save_priority_list(&scope, "user-1", &[a.clone(), b.clone(), c.clone()])
            .await
            .unwrap();
        assert_eq!(codes(&v.entries), strs(&[&a, &b, &c]));
        assert!(v.unranked.is_empty());

        // A new save replaces the whole order; omitted items become unranked.
        let v = repo
            .signals()
            .save_priority_list(&scope, "user-1", &[c.clone(), a.clone()])
            .await
            .unwrap();
        assert_eq!(codes(&v.entries), strs(&[&c, &a]));
        assert_eq!(codes(&v.unranked), strs(&[&b]));

        // A repeated item is refused and the stored list is unchanged.
        let err = repo
            .signals()
            .save_priority_list(&scope, "user-1", &[a.clone(), a.clone()])
            .await
            .unwrap_err();
        assert!(matches!(err, RepoError::Conflict));
        let v = repo.signals().load_priority_list(&scope, "user-1").await.unwrap();
        assert_eq!(codes(&v.entries), strs(&[&c, &a]));

        // The viewer's position is their place in the list they see.
        let s = repo
            .signals()
            .viewer_state_for_board_item(&scope, &a, "user-1")
            .await
            .unwrap();
        assert_eq!(s.priority_position, Some(1));
        let s = repo
            .signals()
            .viewer_state_for_board_item(&scope, &a, "user-2")
            .await
            .unwrap();
        assert_eq!(s.priority_position, None, "another user's list is not mine");

        // One-item and empty lists are accepted.
        let v = repo
            .signals()
            .save_priority_list(&scope, "user-1", std::slice::from_ref(&b))
            .await
            .unwrap();
        assert_eq!(codes(&v.entries), strs(&[&b]));
        let v = repo.signals().save_priority_list(&scope, "user-1", &[]).await.unwrap();
        assert!(v.entries.is_empty());
        assert_eq!(v.unranked.len(), 3);
    }

    // ---- FR-FBR-51: pool membership and drop-out --------------------------

    #[sqlx::test(migrations = "../../migrations")]
    async fn items_leaving_the_pool_are_reported_once_then_dropped(pool: PgPool) {
        let repo = SqlxBoardVoteRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "pool@example.com").await;
        let a = seed_pool_item(&pool, &scope, "stays").await;
        let b = seed_pool_item(&pool, &scope, "ships").await;
        let c = seed_pool_item(&pool, &scope, "unapproved").await;
        let d = seed_pool_item(&pool, &scope, "promoted").await;
        repo.signals()
            .save_priority_list(&scope, "user-1", &[b.clone(), a.clone(), c.clone(), d.clone()])
            .await
            .unwrap();
        repo.signals()
            .save_priority_list(&scope, "user-2", &[c.clone(), a.clone()])
            .await
            .unwrap();

        set_status(&pool, &scope, &b, "shipped").await;
        set_status(&pool, &scope, &d, "duplicate").await; // promotion to the roadmap
        set_moderation(&pool, &scope, &c, ModerationStatus::Pending).await;

        // Out-of-pool items stop counting at once, before anyone reloads.
        let inputs = repo.signals().board_priority_inputs(&scope).await.unwrap();
        let pool_codes: Vec<String> = inputs
            .pool
            .iter()
            .map(|p| p.feedback_id.as_str().to_string())
            .collect();
        assert_eq!(pool_codes, strs(&[&a]));
        for list in &inputs.lists {
            assert!(list.iter().all(|i| i == &a), "only pool items aggregate: {list:?}");
        }

        let v = repo.signals().load_priority_list(&scope, "user-1").await.unwrap();
        assert_eq!(codes(&v.entries), strs(&[&a]));
        let mut removed = codes(&v.removed);
        removed.sort();
        let mut want = strs(&[&b, &d]);
        want.sort();
        assert_eq!(removed, want, "still-public leavers are named");
        assert_eq!(v.removed_hidden, 1, "the un-approved leaver is only counted");

        // Reported once: the next read has dropped them.
        let v = repo.signals().load_priority_list(&scope, "user-1").await.unwrap();
        assert_eq!(codes(&v.entries), strs(&[&a]));
        assert!(v.removed.is_empty());
        assert_eq!(v.removed_hidden, 0);
        let s = repo
            .signals()
            .viewer_state_for_board_item(&scope, &a, "user-1")
            .await
            .unwrap();
        assert_eq!(s.priority_position, Some(0));

        // A leaver cannot be put back while it is out of the pool.
        let err = repo
            .signals()
            .save_priority_list(&scope, "user-1", &[a.clone(), b.clone()])
            .await
            .unwrap_err();
        assert!(matches!(err, RepoError::NotFound));
    }

    // ---- FR-FBR-46/49/56: per-viewer state and owner aggregates ------------

    #[sqlx::test(migrations = "../../migrations")]
    async fn viewer_state_is_the_callers_own_and_owner_sees_only_aggregates(pool: PgPool) {
        let repo = SqlxBoardVoteRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "viewer@example.com").await;
        let a = seed_pool_item(&pool, &scope, "a").await;
        let b = seed_pool_item(&pool, &scope, "b").await;
        let a_id = internal_id(&pool, &scope, &a).await;

        repo.cast(&scope, a_id, "user-1", RoadmapVoterMode::Jwt).await.unwrap();
        repo.signals().set_not_needed(&scope, &b, "user-2").await.unwrap();
        repo.signals()
            .save_priority_list(&scope, "user-2", &[b.clone(), a.clone()])
            .await
            .unwrap();

        let (items, _) = repo
            .signals()
            .list_board_for_viewer(&scope, BoardFilter::default(), Some("user-1"), 50, 0)
            .await
            .unwrap();
        assert_eq!(items.len(), 2);
        for i in &items {
            let v = i.viewer.expect("a signed-in viewer gets state");
            assert!(!v.not_needed, "user-2's mark is not user-1's state");
            assert_eq!(v.priority_position, None, "user-2's list is not user-1's state");
            assert_eq!(v.upvoted, i.item.feedback_id == a);
        }

        let inputs = repo.signals().board_priority_inputs(&scope).await.unwrap();
        let b_stats = inputs.pool.iter().find(|p| p.feedback_id == b).unwrap();
        assert_eq!(b_stats.not_needed_count, 1);
        let a_stats = inputs.pool.iter().find(|p| p.feedback_id == a).unwrap();
        assert_eq!(a_stats.vote_count, 1);
        assert_eq!(inputs.lists, vec![vec![b.clone(), a.clone()]]);
    }

    // ---- FR-FBR-60: erasure reaches every new table -----------------------

    #[sqlx::test(migrations = "../../migrations")]
    async fn erasure_of_a_user_removes_marks_list_and_sessions(pool: PgPool) {
        let repo = SqlxBoardVoteRepo::new(pool.clone());
        let frepo = SqlxFeedbackRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "erase-user@example.com").await;
        let a = seed_pool_item(&pool, &scope, "a").await;
        let b = seed_pool_item(&pool, &scope, "b").await;

        for who in ["gone", "stays"] {
            repo.signals().set_not_needed(&scope, &a, who).await.unwrap();
            repo.signals()
                .save_priority_list(&scope, who, &[b.clone(), a.clone()])
                .await
                .unwrap();
        }
        let gone_hash = "a".repeat(64);
        let stays_hash = "b".repeat(64);
        repo.sessions()
            .create_board_session(&scope, &gone_hash, "gone", 3600)
            .await
            .unwrap();
        repo.sessions()
            .create_board_session(&scope, &stays_hash, "stays", 3600)
            .await
            .unwrap();

        frepo.erase_all_for_end_user(&scope, "gone").await.unwrap();

        assert_eq!(count_rows(&pool, "feedback_not_needed", &scope).await, 1);
        assert_eq!(count_rows(&pool, "board_priority_entries", &scope).await, 2);
        assert_eq!(count_rows(&pool, "board_sessions", &scope).await, 1);
        let gone = repo
            .signals()
            .viewer_state_for_board_item(&scope, &a, "gone")
            .await
            .unwrap();
        assert_eq!(gone, ViewerItemState::default());
        assert!(repo
            .sessions()
            .resolve_board_session(&scope, &gone_hash)
            .await
            .unwrap()
            .is_none());
        let kept = repo.signals().load_priority_list(&scope, "stays").await.unwrap();
        assert_eq!(codes(&kept.entries), strs(&[&b, &a]));
        assert!(repo
            .sessions()
            .resolve_board_session(&scope, &stays_hash)
            .await
            .unwrap()
            .is_some());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn erasure_of_an_item_removes_its_marks_entries_and_note(pool: PgPool) {
        let repo = SqlxBoardVoteRepo::new(pool.clone());
        let frepo = SqlxFeedbackRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "erase-item@example.com").await;
        let keep = seed_pool_item(&pool, &scope, "keep").await;
        // Submitted by a signed-in user, so the end-user erase path can delete
        // it; another user has marked and ranked it.
        let doomed = frepo
            .submit_authenticated(
                &scope,
                "submitter",
                None,
                None,
                None,
                None,
                "doomed",
                None,
                FeedbackKind::Feature,
            )
            .await
            .unwrap();
        set_moderation(&pool, &scope, &doomed, ModerationStatus::Approved).await;
        repo.signals().set_not_needed(&scope, &doomed, "voter").await.unwrap();
        repo.signals()
            .save_priority_list(&scope, "voter", &[doomed.clone(), keep.clone()])
            .await
            .unwrap();
        repo.notes()
            .set_board_note(&scope, &doomed, "why this waits")
            .await
            .unwrap();

        assert!(frepo.delete_for_end_user(&scope, "submitter", &doomed).await.unwrap());

        assert_eq!(count_rows(&pool, "feedback_not_needed", &scope).await, 0);
        assert_eq!(count_rows(&pool, "board_priority_entries", &scope).await, 1);
        assert_eq!(count_rows(&pool, "feedback_board_notes", &scope).await, 0);
        let v = repo.signals().load_priority_list(&scope, "voter").await.unwrap();
        assert_eq!(codes(&v.entries), strs(&[&keep]));
    }
}
