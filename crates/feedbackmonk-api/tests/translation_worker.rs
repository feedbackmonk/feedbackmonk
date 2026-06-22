#![allow(clippy::doc_markdown)] // test-file doc comments name columns/types verbatim
//! FR-FBR-30 — translate-after-accept worker integration (Stream C + Stream D).
//!
//! Drives `translate_once` with a deterministic `FakeTranslator` (the
//! `TranslationProvider` trait is the test seam the Testability Gate identified —
//! the real DeepL adapter is non-deterministic and exercised only behind a
//! `--full`/ignored smoke) against a real Postgres (`sqlx::test`). Verifies all
//! three per-row outcomes — translated / skipped / failed — THROUGH the real
//! Stream-D consumer read (`list_member_bodies_for_cluster`), never via raw SQL
//! (forbidden outside the repository crate by `multi-tenant-isolation-check`).
//!
//! Rows are stamped `pending` at submit by enabling the process-global flag
//! (this test owns it; each `#[sqlx::test]` has an isolated DB, so the flag only
//! affects this test's own rows).

use async_trait::async_trait;

use feedbackmonk_api::translation::{translate_once, TranslateOutput, TranslationProvider};
use feedbackmonk_core::FeedbackKind;
use feedbackmonk_repository::{
    ClusterRepo, FeedbackRepo, ProjectRepo, ProjectScope, SqlxClusterRepo, SqlxFeedbackRepo,
    SqlxProjectRepo, SqlxTenantRepo, TenantRepo, TranslationFlag,
};
use sqlx::PgPool;

/// Deterministic fake. `"DE text"` → translated (detected `DE`); `"EN text"` →
/// detected == target (worker marks `skipped`); `"ERR text"` → provider error
/// (worker marks `failed`).
struct FakeTranslator;

#[async_trait]
impl TranslationProvider for FakeTranslator {
    async fn translate(&self, text: &str, target_lang: &str) -> anyhow::Result<TranslateOutput> {
        match text {
            "ERR text" => anyhow::bail!("fake provider error"),
            "EN text" => Ok(TranslateOutput {
                translated: text.to_string(),
                detected_source_lang: target_lang.to_string(),
            }),
            other => Ok(TranslateOutput {
                translated: format!("[EN] {other}"),
                detected_source_lang: "DE".to_string(),
            }),
        }
    }
}

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

#[sqlx::test(migrations = "../../migrations")]
async fn worker_translates_skips_and_fails_per_row(pool: PgPool) {
    TranslationFlag::set(true); // stamp 'pending' at submit (isolated DB)

    let frepo = SqlxFeedbackRepo::new(pool.clone());
    let crepo = SqlxClusterRepo::new(pool.clone());
    let scope = seed_project_scope(&pool, "xlate-worker@example.com").await;

    let de = frepo
        .submit_anonymous(&scope, &[1u8; 32], None, "DE text", None, FeedbackKind::Bug)
        .await
        .unwrap();
    let en = frepo
        .submit_anonymous(&scope, &[2u8; 32], None, "EN text", None, FeedbackKind::Bug)
        .await
        .unwrap();
    let err = frepo
        .submit_anonymous(&scope, &[3u8; 32], None, "ERR text", None, FeedbackKind::Bug)
        .await
        .unwrap();

    // Put all three in one cluster so the Stream-D consumer read can observe the
    // worker's effect.
    let cluster = crepo
        .create(&scope, "label", Some("summary"), FeedbackKind::Bug, "admin")
        .await
        .unwrap();
    for fb in [&de, &en, &err] {
        frepo.set_cluster_id(&scope, fb, Some(cluster.id)).await.unwrap();
    }

    // Sanity: all three are claimable before the drain.
    assert_eq!(
        frepo.claim_pending_translations(5, 100).await.unwrap().len(),
        3
    );

    // One drain pass.
    translate_once(&FakeTranslator, &frepo, "EN").await;

    // Consumer read reflects: DE → English translation; EN → skipped (falls back
    // to original); ERR → failed (falls back to original).
    let mut bodies = frepo
        .list_member_bodies_for_cluster(&scope, cluster.id, 100)
        .await
        .unwrap();
    bodies.sort();
    assert_eq!(
        bodies,
        vec![
            "EN text".to_string(),     // skipped → fallback to original
            "ERR text".to_string(),    // failed  → fallback to original
            "[EN] DE text".to_string() // translated
        ]
    );

    // The translated + skipped rows have drained; only the failed row remains
    // claimable (attempts 1 < cap).
    let remaining = frepo.claim_pending_translations(5, 100).await.unwrap();
    assert_eq!(remaining.len(), 1, "only the failed row stays in the worklist");
    assert_eq!(remaining[0].body, "ERR text");
}

#[sqlx::test(migrations = "../../migrations")]
async fn drain_is_idempotent_when_worklist_empty(pool: PgPool) {
    // No pending rows → translate_once is a no-op and does not error.
    let frepo = SqlxFeedbackRepo::new(pool.clone());
    let _scope = seed_project_scope(&pool, "xlate-empty@example.com").await;
    translate_once(&FakeTranslator, &frepo, "EN").await;
    assert!(frepo.claim_pending_translations(5, 100).await.unwrap().is_empty());
}
