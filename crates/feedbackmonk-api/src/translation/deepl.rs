#![allow(clippy::doc_markdown)] // module-doc names product/types verbatim (DeepL/LibreTranslate)
//! DeepL cloud translation adapter (FR-FBR-30, DEC-FBR-IMPL-26).
//!
//! v1's recommended cloud provider: EU-based, offers a DPA, and contractually
//! does not train on Pro-API text (the privacy rationale in DEC-FBR-IMPL-26 for
//! choosing DeepL over Google). DeepL AUTO-DETECTS the source language and
//! returns it in `detected_source_language`, so feedbackmonk needs no separate
//! language-detection dependency.
//!
//! Egress: this adapter makes an outbound HTTP call to DeepL with the feedback
//! body (personal data). That egress is a conscious, disclosed, opt-in choice —
//! the provider defaults OFF (DEC-FBR-IMPL-26) and the egress is documented in
//! `docs/operations/SELFHOST_ENV.md` (Contract C21). Do NOT flip the default or
//! hardcode this provider without re-opening DEC-FBR-IMPL-26.

use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;

use super::{TranslateOutput, TranslationProvider};

/// DeepL free-tier API keys end in `:fx` and must hit the free endpoint host;
/// Pro keys hit the Pro host. (DeepL routes by host, not by key prefix.)
const DEEPL_FREE_KEY_SUFFIX: &str = ":fx";
const DEEPL_PRO_ENDPOINT: &str = "https://api.deepl.com/v2/translate";
const DEEPL_FREE_ENDPOINT: &str = "https://api-free.deepl.com/v2/translate";
/// Bound the provider call so a hung DeepL request cannot stall the worker tick.
const DEEPL_TIMEOUT_SECS: u64 = 30;

/// DeepL translation provider (`reqwest` 0.12, rustls).
pub struct DeepLTranslator {
    api_key: String,
    endpoint: String,
    client: reqwest::Client,
}

impl DeepLTranslator {
    /// Construct a DeepL translator from an API key. The endpoint host is chosen
    /// from the key shape: free keys (`…:fx`) hit `api-free.deepl.com`, Pro keys
    /// hit `api.deepl.com`.
    ///
    /// # Errors
    /// Returns an error if the `reqwest` client cannot be built (TLS backend).
    pub fn new(api_key: String) -> anyhow::Result<Self> {
        let endpoint = if api_key.trim_end().ends_with(DEEPL_FREE_KEY_SUFFIX) {
            DEEPL_FREE_ENDPOINT
        } else {
            DEEPL_PRO_ENDPOINT
        };
        Self::with_endpoint(api_key, endpoint.to_string())
    }

    /// Construct with an explicit endpoint (used by tests to point at a mock
    /// server). Production code uses [`DeepLTranslator::new`].
    ///
    /// # Errors
    /// Returns an error if the `reqwest` client cannot be built.
    pub fn with_endpoint(api_key: String, endpoint: String) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(DEEPL_TIMEOUT_SECS))
            .build()?;
        Ok(Self {
            api_key,
            endpoint,
            client,
        })
    }
}

#[derive(Deserialize)]
struct DeepLResponse {
    translations: Vec<DeepLTranslation>,
}

#[derive(Deserialize)]
struct DeepLTranslation {
    detected_source_language: String,
    text: String,
}

#[async_trait]
impl TranslationProvider for DeepLTranslator {
    async fn translate(&self, text: &str, target_lang: &str) -> anyhow::Result<TranslateOutput> {
        // DeepL v2 `/translate` is a form-POST with the auth key in the
        // `DeepL-Auth-Key` Authorization scheme. `source_lang` is omitted so
        // DeepL auto-detects it (and returns `detected_source_language`).
        let resp = self
            .client
            .post(&self.endpoint)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("DeepL-Auth-Key {}", self.api_key),
            )
            .form(&[("text", text), ("target_lang", target_lang)])
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            // Surface the status; do NOT log the body verbatim (it may echo the
            // submitted text). The worker logs a scrubbed WARN and marks failed.
            anyhow::bail!("DeepL API returned HTTP {status}");
        }

        let parsed: DeepLResponse = resp.json().await?;
        let first = parsed
            .translations
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("DeepL response carried no translations"))?;

        Ok(TranslateOutput {
            translated: first.text,
            detected_source_lang: first.detected_source_language,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_key_routes_to_free_endpoint() {
        let t = DeepLTranslator::new("abc-123:fx".to_string()).unwrap();
        assert_eq!(t.endpoint, DEEPL_FREE_ENDPOINT);
    }

    #[test]
    fn pro_key_routes_to_pro_endpoint() {
        let t = DeepLTranslator::new("abc-123".to_string()).unwrap();
        assert_eq!(t.endpoint, DEEPL_PRO_ENDPOINT);
    }
}
