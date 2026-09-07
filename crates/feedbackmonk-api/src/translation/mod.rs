#![allow(clippy::doc_markdown)] // module-doc names product/types verbatim (DeepL/LibreTranslate)
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
//!
//! ## Two consumers, two different strings (FR-FBR-30 vs FR-FBR-40)
//!
//! - **Inbound** ([`worker`], FR-FBR-30): the *submitter's* feedback body →
//!   the canonical content language, PERSISTED beside the verbatim original.
//! - **Outbound** ([`translate_to`], FR-FBR-40): the *team's* status note or
//!   public reply → the submitter's own language, for one email, persisted
//!   nowhere.
//!
//! They share a provider and nothing else. The outbound path reads and writes no
//! feedback column at all, which is what keeps the Q24 read-isolation invariant
//! (and its oracle) untouched by FR-FBR-40.

pub mod deepl;
pub mod libretranslate;
pub mod noop;
pub mod worker;

use async_trait::async_trait;

use feedbackmonk_i18n::Locale;

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

/// The provider-native target code for a shipped locale, or `None` when the
/// provider has no target for it.
///
/// The C34 table's `deepl` column is the single vocabulary both providers are
/// driven with — the same one `FEEDBACKMONK_TRANSLATION_TARGET_LANG` already
/// carries into the FR-FBR-30 worker, and `LibreTranslateTranslator` lowercases
/// what it is handed. Five locales (`ga`, `fa`, `ml`, `is`, `si`) carry `None`
/// **by design**: no target code means no translation, which per FR-FBR-40 means
/// the recipient reads the original — never an error, never a blocked send.
#[must_use]
pub fn provider_target_code(target: Locale) -> Option<&'static str> {
    target.entry().deepl
}

/// Translate one piece of **outbound, team-authored** text into `target`
/// (FR-FBR-40).
///
/// Distinct from the FR-FBR-30 inbound pipeline in every way that matters: that
/// one translates a *submitter's* feedback into the canonical content language
/// and persists it; this one translates the *team's* status note or public reply
/// for one email and persists nothing. No database column is read or written
/// here — in particular this path never touches the inbound translation column,
/// whose readers are allow-listed by the `translation-egress-q24-isolation`
/// oracle (Probe B). Conflating the two is the mistake this comment exists to
/// prevent.
///
/// `Ok(None)` means "no usable translation, send the original" and is a normal
/// outcome, not a degraded one:
///   - the locale has no provider target code ([`provider_target_code`]),
///   - the provider returned an empty string,
///   - the provider returned the input unchanged (already in the target
///     language, or a no-op provider).
///
/// `Err` is reserved for a genuine provider/transport failure, which the caller
/// logs and then sends the original for.
///
/// # Errors
/// Propagates the provider's error verbatim.
pub async fn translate_to(
    provider: &dyn TranslationProvider,
    text: &str,
    target: Locale,
) -> anyhow::Result<Option<String>> {
    let Some(code) = provider_target_code(target) else {
        return Ok(None);
    };
    let out = provider.translate(text, code).await?;
    let translated = out.translated.trim();
    if translated.is_empty() || translated == text.trim() {
        return Ok(None);
    }
    Ok(Some(out.translated))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake(anyhow::Result<TranslateOutput>);

    #[async_trait]
    impl TranslationProvider for Fake {
        async fn translate(&self, _text: &str, _target: &str) -> anyhow::Result<TranslateOutput> {
            match &self.0 {
                Ok(o) => Ok(o.clone()),
                Err(e) => Err(anyhow::anyhow!("{e}")),
            }
        }
    }

    fn out(s: &str) -> TranslateOutput {
        TranslateOutput {
            translated: s.to_string(),
            detected_source_lang: "EN".into(),
        }
    }

    #[test]
    fn target_code_comes_from_the_c34_table() {
        assert_eq!(provider_target_code(Locale::parse("de").unwrap()), Some("DE"));
        assert_eq!(
            provider_target_code(Locale::parse("pt-BR").unwrap()),
            Some("PT-BR")
        );
        // The five locales DeepL has no target for — English by design.
        for code in ["ga", "fa", "ml", "is", "si"] {
            assert_eq!(
                provider_target_code(Locale::parse(code).unwrap()),
                None,
                "{code} must have no provider target"
            );
        }
    }

    #[tokio::test]
    async fn no_provider_code_is_ok_none_not_an_error() {
        let p = Fake(Ok(out("should never be called")));
        let fa = Locale::parse("fa").unwrap();
        assert_eq!(translate_to(&p, "Wir arbeiten daran.", fa).await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_real_translation_comes_back() {
        let p = Fake(Ok(out("Wir arbeiten daran.")));
        let de = Locale::parse("de").unwrap();
        assert_eq!(
            translate_to(&p, "We're working on it.", de).await.unwrap(),
            Some("Wir arbeiten daran.".to_string())
        );
    }

    #[tokio::test]
    async fn empty_or_unchanged_output_is_ok_none() {
        let de = Locale::parse("de").unwrap();
        assert_eq!(translate_to(&Fake(Ok(out("   "))), "hello", de).await.unwrap(), None);
        assert_eq!(translate_to(&Fake(Ok(out("hello"))), "hello", de).await.unwrap(), None);
    }

    #[tokio::test]
    async fn provider_failure_is_an_error_for_the_caller_to_swallow() {
        let p = Fake(Err(anyhow::anyhow!("HTTP 503")));
        let de = Locale::parse("de").unwrap();
        assert!(translate_to(&p, "hello", de).await.is_err());
    }
}
