//! Board-session repository — the server half of the hosted board's signed-in
//! session (FR-FBR-44, DEC-FBR-21, migration 00034, Contract C43).
//!
//! The API's exchange endpoint verifies a customer-signed end-user JWT with the
//! existing verifier, mints a random cookie token, and stores here only the
//! token's sha256 hex digest and the JWT's `sub` — nothing else from the token.
//! Hashing and token generation live in the API layer (mirrors
//! `password_resets`); this repository stores and looks up an opaque digest.
//!
//! Invariants:
//! 1. A session lives at most [`feedbackmonk_core::BOARD_SESSION_MAX_AGE_SECS`]
//!    (8 h) from creation and is never extended: there is no update method,
//!    and the 00034 CHECK refuses a longer row.
//! 2. [`BoardSessionRepo::resolve_board_session`] refuses an expired session
//!    (`expires_at > now()` in SQL) — expiry is not left to the cookie.
//! 3. Every method is project-scoped: a token minted for one project resolves
//!    to nothing on another (the cookie is project-scoped too).
//! 4. Erasure (FR-FBR-60): `FeedbackRepo::erase_all_for_end_user` deletes the
//!    sub's sessions.
//!
//! Reached through [`crate::BoardVoteRepo::sessions`].

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use feedbackmonk_core::BOARD_SESSION_MAX_AGE_SECS;

use crate::error::{RepoError, Result};
use crate::scope::ProjectScope;

/// A live board session: the viewer's `sub` and when the session ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardSession {
    pub user_sub: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[async_trait]
pub trait BoardSessionRepo: Send + Sync {
    /// Store a new session for `user_sub` under `token_hash` (64 lowercase
    /// hex chars, sha256 of the cookie token). Lifetime is `ttl_secs`, clamped
    /// to 8 hours; `ttl_secs <= 0` or a malformed hash is `Conflict`. A reused
    /// hash is `Conflict` (never a silent overwrite).
    async fn create_board_session(
        &self,
        scope: &ProjectScope,
        token_hash: &str,
        user_sub: &str,
        ttl_secs: i64,
    ) -> Result<BoardSession>;

    /// The live session behind `token_hash` in this project, or `None` when it
    /// is unknown, expired, ended or minted for another project.
    async fn resolve_board_session(
        &self,
        scope: &ProjectScope,
        token_hash: &str,
    ) -> Result<Option<BoardSession>>;

    /// End (delete) a session — sign-out. Returns whether one existed.
    async fn end_board_session(&self, scope: &ProjectScope, token_hash: &str) -> Result<bool>;

    /// Delete this project's expired sessions. Housekeeping only: lookups
    /// already refuse expired rows. Returns the number removed.
    async fn purge_expired_board_sessions(&self, scope: &ProjectScope) -> Result<u64>;
}

/// The Postgres implementation, reached through
/// [`crate::BoardVoteRepo::sessions`].
#[derive(Clone)]
pub struct SqlxBoardSessionRepo {
    pool: PgPool,
}

impl SqlxBoardSessionRepo {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[async_trait]
impl BoardSessionRepo for SqlxBoardSessionRepo {
    async fn create_board_session(
        &self,
        scope: &ProjectScope,
        token_hash: &str,
        user_sub: &str,
        ttl_secs: i64,
    ) -> Result<BoardSession> {
        if ttl_secs <= 0 || !is_sha256_hex(token_hash) || user_sub.is_empty() {
            return Err(RepoError::Conflict);
        }
        // i64 → f64 is exact for any TTL ≤ 8 h.
        #[allow(clippy::cast_precision_loss)]
        let ttl = ttl_secs.min(BOARD_SESSION_MAX_AGE_SECS) as f64;
        // Expiry is computed from the SAME now() the row's created_at takes, so
        // the 00034 ceiling CHECK holds exactly at 8 h.
        let row = match sqlx::query!(
            r#"
            INSERT INTO board_sessions (token_hash, tenant_id, project_id, user_sub, created_at, expires_at)
            VALUES ($1, $2, $3, $4, now(), now() + make_interval(secs => $5::float8))
            RETURNING user_sub, created_at, expires_at
            "#,
            token_hash,
            scope.tenant_id(),
            scope.project_id(),
            user_sub,
            ttl,
        )
        .fetch_one(&self.pool)
        .await
        {
            Ok(r) => r,
            Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
                return Err(RepoError::Conflict)
            }
            Err(e) => return Err(e.into()),
        };
        Ok(BoardSession {
            user_sub: row.user_sub,
            created_at: row.created_at,
            expires_at: row.expires_at,
        })
    }

    async fn resolve_board_session(
        &self,
        scope: &ProjectScope,
        token_hash: &str,
    ) -> Result<Option<BoardSession>> {
        let row = sqlx::query!(
            r#"
            SELECT user_sub, created_at, expires_at
            FROM board_sessions
            WHERE token_hash = $1 AND tenant_id = $2 AND project_id = $3
              AND expires_at > now()
            "#,
            token_hash,
            scope.tenant_id(),
            scope.project_id(),
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| BoardSession {
            user_sub: r.user_sub,
            created_at: r.created_at,
            expires_at: r.expires_at,
        }))
    }

    async fn end_board_session(&self, scope: &ProjectScope, token_hash: &str) -> Result<bool> {
        let result = sqlx::query!(
            r#"
            DELETE FROM board_sessions
            WHERE token_hash = $1 AND tenant_id = $2 AND project_id = $3
            "#,
            token_hash,
            scope.tenant_id(),
            scope.project_id(),
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn purge_expired_board_sessions(&self, scope: &ProjectScope) -> Result<u64> {
        let result = sqlx::query!(
            r#"
            DELETE FROM board_sessions
            WHERE tenant_id = $1 AND project_id = $2 AND expires_at <= now()
            "#,
            scope.tenant_id(),
            scope.project_id(),
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board_votes::{BoardVoteRepo, SqlxBoardVoteRepo};
    use crate::test_support::seed_project_scope;

    fn hash(c: char) -> String {
        c.to_string().repeat(64)
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn session_round_trips_and_is_project_scoped(pool: PgPool) {
        let repo = SqlxBoardVoteRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "sess@example.com").await;
        let other = seed_project_scope(&pool, "sess-other@example.com").await;

        let s = repo
            .sessions()
            .create_board_session(&scope, &hash('1'), "user-1", 3600)
            .await
            .unwrap();
        assert_eq!(s.user_sub, "user-1");
        assert_eq!((s.expires_at - s.created_at).num_seconds(), 3600);

        let got = repo.sessions().resolve_board_session(&scope, &hash('1')).await.unwrap();
        assert_eq!(got.as_ref().map(|g| g.user_sub.as_str()), Some("user-1"));
        // Minted for one project, it resolves to nothing on another.
        assert!(repo
            .sessions()
            .resolve_board_session(&other, &hash('1'))
            .await
            .unwrap()
            .is_none());

        // A reused hash is refused, never overwritten.
        let err = repo
            .sessions()
            .create_board_session(&scope, &hash('1'), "user-2", 3600)
            .await
            .unwrap_err();
        assert!(matches!(err, RepoError::Conflict));

        assert!(repo.sessions().end_board_session(&scope, &hash('1')).await.unwrap());
        assert!(repo
            .sessions()
            .resolve_board_session(&scope, &hash('1'))
            .await
            .unwrap()
            .is_none());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn session_lifetime_is_capped_at_eight_hours(pool: PgPool) {
        let repo = SqlxBoardVoteRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "sess-cap@example.com").await;
        let s = repo
            .sessions()
            .create_board_session(&scope, &hash('2'), "user-1", 30 * 24 * 3600)
            .await
            .unwrap();
        assert_eq!(
            (s.expires_at - s.created_at).num_seconds(),
            BOARD_SESSION_MAX_AGE_SECS,
            "a longer request is clamped to 8 h (DEC-FBR-21)"
        );
        for bad in [0, -5] {
            let err = repo
                .sessions()
                .create_board_session(&scope, &hash('3'), "user-1", bad)
                .await
                .unwrap_err();
            assert!(matches!(err, RepoError::Conflict));
        }
        let err = repo
            .sessions()
            .create_board_session(&scope, "not-a-digest", "user-1", 60)
            .await
            .unwrap_err();
        assert!(matches!(err, RepoError::Conflict));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn expired_session_is_refused_and_purged(pool: PgPool) {
        let repo = SqlxBoardVoteRepo::new(pool.clone());
        let scope = seed_project_scope(&pool, "sess-exp@example.com").await;
        repo.sessions()
            .create_board_session(&scope, &hash('4'), "user-1", 1)
            .await
            .unwrap();
        // Let the one-second session lapse.
        tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
        assert!(repo
            .sessions()
            .resolve_board_session(&scope, &hash('4'))
            .await
            .unwrap()
            .is_none());
        assert_eq!(repo.sessions().purge_expired_board_sessions(&scope).await.unwrap(), 1);
    }
}
