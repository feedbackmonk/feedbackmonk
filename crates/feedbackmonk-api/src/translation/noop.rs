#![allow(clippy::doc_markdown)] // module-doc names product/types verbatim (DeepL/LibreTranslate)
//! No-op translation provider (FR-FBR-30, DEC-FBR-IMPL-26 — the default-off
//! contract type).
//!
//! Returns the input text unchanged and reports the detected source language as
//! equal to the target, which makes the worker mark the row `skipped` (no
//! translation needed). The translate-after-accept worker is NOT spawned when
//! the provider is off (`main.rs` returns `None`), so this is rarely constructed
//! in production — it exists so the `Arc<dyn TranslationProvider>` contract has a
//! concrete impl and so tests can exercise the already-target-language path.

use async_trait::async_trait;

use super::{TranslateOutput, TranslationProvider};

/// Translator that performs no translation.
pub struct NoOpTranslator;

#[async_trait]
impl TranslationProvider for NoOpTranslator {
    async fn translate(&self, text: &str, target_lang: &str) -> anyhow::Result<TranslateOutput> {
        Ok(TranslateOutput {
            translated: text.to_string(),
            detected_source_lang: target_lang.to_string(),
        })
    }
}
