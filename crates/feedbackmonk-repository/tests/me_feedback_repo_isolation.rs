//! Gap #4 (DELTA) — repository-level isolation tests for the end-user
//! (JWT-sub-scoped) read surface. Second leg of defense behind the
//! HTTP-level fixture (`feedbackmonk-api/tests/me_feedback_isolation.rs`).
//!
//! Lives in its own integration-test crate (rather than the inline
//! `feedback.rs` test module) to stay out of the way of the parallel
//! collaborators co-touching `feedback.rs` — exercises only the public
//! repository API.
//!
//! Invariants:
//!   - `list_for_end_user` returns ONLY the caller's own `end_user_sub` rows;
//!     anonymous rows are structurally excluded.
//!   - `get_for_end_user` returns `NotFound` for another sub's / an anonymous
//!     feedback id (never a leak).
//!   - cross-tenant sub collisions do not leak rows.
//!
//! NOTE: `submit_authenticated`'s 6th positional arg (`crash_event_id`,
//! parity gap #2) is `None` — these rows are not crash-linked.

use feedbackmonk_core::FeedbackKind;
use feedbackmonk_repository::{
    FeedbackRepo, ProjectRepo, ProjectScope, RepoError, SqlxFeedbackRepo, SqlxProjectRepo,
    SqlxTenantRepo, TenantRepo,
};
use sqlx::PgPool;

async fn seed_project_scope(pool: &PgPool, email: &str) -> ProjectScope {
    let trepo = SqlxTenantRepo::new(pool.clone());
    let prepo = SqlxProjectRepo::new(pool.clone());
    let t = trepo.create(email, "h").await.unwrap();
    let scope = trepo.scope_for(t.id).await.unwrap();
    let p = prepo.create(&scope, "Proj", &format!("p-{}", &t.id.to_string()[..8])).await.unwrap();
    prepo.open(&scope, p.id).await.unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn list_for_end_user_returns_only_own_sub_and_excludes_anon(pool: PgPool) {
    let repo = SqlxFeedbackRepo::new(pool.clone());
    let scope = seed_project_scope(&pool, "me-list@example.com").await;

    repo.submit_authenticated(&scope, "sub-A", Some("a@x.com"), None, None, None, "A-1", FeedbackKind::Bug).await.unwrap();
    repo.submit_authenticated(&scope, "sub-A", Some("a@x.com"), None, None, None, "A-2", FeedbackKind::Feature).await.unwrap();
    repo.submit_authenticated(&scope, "sub-B", Some("b@x.com"), None, None, None, "B-1", FeedbackKind::Bug).await.unwrap();
    repo.submit_anonymous(&scope, &[1u8; 32], None, "anon-1", FeedbackKind::Other).await.unwrap();

    let (items, total) = repo.list_for_end_user(&scope, "sub-A", 50, 0).await.unwrap();
    assert_eq!(total, 2, "only A's two rows counted (B + anon excluded)");
    assert_eq!(items.len(), 2);
    let bodies: Vec<&str> = items.iter().map(|i| i.body.as_str()).collect();
    assert!(bodies.contains(&"A-1") && bodies.contains(&"A-2"));
    assert!(!bodies.contains(&"B-1"), "another user's row leaked");
    assert!(!bodies.contains(&"anon-1"), "anonymous row leaked into JWT surface");
}

#[sqlx::test(migrations = "../../migrations")]
async fn get_for_end_user_rejects_other_sub_and_anon_as_not_found(pool: PgPool) {
    let repo = SqlxFeedbackRepo::new(pool.clone());
    let scope = seed_project_scope(&pool, "me-get@example.com").await;

    let b_fb = repo.submit_authenticated(&scope, "sub-B", Some("b@x.com"), None, None, None, "B-private", FeedbackKind::Bug).await.unwrap();
    let anon_fb = repo.submit_anonymous(&scope, &[2u8; 32], None, "anon-private", FeedbackKind::Other).await.unwrap();
    let a_fb = repo.submit_authenticated(&scope, "sub-A", Some("a@x.com"), None, None, None, "A-own", FeedbackKind::Bug).await.unwrap();

    let mine = repo.get_for_end_user(&scope, "sub-A", &a_fb).await.unwrap();
    assert_eq!(mine.feedback_id, a_fb);
    assert_eq!(mine.body, "A-own");

    assert!(matches!(repo.get_for_end_user(&scope, "sub-A", &b_fb).await.unwrap_err(), RepoError::NotFound));
    assert!(matches!(repo.get_for_end_user(&scope, "sub-A", &anon_fb).await.unwrap_err(), RepoError::NotFound));
}

#[sqlx::test(migrations = "../../migrations")]
async fn list_for_end_user_cross_tenant_returns_empty(pool: PgPool) {
    let repo = SqlxFeedbackRepo::new(pool.clone());
    let s1 = seed_project_scope(&pool, "me-ct1@example.com").await;
    let s2 = seed_project_scope(&pool, "me-ct2@example.com").await;
    repo.submit_authenticated(&s1, "shared-sub", Some("a@x.com"), None, None, None, "t1-row", FeedbackKind::Bug).await.unwrap();

    let (items, total) = repo.list_for_end_user(&s2, "shared-sub", 50, 0).await.unwrap();
    assert!(items.is_empty());
    assert_eq!(total, 0, "cross-tenant sub collision must not leak rows");
}
