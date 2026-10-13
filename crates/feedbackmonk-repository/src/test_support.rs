//! Shared fixtures for this crate's `#[sqlx::test]` unit tests (compiled only
//! under `cfg(test)`). Seeds a tenant + project scope and board items in a
//! chosen moderation state, kind and triage status.

use sqlx::PgPool;

use feedbackmonk_core::{FeedbackId, FeedbackKind, ModerationStatus};

use crate::feedback::{FeedbackRepo, SqlxFeedbackRepo};
use crate::projects::{ProjectRepo, SqlxProjectRepo};
use crate::scope::ProjectScope;
use crate::tenants::{SqlxTenantRepo, TenantRepo};

pub(crate) async fn seed_project_scope(pool: &PgPool, email: &str) -> ProjectScope {
    let trepo = SqlxTenantRepo::new(pool.clone());
    let prepo = SqlxProjectRepo::new(pool.clone());
    let t = trepo.create(email, "h").await.unwrap();
    let scope = trepo.scope_for(t.id).await.unwrap();
    let p = prepo.create(&scope, "Proj", "proj").await.unwrap();
    prepo.open(&scope, p.id).await.unwrap()
}

/// Submit an anonymous item of `kind` and move it to `moderation`.
pub(crate) async fn seed_item(
    pool: &PgPool,
    scope: &ProjectScope,
    body: &str,
    kind: FeedbackKind,
    moderation: ModerationStatus,
) -> FeedbackId {
    let repo = SqlxFeedbackRepo::new(pool.clone());
    let fb = repo
        .submit_anonymous(scope, &[7u8; 32], None, body, None, kind)
        .await
        .unwrap();
    if moderation != ModerationStatus::Pending {
        set_moderation(pool, scope, &fb, moderation).await;
    }
    fb
}

/// An approved `feature` item in an open status: a ranking-pool member.
pub(crate) async fn seed_pool_item(pool: &PgPool, scope: &ProjectScope, body: &str) -> FeedbackId {
    seed_item(pool, scope, body, FeedbackKind::Feature, ModerationStatus::Approved).await
}

pub(crate) async fn set_moderation(
    pool: &PgPool,
    scope: &ProjectScope,
    fb: &FeedbackId,
    to: ModerationStatus,
) {
    let repo = SqlxFeedbackRepo::new(pool.clone());
    let mut tx = pool.begin().await.unwrap();
    repo.moderate_in_executor(scope, &mut tx, fb, to, None, scope.tenant_id())
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

/// Set an item's triage status directly (fixture shortcut around the status
/// workflow, which is not under test here).
pub(crate) async fn set_status(pool: &PgPool, scope: &ProjectScope, fb: &FeedbackId, status: &str) {
    sqlx::query("UPDATE feedback SET status = $1 WHERE project_id = $2 AND short_code = $3")
        .bind(status)
        .bind(scope.project_id())
        .bind(fb.as_str())
        .execute(pool)
        .await
        .unwrap();
}

/// Rows in `table` for this project — a fixture probe for erasure tests.
pub(crate) async fn count_rows(pool: &PgPool, table: &str, scope: &ProjectScope) -> i64 {
    // `table` is a fixed test literal, never input.
    let sql = format!("SELECT count(*) FROM {table} WHERE project_id = $1");
    sqlx::query_scalar::<_, i64>(&sql)
        .bind(scope.project_id())
        .fetch_one(pool)
        .await
        .unwrap()
}
