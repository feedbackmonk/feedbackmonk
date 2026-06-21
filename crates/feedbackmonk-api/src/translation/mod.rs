//! Translation provider abstraction (FR-FBR-30, DEC-FBR-IMPL-26).
//!
//! Mirrors the `email::Mailer` trait + `main.rs::build_mailer()` precedent: a
//! `Send + Sync` trait decouples the translate-after-accept worker from the
//! concrete provider, an env-selected `build_translation_provider()` (in
//! `main.rs`) constructs the right impl, and test code substitutes a fake.
//!
//! Two concrete impls (DEC-FBR-IMPL-26 — pluggable, DEFAULT OFF):
//!   - [`DeepLTranslator`] — the v1 cloud provider (DeepL, EU; auto-detects the
//!     source language, so language detection comes free). A conscious, disclosed
//!     egress choice (`docs/operations/SELFHOST_ENV.md` Contract C21).
//!   - [`NoOpTranslator`] — the default-off contract type. In practice the worker
//!     is NOT spawned when the provider is off, so this is rarely constructed;
//!     it exists so the `Arc<dyn TranslationProvider>` contract has a concrete
//!     type and so tests can exercise the "already-target-language → skip" path.
//!
//! The worker that consumes this trait lives in [`worker`]. The provider is
//! NEVER called synchronously on the public submit path (DEC-FBR-IMPL-25 D3) —
//! only by the background worker, after a row is accepted.

pub mod deepl;
pub mod libretranslate;
pub mod noop;
pub mod worker;

use async_trait::async_trait;

pub use deepl::DeepLTranslator;
pub use libretranslate::LibreTranslateTranslator;
pub use noop::NoOpTranslator;
pub use worker::{
    same_language, spawn_translation_worker, translate_once, DEFAULT_TRANSLATION_POLL_SECS,
    DEFAULT_TRANSLATION_TARGET_LANG, MAX_TRANSLATION_ATTEMPTS, TRANSLATION_BATCH_LIMIT,
};

/// Output of one translation call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslateOutput {
    /// The translated text, in the requested target language.
    pub translated: String,
    /// The provider-detected source language (BCP-47 / ISO-639-1, e.g. `DE`,
    /// `PT-BR`). When this equals the target language the worker marks the row
    /// `skipped` (no translation was needed) instead of writing `body_translated`.
    pub detected_source_lang: String,
}

/// A pluggable translation backend (DEC-FBR-IMPL-26). `Send + Sync` so it can be
/// shared across the worker's `Arc`. Implementations MUST NOT panic on provider
/// errors — return `Err` so the worker can mark the row `failed` and retry.
#[async_trait]
pub trait TranslationProvider: Send + Sync {
    /// Translate `text` into `target_lang`, returning the translation plus the
    /// detected source language. `target_lang` is a provider-native language
    /// code (e.g. DeepL's `EN`). Returns `Err` on any provider/transport error.
    async fn translate(&self, text: &str, target_lang: &str) -> anyhow::Result<TranslateOutput>;
}
