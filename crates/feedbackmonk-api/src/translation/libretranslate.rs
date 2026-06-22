#![allow(clippy::doc_markdown)] // module-doc names product/types verbatim (DeepL/LibreTranslate)
//! LibreTranslate adapter (FR-FBR-30 future-item #2, DEC-FBR-IMPL-26).
//!
//! The **no-egress** provider option: LibreTranslate is AGPL and self-hostable,
//! so a strict self-hoster can run it inside their own deployment (or point at a
//! private instance) and enable translation with **zero data leaving their
//! infrastructure** — removing the GDPR-data-processor obligation that the cloud
//! DeepL path carries. This adapter talks to an operator-supplied LibreTranslate
//! HTTP endpoint; bundling a LibreTranslate service into the self-host compose
//! is a separate follow-up (the operator supplies the URL for now).
//!
//! API: `POST {base}/translate` with a JSON body `{ q, source: "auto", target,
//! format: "text", api_key? }`. With `source: "auto"` the response carries
//! `detectedLanguage.language`, so language detection comes free (as with DeepL).

use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::{TranslateOutput, TranslationProvider};

const LIBRETRANSLATE_TIMEOUT_SECS: u64 = 30;

/// LibreTranslate translation provider (`reqwest` 0.12, rustls).
pub struct LibreTranslateTranslator {
    /// Fully-formed translate endpoint, e.g. `http://libretranslate:5000/translate`.
    endpoint: String,
    /// Optional API key (some instances require one; many self-host instances
    /// run open). Sent as the `api_key` body field when present.
    api_key: Option<String>,
    client: reqwest::Client,
}

impl LibreTranslateTranslator {
    /// Construct from a base URL (e.g. `http://libretranslate:5000`) and an
    /// optional API key. A trailing `/translate` is appended if the base does
    /// not already end in it.
    ///
    /// # Errors
    /// Returns an error if the `reqwest` client cannot be built.
    pub fn new(base_url: &str, api_key: Option<String>) -> anyhow::Result<Self> {
        let trimmed = base_url.trim().trim_end_matches('/');
        let endpoint = if trimmed.ends_with("/translate") {
            trimmed.to_string()
        } else {
            format!("{trimmed}/translate")
        };
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(LIBRETRANSLATE_TIMEOUT_SECS))
            .build()?;
        Ok(Self {
            endpoint,
            api_key: api_key.filter(|k| !k.trim().is_empty()),
            client,
        })
    }
}

#[derive(Deserialize)]
struct LibreTranslateResponse {
    #[serde(rename = "translatedText")]
    translated_text: String,
    #[serde(rename = "detectedLanguage")]
    detected_language: Option<DetectedLanguage>,
}

#[derive(Deserialize)]
struct DetectedLanguage {
    language: String,
}

#[async_trait]
impl TranslationProvider for LibreTranslateTranslator {
    async fn translate(&self, text: &str, target_lang: &str) -> anyhow::Result<TranslateOutput> {
        // LibreTranslate uses lowercase ISO-639-1 codes; `source: "auto"` makes
        // it detect + return the source language.
        let target = target_lang.trim().to_ascii_lowercase();
        let mut body = json!({
            "q": text,
            "source": "auto",
            "target": target,
            "format": "text",
        });
        if let Some(key) = &self.api_key {
            body["api_key"] = json!(key);
        }

        let resp = self
            .client
            .post(&self.endpoint)
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            // Do NOT echo the response body (it may quote the submitted text).
            anyhow::bail!("LibreTranslate returned HTTP {status}");
        }

        let parsed: LibreTranslateResponse = resp.json().await?;
        // `detectedLanguage` is present with source=auto; if an instance omits
        // it, report an empty source so the worker writes the translation rather
        // than mis-marking the row `skipped`.
        let detected_source_lang = parsed
            .detected_language
            .map(|d| d.language)
            .unwrap_or_default();

        Ok(TranslateOutput {
            translated: parsed.translated_text,
            detected_source_lang,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_translate_path_when_absent() {
        let t = LibreTranslateTranslator::new("http://lt:5000", None).unwrap();
        assert_eq!(t.endpoint, "http://lt:5000/translate");
    }

    #[test]
    fn keeps_translate_path_when_present_and_trims_slash() {
        let t = LibreTranslateTranslator::new("http://lt:5000/translate/", None).unwrap();
        assert_eq!(t.endpoint, "http://lt:5000/translate");
    }

    #[test]
    fn blank_api_key_becomes_none() {
        let t = LibreTranslateTranslator::new("http://lt:5000", Some("  ".into())).unwrap();
        assert!(t.api_key.is_none());
    }
}
