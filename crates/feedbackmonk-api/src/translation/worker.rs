#![allow(clippy::doc_markdown)] // module-doc names product/types verbatim (DeepL/LibreTranslate)
//! Async translate-after-accept worker (FR-FBR-30, DEC-FBR-IMPL-25 D3).
//!
//! A background poll-loop — modelled on `roadmap_voting_cache::spawn_refresh_tick`
//! (the `spawn_voting_cache_refresh` precedent): fire every N seconds, claim a
//! batch of `pending` (and bounded-retry `failed`) rows, translate each off the
//! request path, and write the result back. It NEVER blocks or touches the
//! public submit path (DEC-FBR-IMPL-25 D3) — submit only STAMPS `pending`; this
//! worker drains the queue.
//!
//! Failure tolerance (voting-cache + clustering-best-effort precedent): a claim
//! error logs WARN and waits for the next tick; a per-row provider error marks
//! that row `failed` (re-pollable until the attempts cap) and continues with the
//! next row. A provider outage is therefore never user-visible — every consumer
//! falls back to the verbatim `body` while a row is un-translated.
//!
//! Spawned in `main.rs` ONLY when `build_translation_provider()` returned `Some`
//! (provider ≠ off), exactly the `let _tick = spawn(...)` voting-cache pattern
//! (the JoinHandle is intentionally not held — process exit aborts the task).

use std::sync::Arc;
use std::time::Duration;

use feedbackmonk_repository::FeedbackRepo;
use tracing::warn;

use super::TranslationProvider;

/// Default poll interval (`FEEDBACKMONK_TRANSLATION_POLL_SECS`). Short enough
/// that translations land promptly for the admin/analyst, long enough not to
/// hammer the provider. The loop is non-overlapping (awaits the batch before the
/// next tick), so this is also the floor on batch spacing.
pub const DEFAULT_TRANSLATION_POLL_SECS: u64 = 15;

/// Default canonical target language (`FEEDBACKMONK_TRANSLATION_TARGET_LANG`).
/// English for v1 (DEC-FBR-IMPL-25); the machine consumers genuinely need it.
pub const DEFAULT_TRANSLATION_TARGET_LANG: &str = "EN";

/// Rows claimed per tick. Bounded so one tick can't monopolise the provider or
/// hold a long-running batch; the worklist drains across successive ticks.
pub const TRANSLATION_BATCH_LIMIT: i64 = 32;

/// Bounded auto-retry cap (DEC-FBR-IMPL-25 D2): a `failed` row is re-claimed
/// until it has failed this many times, then it falls out of the worklist
/// (terminal). Tolerates a transient provider blip without hammering on a
/// persistent outage.
pub const MAX_TRANSLATION_ATTEMPTS: i16 = 5;

/// Whether `detected` and `target` name the same language (primary subtag,
/// case-insensitive). `EN` ≡ `en` ≡ `EN-US` vs target `EN`. When true the worker
/// marks the row `skipped` instead of writing a translation.
#[must_use]
pub fn same_language(detected: &str, target: &str) -> bool {
    let primary = |s: &str| s.split('-').next().unwrap_or("").to_ascii_lowercase();
    !detected.is_empty() && primary(detected) == primary(target)
}

/// Spawn the long-running translate-after-accept tick. Fires every `poll_secs`.
/// The JoinHandle is returned for symmetry with `spawn_refresh_tick`; callers
/// typically drop it (process exit aborts the task).
#[must_use]
pub fn spawn_translation_worker(
    provider: Arc<dyn TranslationProvider>,
    feedback: Arc<dyn FeedbackRepo>,
    target_lang: String,
    poll_secs: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(poll_secs.max(1)));
        loop {
            interval.tick().await;
            translate_once(provider.as_ref(), feedback.as_ref(), &target_lang).await;
        }
    })
}

/// One drain pass — claim a batch and translate each row. Exposed for tests
/// (drive it directly with a fake provider + real repo instead of sleeping
/// through the interval). Tolerates per-row failure.
pub async fn translate_once(
    provider: &dyn TranslationProvider,
    feedback: &dyn FeedbackRepo,
    target_lang: &str,
) {
    let pending = match feedback
        .claim_pending_translations(MAX_TRANSLATION_ATTEMPTS, TRANSLATION_BATCH_LIMIT)
        .await
    {
        Ok(p) => p,
        Err(e) => {
            warn!(error = %e, "translation worker: claim failed; retrying next tick");
            return;
        }
    };

    for row in pending {
        match provider.translate(&row.body, target_lang).await {
            Ok(out) if same_language(&out.detected_source_lang, target_lang) => {
                // Already the canonical language — record the detection and skip
                // (reads fall back to the already-canonical `body`).
                if let Err(e) = feedback
                    .mark_translation_skipped(row.id, &out.detected_source_lang)
                    .await
                {
                    warn!(feedback_id = %row.id, error = %e, "translation worker: mark_skipped failed");
                }
            }
            Ok(out) => {
                if let Err(e) = feedback
                    .set_translation(row.id, &out.translated, &out.detected_source_lang)
                    .await
                {
                    warn!(feedback_id = %row.id, error = %e, "translation worker: set_translation failed");
                }
            }
            Err(e) => {
                // Provider error: mark failed (re-pollable until the attempts
                // cap). Do NOT log the body — only the id + error.
                warn!(feedback_id = %row.id, error = %e, "translation worker: provider error; marking failed");
                if let Err(e2) = feedback.mark_translation_failed(row.id).await {
                    warn!(feedback_id = %row.id, error = %e2, "translation worker: mark_failed failed");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_language_matches_primary_subtag_case_insensitively() {
        assert!(same_language("EN", "EN"));
        assert!(same_language("en", "EN"));
        assert!(same_language("EN-US", "EN"));
        assert!(same_language("en-GB", "en"));
        assert!(!same_language("DE", "EN"));
        assert!(!same_language("PT-BR", "EN"));
        assert!(!same_language("", "EN"));
    }
}
