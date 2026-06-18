//! Outbound egress sanitizer (Contract C27, 25c — FR-FBR-25c). **THE
//! source-never-leaves chokepoint.**
//!
//! Every outbound payload the runner POSTs (the implementer's `result_ref`, the
//! analyst's recommendation bodies) passes through [`sanitize_outbound`] before
//! the HTTP call. It does three things: (a) reuses the canonical
//! `feedbackmonk_tracing::scrub` 20-pattern PII scrubber (the same oracle-guarded
//! chokepoint the rest of the workspace uses — FR-FBR-10); (b) applies a
//! secret-pattern denylist (high-entropy strings, `.env`-shaped dumps, key
//! material) — redact-or-reject; (c) enforces the references-not-dumps invariant
//! (file/line references pass; file *contents* are rejected).
//!
//! The `feedback-as-data-audit` oracle (Probe B) asserts every outbound POST
//! routes through this one function. A single egress chokepoint mirrors the PII
//! scrubber's single-writer discipline.
//!
//! Stage-0 status: the signature + the PII-scrub reuse are frozen here; Worker A
//! (Stage 1) finalizes the secret denylist + references-not-dumps logic and
//! activates the C24 (f) runner-side corpus against it.

use thiserror::Error;

/// Why an outbound payload was rejected (rather than redacted) at the egress
/// chokepoint.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SanitizeError {
    #[error("outbound payload rejected: contains a source/secret dump (references only, never contents)")]
    SecretDump,
    #[error("outbound payload rejected: malformed")]
    Malformed,
}

/// Apply the canonical PII scrubber to a string (FR-FBR-10). Thin wrapper over
/// `feedbackmonk_tracing::scrub` so the runner shares the ONE workspace
/// chokepoint rather than re-implementing PII patterns.
#[must_use]
pub fn scrub_pii(text: &str) -> String {
    feedbackmonk_tracing::scrub(text)
}

/// **THE outbound egress chokepoint.** Sanitize a payload before it crosses the
/// wire: PII-scrub, then secret-denylist redact-or-reject, then
/// references-not-dumps.
///
/// Stage-0: applies the PII-scrub leg as the frozen floor; Worker A (Stage 1)
/// finalizes the secret denylist + references-not-dumps and wires the C24 (f)
/// corpus. The signature is frozen so Worker B/C call it today.
///
/// # Errors
/// [`SanitizeError::SecretDump`] when the payload carries source/secret content
/// rather than references (finalized by Worker A).
pub fn sanitize_outbound(value: &serde_json::Value) -> Result<serde_json::Value, SanitizeError> {
    // Frozen floor (leg a): PII-scrub every string in the payload. The secret
    // denylist (b) + references-not-dumps (c) are finalized by Worker A — until
    // then this is a scrub-only pass-through so Worker B/C can call the chokepoint.
    Ok(scrub_json_strings(value))
}

/// Recursively PII-scrub every string leaf of a JSON value.
fn scrub_json_strings(value: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::String(s) => Value::String(scrub_pii(s)),
        Value::Array(items) => Value::Array(items.iter().map(scrub_json_strings).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), scrub_json_strings(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scrub_pii_reuses_canonical_scrubber() {
        let out = scrub_pii("contact alice@example.com");
        assert!(!out.contains("alice@example.com"), "email must be scrubbed: {out}");
    }

    #[test]
    fn sanitize_outbound_scrubs_nested_strings() {
        let payload = json!({
            "summary": "report from bob@example.com",
            "refs": ["see line 10", "ping carol@example.com"]
        });
        let out = sanitize_outbound(&payload).unwrap();
        let s = out.to_string();
        assert!(!s.contains("bob@example.com"));
        assert!(!s.contains("carol@example.com"));
    }
}
