#![allow(clippy::doc_markdown)] // test-file doc comments name columns/types verbatim
//! FR-FBR-30 — translate-after-accept worklist + store-both repository tests.
//!
//! Exercises the four worklist methods (`claim_pending_translations`,
//! `set_translation`, `mark_translation_skipped`, `mark_translation_failed`),
//! the submit-time `pending` stamping gate (`TranslationFlag`), and the
//! Stream-D consumer read (`list_member_bodies_for_cluster` reads the translation
//! with fallback to the verbatim original). Lives in the repository integration
//! crate so it can read the `body_translated` / `source_lang` / `translation_status`
//! columns directly via raw SQL (permitted inside the repository crate; DEC-FBR-03).
//!
//! Worklist-mechanics tests set `translation_status` explicitly via raw UPDATE so
//! they do not depend on the process-global enablement flag (which other tests in
//! the same process may have flipped). The dedicated stamping test owns the flag.
//!
//! NOTE: each `#[sqlx::test]` gets its OWN isolated database, so the global
//! claim query only ever sees the current test's rows.

use feedbackmonk_core::{FeedbackKind, Sentiment};
use feedbackmonk_repository::{
    ClusterRepo, FeedbackRepo, ProjectRepo, ProjectScope, RepoError, SqlxClusterRepo,
    SqlxFeedbackRepo, SqlxProjectRepo, SqlxTenantRepo, TenantRepo, TranslationFlag,
};
use sqlx::PgPool;
use uuid::Uuid;

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

/// Force a project's feedback rows into a given translation_status (raw UPDATE —
/// repository crate is allowed raw SQL). Returns nothing.
async fn force_status(pool: &PgPool, scope: &ProjectScope, status: &str) {
    sqlx::query("UPDATE feedback SET translation_status = $2 WHERE project_id = $1")
        .bind(scope.project_id())
        .bind(status)
        .execute(pool)
        .await
        .unwrap();
}

/// Read (body_translated, source_lang, translation_status, translation_attempts)
/// for a feedback row by its UUID.
async fn read_translation(
    pool: &PgPool,
    id: Uuid,
) -> (Option<String>, Option<String>, Option<String>, i16) {
    let r = sqlx::query!(
        "SELECT body_translated, source_lang, translation_status, translation_attempts \
         FROM feedback WHERE id = $1",
        id
    )
    .fetch_one(pool)
    .await
    .unwrap();
    (
        r.body_translated,
        r.source_lang,
        r.translation_status,
        r.translation_attempts,
    )
}

#[sqlx::test(migrations = "../../migrations")]
async fn claim_set_translation_round_trips(pool: PgPool) {
    let repo = SqlxFeedbackRepo::new(pool.clone());
    let scope = seed_project_scope(&pool, "xlate-set@example.com").await;

    repo.submit_anonymous(&scope, &[1u8; 32], None, "Hallo Welt", None, FeedbackKind::Bug)
        .await
        .unwrap();
    // Make it a worklist row regardless of the global flag.
    force_status(&pool, &scope, "pending").await;

    let claimed = repo.claim_pending_translations(5, 100).await.unwrap();
    assert_eq!(claimed.len(), 1, "the pending row is claimed");
    assert_eq!(claimed[0].body, "Hallo Welt");

    repo.set_translation(claimed[0].id, "Hello World", "DE")
        .await
        .unwrap();

    let (translated, lang, status, attempts) = read_translation(&pool, claimed[0].id).await;
    assert_eq!(translated.as_deref(), Some("Hello World"));
    assert_eq!(lang.as_deref(), Some("DE"));
    assert_eq!(status.as_deref(), Some("translated"));
    assert_eq!(attempts, 0);

    // Verbatim body is NEVER overwritten (Q24).
    let body = sqlx::query!("SELECT body FROM feedback WHERE id = $1", claimed[0].id)
        .fetch_one(&pool)
        .await
        .unwrap()
        .body;
    assert_eq!(body.as_deref(), Some("Hallo Welt"), "original body preserved");

    // A translated row is no longer in the worklist.
    assert!(repo.claim_pending_translations(5, 100).await.unwrap().is_empty());
}

#[sqlx::test(migrations = "../../migrations")]
async fn skip_marks_status_and_leaves_translation_null(pool: PgPool) {
    let repo = SqlxFeedbackRepo::new(pool.clone());
    let scope = seed_project_scope(&pool, "xlate-skip@example.com").await;
    repo.submit_anonymous(&scope, &[2u8; 32], None, "Already English", None, FeedbackKind::Bug)
        .await
        .unwrap();
    force_status(&pool, &scope, "pending").await;

    let claimed = repo.claim_pending_translations(5, 100).await.unwrap();
    repo.mark_translation_skipped(claimed[0].id, "EN").await.unwrap();

    let (translated, lang, status, _) = read_translation(&pool, claimed[0].id).await;
    assert_eq!(translated, None, "skipped rows keep body_translated NULL");
    assert_eq!(lang.as_deref(), Some("EN"));
    assert_eq!(status.as_deref(), Some("skipped"));
    assert!(repo.claim_pending_translations(5, 100).await.unwrap().is_empty());
}

#[sqlx::test(migrations = "../../migrations")]
async fn failed_rows_retry_until_attempts_cap(pool: PgPool) {
    let repo = SqlxFeedbackRepo::new(pool.clone());
    let scope = seed_project_scope(&pool, "xlate-fail@example.com").await;
    repo.submit_anonymous(&scope, &[3u8; 32], None, "Bonjour", None, FeedbackKind::Bug)
        .await
        .unwrap();
    force_status(&pool, &scope, "pending").await;

    let id = repo.claim_pending_translations(5, 100).await.unwrap()[0].id;

    // Fail it twice — still re-claimable under a cap of 3.
    repo.mark_translation_failed(id).await.unwrap();
    repo.mark_translation_failed(id).await.unwrap();
    let (_, _, status, attempts) = read_translation(&pool, id).await;
    assert_eq!(status.as_deref(), Some("failed"));
    assert_eq!(attempts, 2);
    assert_eq!(
        repo.claim_pending_translations(3, 100).await.unwrap().len(),
        1,
        "a failed row below the cap is re-claimable"
    );

    // Once attempts reach the cap, it falls out of the worklist (terminal).
    assert!(
        repo.claim_pending_translations(2, 100).await.unwrap().is_empty(),
        "attempts (2) >= cap (2) → no longer claimed"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn claim_excludes_sentiment_only_and_translated(pool: PgPool) {
    let repo = SqlxFeedbackRepo::new(pool.clone());
    let scope = seed_project_scope(&pool, "xlate-exclude@example.com").await;

    // A sentiment-only (NULL body) row: even if forced pending, it carries no
    // text and must never be claimed.
    repo.submit_anonymous(&scope, &[4u8; 32], None, "", Some(Sentiment::Positive), FeedbackKind::Other)
        .await
        .unwrap();
    force_status(&pool, &scope, "pending").await;

    let claimed = repo.claim_pending_translations(5, 100).await.unwrap();
    assert!(claimed.is_empty(), "sentiment-only (NULL body) rows are never claimed");
}

#[sqlx::test(migrations = "../../migrations")]
async fn submit_stamps_pending_only_when_enabled_and_body_present(pool: PgPool) {
    let repo = SqlxFeedbackRepo::new(pool.clone());
    let scope = seed_project_scope(&pool, "xlate-stamp@example.com").await;

    // Enablement gate ON (this test owns the global flag; isolated DB).
    TranslationFlag::set(true);
    let with_body = repo
        .submit_anonymous(&scope, &[5u8; 32], None, "Guten Tag", None, FeedbackKind::Bug)
        .await
        .unwrap();
    // Sentiment-only: enabled, but no body → NOT stamped.
    repo.submit_anonymous(&scope, &[6u8; 32], None, "", Some(Sentiment::Neutral), FeedbackKind::Other)
        .await
        .unwrap();

    let claimed = repo.claim_pending_translations(5, 100).await.unwrap();
    assert_eq!(claimed.len(), 1, "only the body-bearing row is stamped pending");
    assert_eq!(claimed[0].body, "Guten Tag");

    // Sanity: the body-bearing row's status is exactly 'pending'.
    let status = sqlx::query!(
        "SELECT translation_status FROM feedback WHERE short_code = $1",
        with_body.as_str()
    )
    .fetch_one(&pool)
    .await
    .unwrap()
    .translation_status;
    assert_eq!(status.as_deref(), Some("pending"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn cluster_member_bodies_read_translation_with_fallback(pool: PgPool) {
    // Stream D: the analyst/clustering consumer reads body_translated when
    // present, falling back to the verbatim body otherwise.
    let frepo = SqlxFeedbackRepo::new(pool.clone());
    let crepo = SqlxClusterRepo::new(pool.clone());
    let scope = seed_project_scope(&pool, "xlate-cluster@example.com").await;

    // Two rows: one will be translated, one left untranslated.
    let translated_fb = frepo
        .submit_anonymous(&scope, &[7u8; 32], None, "Hallo", None, FeedbackKind::Bug)
        .await
        .unwrap();
    let untranslated_fb = frepo
        .submit_anonymous(&scope, &[8u8; 32], None, "Plain English", None, FeedbackKind::Bug)
        .await
        .unwrap();

    let cluster = crepo
        .create(&scope, "label", Some("summary"), FeedbackKind::Bug, "admin")
        .await
        .unwrap();
    frepo
        .set_cluster_id(&scope, &translated_fb, Some(cluster.id))
        .await
        .unwrap();
    frepo
        .set_cluster_id(&scope, &untranslated_fb, Some(cluster.id))
        .await
        .unwrap();

    // Translate the first row.
    let translated_id = sqlx::query!(
        "SELECT id FROM feedback WHERE short_code = $1",
        translated_fb.as_str()
    )
    .fetch_one(&pool)
    .await
    .unwrap()
    .id;
    frepo.set_translation(translated_id, "Hello", "DE").await.unwrap();

    let bodies = frepo
        .list_member_bodies_for_cluster(&scope, cluster.id, 100)
        .await
        .unwrap();
    assert!(bodies.contains(&"Hello".to_string()), "translated row read as English");
    assert!(
        bodies.contains(&"Plain English".to_string()),
        "untranslated row falls back to the original body"
    );
    assert!(
        !bodies.contains(&"Hallo".to_string()),
        "the verbatim original of a translated row is NOT what the consumer reads"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn backfill_stamps_null_body_bearing_rows_in_bounded_batches(pool: PgPool) {
    // FR-FBR-30 #5: lazy backfill leaves pre-feature rows NULL; backfill stamps
    // them pending. Force every row's status back to NULL after submit so the
    // setup is deterministic regardless of the process-global flag (a sibling
    // test may have flipped it ON).
    let repo = SqlxFeedbackRepo::new(pool.clone());
    let scope = seed_project_scope(&pool, "xlate-backfill@example.com").await;

    // 3 body-bearing rows + 1 sentiment-only (NULL body).
    for i in 0..3u8 {
        repo.submit_anonymous(&scope, &[i; 32], None, "Hallo Welt", None, FeedbackKind::Bug)
            .await
            .unwrap();
    }
    repo.submit_anonymous(&scope, &[9u8; 32], None, "", Some(Sentiment::Positive), FeedbackKind::Other)
        .await
        .unwrap();
    // Pre-feature baseline: translation_status IS NULL for all rows.
    sqlx::query("UPDATE feedback SET translation_status = NULL WHERE project_id = $1")
        .bind(scope.project_id())
        .execute(&pool)
        .await
        .unwrap();

    // Bounded batch of 2 → stamps 2; nothing claimable before backfill.
    assert!(repo.claim_pending_translations(5, 100).await.unwrap().is_empty());
    assert_eq!(repo.backfill_pending_translations(2).await.unwrap(), 2);
    assert_eq!(repo.claim_pending_translations(5, 100).await.unwrap().len(), 2);

    // Next batch stamps the remaining body-bearing row (the sentiment-only row
    // is body-less → never stamped). Then 0 left.
    assert_eq!(repo.backfill_pending_translations(100).await.unwrap(), 1);
    assert_eq!(repo.backfill_pending_translations(100).await.unwrap(), 0);
    assert_eq!(repo.claim_pending_translations(5, 100).await.unwrap().len(), 3);
}

#[sqlx::test(migrations = "../../migrations")]
async fn get_translation_for_admin_returns_view_and_rejects_cross_scope(pool: PgPool) {
    // FR-FBR-30 #3: the admin (controller) read of the translation facets.
    let repo = SqlxFeedbackRepo::new(pool.clone());
    let scope = seed_project_scope(&pool, "xlate-admin1@example.com").await;
    let other = seed_project_scope(&pool, "xlate-admin2@example.com").await;

    let fb = repo
        .submit_anonymous(&scope, &[1u8; 32], None, "Hallo", None, FeedbackKind::Bug)
        .await
        .unwrap();

    // Untranslated → the translation itself is absent (these are NULL regardless
    // of whether the row was stamped `pending`, which only touches the status).
    let view = repo.get_translation_for_admin(&scope, &fb).await.unwrap();
    assert_eq!(view.body_translated, None);
    assert_eq!(view.source_lang, None);

    // After translation → facets populated; body NOT part of the view.
    let id = sqlx::query!("SELECT id FROM feedback WHERE short_code = $1", fb.as_str())
        .fetch_one(&pool)
        .await
        .unwrap()
        .id;
    repo.set_translation(id, "Hello", "DE").await.unwrap();
    let view = repo.get_translation_for_admin(&scope, &fb).await.unwrap();
    assert_eq!(view.body_translated.as_deref(), Some("Hello"));
    assert_eq!(view.source_lang.as_deref(), Some("DE"));
    assert_eq!(view.translation_status.as_deref(), Some("translated"));

    // Cross-scope read → NotFound (never another tenant's translation).
    assert!(matches!(
        repo.get_translation_for_admin(&other, &fb).await.unwrap_err(),
        RepoError::NotFound
    ));
}
