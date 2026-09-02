//! Admin full-text search × status filter (GitCellar parity gap #3 follow-up).
//!
//! `search_for_admin` gained an optional `status_filter` so the admin UI's
//! status pills compose with a live query instead of being silently ignored
//! while one is active. These tests pin the composition at the repository
//! layer (the sole query path, DEC-FBR-03):
//!
//!   - `None` keeps the pre-existing behaviour: every FTS hit in scope.
//!   - `Some(status)` intersects FTS hits with that status — both the page
//!     and the `total` count.
//!   - the filter never widens the result past the tenant/project scope.
//!
//! Lives in its own integration-test crate (like `me_feedback_repo_isolation`)
//! to stay out of the way of collaborators co-touching `feedback.rs`.

use feedbackmonk_core::{FeedbackKind, FeedbackStatus};
use feedbackmonk_repository::{
    FeedbackRepo, ProjectRepo, ProjectScope, SqlxFeedbackRepo, SqlxProjectRepo, SqlxTenantRepo,
    TenantRepo,
};
use sqlx::PgPool;

async fn seed_project_scope(pool: &PgPool, email: &str) -> ProjectScope {
    let trepo = SqlxTenantRepo::new(pool.clone());
    let prepo = SqlxProjectRepo::new(pool.clone());
    let t = trepo.create(email, "h").await.unwrap();
    let scope = trepo.scope_for(t.id).await.unwrap();
    let p = prepo
        .create(&scope, "Proj", &format!("p-{}", &t.id.to_string()[..8]))
        .await
        .unwrap();
    prepo.open(&scope, p.id).await.unwrap()
}

/// Two rows mention "safari" (one submitted, one triaged), one does not.
async fn seed_corpus(repo: &SqlxFeedbackRepo, pool: &PgPool, scope: &ProjectScope) {
    let triaged = repo
        .submit_anonymous(scope, &[1u8; 32], None, "Login button dead on mobile Safari", None, FeedbackKind::Bug)
        .await
        .unwrap();
    repo.submit_anonymous(scope, &[2u8; 32], None, "Safari renders the roadmap blank", None, FeedbackKind::Bug)
        .await
        .unwrap();
    repo.submit_anonymous(scope, &[3u8; 32], None, "Dark mode please", None, FeedbackKind::Feature)
        .await
        .unwrap();

    // Promote one hit to `triaged` through the same column the filter reads.
    sqlx::query("UPDATE feedback SET status = 'triaged' WHERE short_code = $1")
        .bind(triaged.as_str())
        .execute(pool)
        .await
        .unwrap();
}

#[sqlx::test(migrations = "../../migrations")]
async fn no_status_filter_returns_every_fts_hit(pool: PgPool) {
    let repo = SqlxFeedbackRepo::new(pool.clone());
    let scope = seed_project_scope(&pool, "search-all@example.com").await;
    seed_corpus(&repo, &pool, &scope).await;

    let (items, total) = repo.search_for_admin(&scope, "safari", None, 50, 0).await.unwrap();
    assert_eq!(total, 2, "both safari rows counted");
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|i| i.body_excerpt.to_lowercase().contains("safari")));
}

#[sqlx::test(migrations = "../../migrations")]
async fn status_filter_intersects_fts_hits_in_page_and_total(pool: PgPool) {
    let repo = SqlxFeedbackRepo::new(pool.clone());
    let scope = seed_project_scope(&pool, "search-status@example.com").await;
    seed_corpus(&repo, &pool, &scope).await;

    let (items, total) = repo
        .search_for_admin(&scope, "safari", Some(FeedbackStatus::Triaged), 50, 0)
        .await
        .unwrap();
    assert_eq!(total, 1, "only the triaged safari row counted");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].status, FeedbackStatus::Triaged);
    assert!(items[0].body_excerpt.contains("Login button"));

    // A status with FTS hits but no rows in that status → empty, not an error.
    let (items, total) = repo
        .search_for_admin(&scope, "safari", Some(FeedbackStatus::Shipped), 50, 0)
        .await
        .unwrap();
    assert_eq!(total, 0);
    assert!(items.is_empty());

    // A status match with no FTS match is still nothing (filter never widens).
    let (items, total) = repo
        .search_for_admin(&scope, "zebra", Some(FeedbackStatus::Triaged), 50, 0)
        .await
        .unwrap();
    assert_eq!(total, 0);
    assert!(items.is_empty());
}

#[sqlx::test(migrations = "../../migrations")]
async fn status_filter_stays_within_project_scope(pool: PgPool) {
    let repo = SqlxFeedbackRepo::new(pool.clone());
    let s1 = seed_project_scope(&pool, "search-ct1@example.com").await;
    let s2 = seed_project_scope(&pool, "search-ct2@example.com").await;
    seed_corpus(&repo, &pool, &s1).await;

    let (items, total) = repo
        .search_for_admin(&s2, "safari", Some(FeedbackStatus::Triaged), 50, 0)
        .await
        .unwrap();
    assert_eq!(total, 0, "another tenant's triaged hit must not leak");
    assert!(items.is_empty());
}
