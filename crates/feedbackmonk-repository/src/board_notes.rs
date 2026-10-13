//! Board-notes repository — the board detail view's public record of an item
//! (FR-FBR-57/59) and the owner's board note on it (FR-FBR-58, DEC-FBR-22,
//! migration 00036, Contracts C43/C44).
//!
//! Every fn here that reads `feedback` is a board read and hard-filters
//! `moderation_status = 'approved'` in each SQL literal (C29 inv. 1). That
//! includes the OWNER's note writes: a board note belongs to a board item, so
//! it can be set, read or cleared only while the item is on the board. A note
//! on an item that later leaves the board is kept (it reappears if the item is
//! re-approved) and is erased with the item by FK cascade (FR-FBR-60).
//!
//! The detail projection (FR-FBR-59) carries no submitter identity, no reply,
//! no internal or status-change note, no attachment, no translated body and no
//! submitter locale: the timeline is `(to_status, transitioned_at)` only.
//!
//! Reached through [`crate::BoardVoteRepo::notes`].

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use feedbackmonk_core::{FeedbackId, FeedbackKind, FeedbackStatus};

use crate::error::{RepoError, Result};
use crate::feedback::BoardItem;
use crate::scope::ProjectScope;

/// Longest board note, in characters (00036 CHECK).
pub const BOARD_NOTE_MAX_CHARS: usize = 4000;

/// The owner's note, written for every board viewer (FR-FBR-58).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardNote {
    pub body: String,
    pub updated_at: DateTime<Utc>,
}

/// One submitter-visible status transition: the status reached and when.
/// Never the transition's note or who made it (FR-FBR-57).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardTimelineEntry {
    pub status: FeedbackStatus,
    pub at: DateTime<Utc>,
}

/// The board detail view's record of one approved item (FR-FBR-57).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardItemDetail {
    pub item: BoardItem,
    pub board_note: Option<BoardNote>,
    /// Oldest first.
    pub timeline: Vec<BoardTimelineEntry>,
}

#[async_trait]
pub trait BoardNoteRepo: Send + Sync {
    /// The public detail record of one approved, in-scope item (FR-FBR-57).
    /// `NotFound` for a pending/rejected/out-of-scope short code.
    async fn get_board_item_detail(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
    ) -> Result<BoardItemDetail>;

    /// The owner's read of an item's note. `NotFound` when the item is not on
    /// the board; `Ok(None)` when it is but has no note.
    async fn get_board_note(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
    ) -> Result<Option<BoardNote>>;

    /// Set or replace the note (owner only — the caller holds an admin-minted
    /// scope). `body` is trimmed; empty or longer than
    /// [`BOARD_NOTE_MAX_CHARS`] is `Conflict`. `NotFound` when the item is not
    /// on the board.
    async fn set_board_note(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
        body: &str,
    ) -> Result<BoardNote>;

    /// Clear the note. Returns whether one existed. `NotFound` when the item
    /// is not on the board.
    async fn clear_board_note(&self, scope: &ProjectScope, feedback_id: &FeedbackId) -> Result<bool>;
}

/// The Postgres implementation, reached through [`crate::BoardVoteRepo::notes`].
#[derive(Clone)]
pub struct SqlxBoardNoteRepo {
    pool: PgPool,
}

impl SqlxBoardNoteRepo {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl BoardNoteRepo for SqlxBoardNoteRepo {
    async fn get_board_item_detail(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
    ) -> Result<BoardItemDetail> {
        let row = sqlx::query!(
            r#"
            SELECT
                f.id, f.short_code, f.kind, f.status, f.body, f.accepted_at,
                (SELECT count(*) FROM feedback_board_votes v WHERE v.feedback_id = f.id) AS "vote_count!",
                n.body AS "note_body?",
                n.updated_at AS "note_updated_at?"
            FROM feedback AS f
            LEFT JOIN feedback_board_notes AS n ON n.feedback_id = f.id
            WHERE f.tenant_id = $1 AND f.project_id = $2 AND f.short_code = $3
              AND f.moderation_status = 'approved'
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_id.as_str(),
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(RepoError::NotFound)?;

        // Status timeline: target status + date ONLY (no reason_note, no
        // transitioned_by — FR-FBR-57). Joined under the approved filter so a
        // hidden item's history is unreachable.
        let timeline = sqlx::query!(
            r#"
            SELECT h.to_status, h.transitioned_at
            FROM feedback_status_history AS h
            JOIN feedback AS f ON f.id = h.feedback_id
            WHERE f.tenant_id = $1 AND f.project_id = $2 AND f.id = $3
              AND f.moderation_status = 'approved'
            ORDER BY h.transitioned_at ASC, h.id ASC
            "#,
            scope.tenant_id(),
            scope.project_id(),
            row.id,
        )
        .fetch_all(&self.pool)
        .await?;

        let board_note = match (row.note_body, row.note_updated_at) {
            (Some(body), Some(updated_at)) => Some(BoardNote { body, updated_at }),
            _ => None,
        };

        Ok(BoardItemDetail {
            item: BoardItem {
                feedback_id: FeedbackId::from(row.short_code),
                kind: FeedbackKind::from_db_str(&row.kind),
                status: FeedbackStatus::from_db_str(&row.status),
                body: row.body.unwrap_or_default(),
                vote_count: row.vote_count,
                accepted_at: row.accepted_at,
            },
            board_note,
            timeline: timeline
                .into_iter()
                .map(|t| BoardTimelineEntry {
                    status: FeedbackStatus::from_db_str(&t.to_status),
                    at: t.transitioned_at,
                })
                .collect(),
        })
    }

    async fn get_board_note(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
    ) -> Result<Option<BoardNote>> {
        let row = sqlx::query!(
            r#"
            SELECT n.body AS "body?", n.updated_at AS "updated_at?"
            FROM feedback AS f
            LEFT JOIN feedback_board_notes AS n ON n.feedback_id = f.id
            WHERE f.tenant_id = $1 AND f.project_id = $2 AND f.short_code = $3
              AND f.moderation_status = 'approved'
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_id.as_str(),
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(RepoError::NotFound)?;
        Ok(match (row.body, row.updated_at) {
            (Some(body), Some(updated_at)) => Some(BoardNote { body, updated_at }),
            _ => None,
        })
    }

    async fn set_board_note(
        &self,
        scope: &ProjectScope,
        feedback_id: &FeedbackId,
        body: &str,
    ) -> Result<BoardNote> {
        let body = body.trim();
        if body.is_empty() || body.chars().count() > BOARD_NOTE_MAX_CHARS {
            return Err(RepoError::Conflict);
        }
        let row = sqlx::query!(
            r#"
            INSERT INTO feedback_board_notes (feedback_id, tenant_id, project_id, body, updated_at)
            SELECT f.id, f.tenant_id, f.project_id, $4, now()
            FROM feedback AS f
            WHERE f.tenant_id = $1 AND f.project_id = $2 AND f.short_code = $3
              AND f.moderation_status = 'approved'
            ON CONFLICT (feedback_id) DO UPDATE
                SET body = EXCLUDED.body, updated_at = EXCLUDED.updated_at
            RETURNING body, updated_at
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_id.as_str(),
            body,
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(RepoError::NotFound)?;
        Ok(BoardNote {
            body: row.body,
            updated_at: row.updated_at,
        })
    }

    async fn clear_board_note(&self, scope: &ProjectScope, feedback_id: &FeedbackId) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let target = sqlx::query!(
            r#"
            SELECT f.id
            FROM feedback AS f
            WHERE f.tenant_id = $1 AND f.project_id = $2 AND f.short_code = $3
              AND f.moderation_status = 'approved'
            "#,
            scope.tenant_id(),
            scope.project_id(),
            feedback_id.as_str(),
        )
        .fetch_optional(&mut *tx)
        .await?;
        let Some(target) = target else {
            tx.rollback().await?;
            return Err(RepoError::NotFound);
        };
        let result = sqlx::query!(
            r#"
            DELETE FROM feedback_board_notes
            WHERE feedback_id = $1 AND tenant_id = $2 AND project_id = $3
              AND feedback_id IN (
                  SELECT f.id FROM feedback AS f
                  WHERE f.id = $1 AND f.moderation_status = 'approved'
              )
            "#,
            target.id,
            scope.tenant_id(),
            scope.project_id(),
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(result.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board_votes::{BoardVoteRepo, SqlxBoardVoteRepo};
    use crate::test_support::{seed_item, seed_pool_item, seed_project_scope, set_moderation};
    use feedbackmonk_core::ModerationStatus;

    #[sqlx::test(migrations = "../../migrations")]
    async fn note_set_edit_clear_round_trips_into_detail(pool: PgPool) {
        let repo = SqlxBoardVoteRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "note@example.com").await;
        let fb = seed_pool_item(&pool, &scope, "verbatim body").await;

        let d = repo.notes().get_board_item_detail(&scope, &fb).await.unwrap();
        assert_eq!(d.item.body, "verbatim body");
        assert!(d.board_note.is_none());
        assert!(repo.notes().get_board_note(&scope, &fb).await.unwrap().is_none());

        repo.notes().set_board_note(&scope, &fb, "  first  ").await.unwrap();
        repo.notes().set_board_note(&scope, &fb, "edited").await.unwrap();
        let d = repo.notes().get_board_item_detail(&scope, &fb).await.unwrap();
        assert_eq!(d.board_note.map(|n| n.body).as_deref(), Some("edited"));

        assert!(repo.notes().clear_board_note(&scope, &fb).await.unwrap());
        assert!(!repo.notes().clear_board_note(&scope, &fb).await.unwrap());
        let d = repo.notes().get_board_item_detail(&scope, &fb).await.unwrap();
        assert!(d.board_note.is_none());

        let long = "x".repeat(BOARD_NOTE_MAX_CHARS + 1);
        for bad in ["", "   ", long.as_str()] {
            let err = repo.notes().set_board_note(&scope, &fb, bad).await.unwrap_err();
            assert!(matches!(err, RepoError::Conflict));
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn notes_and_detail_reach_only_board_items(pool: PgPool) {
        let repo = SqlxBoardVoteRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "note-gate@example.com").await;
        let pending =
            seed_item(&pool, &scope, "pending", FeedbackKind::Feature, ModerationStatus::Pending).await;
        let other = seed_project_scope(&pool, "note-gate-other@example.com").await;
        let foreign = seed_pool_item(&pool, &other, "other").await;

        for fb in [&pending, &foreign] {
            assert!(matches!(
                repo.notes().get_board_item_detail(&scope, fb).await.unwrap_err(),
                RepoError::NotFound
            ));
            assert!(matches!(
                repo.notes().set_board_note(&scope, fb, "note").await.unwrap_err(),
                RepoError::NotFound
            ));
            assert!(matches!(
                repo.notes().clear_board_note(&scope, fb).await.unwrap_err(),
                RepoError::NotFound
            ));
        }

        // A note survives un-approval unseen and returns with re-approval.
        let fb = seed_pool_item(&pool, &scope, "toggled").await;
        repo.notes().set_board_note(&scope, &fb, "kept").await.unwrap();
        set_moderation(&pool, &scope, &fb, ModerationStatus::Rejected).await;
        assert!(matches!(
            repo.notes().get_board_item_detail(&scope, &fb).await.unwrap_err(),
            RepoError::NotFound
        ));
        set_moderation(&pool, &scope, &fb, ModerationStatus::Approved).await;
        let d = repo.notes().get_board_item_detail(&scope, &fb).await.unwrap();
        assert_eq!(d.board_note.map(|n| n.body).as_deref(), Some("kept"));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn detail_timeline_carries_status_and_date_only(pool: PgPool) {
        let repo = SqlxBoardVoteRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "note-tl@example.com").await;
        let fb = seed_pool_item(&pool, &scope, "timeline").await;
        sqlx::query(
            "INSERT INTO feedback_status_history (feedback_id, from_status, to_status, reason_note, transitioned_by, transitioned_at)
             SELECT id, 'submitted', 'triaged', 'internal reasoning', $2, now() - interval '1 hour'
             FROM feedback WHERE project_id = $1 AND short_code = $3",
        )
        .bind(scope.project_id())
        .bind(scope.tenant_id())
        .bind(fb.as_str())
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO feedback_status_history (feedback_id, from_status, to_status, transitioned_by)
             SELECT id, 'triaged', 'in-progress', $2 FROM feedback WHERE project_id = $1 AND short_code = $3",
        )
        .bind(scope.project_id())
        .bind(scope.tenant_id())
        .bind(fb.as_str())
        .execute(&pool)
        .await
        .unwrap();

        let d = repo.notes().get_board_item_detail(&scope, &fb).await.unwrap();
        let statuses: Vec<FeedbackStatus> = d.timeline.iter().map(|t| t.status).collect();
        assert_eq!(statuses, vec![FeedbackStatus::Triaged, FeedbackStatus::InProgress]);
        assert!(d.timeline[0].at < d.timeline[1].at, "oldest first");
    }
}
