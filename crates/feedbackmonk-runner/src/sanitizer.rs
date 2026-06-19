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

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;
use thiserror::Error;

/// Max chars of any single outbound string. An outbound payload carries
/// conclusions + short references (`"src/auth.rs:42"`, a PR url, a one-line
/// summary); a file-contents/source dump blows past this. Mirrors the API-side
/// `validate_source_refs` `MAX_REF_STR_CHARS` discipline, with headroom for a
/// human-readable result summary.
const MAX_STR_CHARS: usize = 2_048;

/// Max serialized byte size of a whole outbound payload — a hard ceiling against
/// a many-small-entries dump. Generous for a `ResultRef` / recommendation.
const MAX_PAYLOAD_BYTES: usize = 32 * 1024;

/// Why an outbound payload was rejected (rather than redacted) at the egress
/// chokepoint.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SanitizeError {
    #[error("outbound payload rejected: contains a source/secret dump (references only, never contents)")]
    SecretDump,
    #[error("outbound payload rejected: malformed")]
    Malformed,
}

/// A secret-shaped key/value assignment: a name containing a secret-ish token
/// (SECRET, PASSWORD, PRIVATE_KEY, API_KEY, ACCESS_KEY, TOKEN, CREDENTIAL,
/// PASSPHRASE) immediately assigned a non-empty value via `=` or `:`. Matches a
/// leaked `.env` line such as `AWS_SECRET_ACCESS_KEY=...` regardless of casing.
static SECRET_ASSIGN_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b[A-Z0-9_]*(?:SECRET|PASSWORD|PASSWD|PRIVATE[_-]?KEY|API[_-]?KEY|ACCESS[_-]?KEY|AUTH[_-]?TOKEN|CREDENTIAL|PASSPHRASE)[A-Z0-9_]*\s*[:=]\s*\S",
    )
    .expect("valid secret-assignment regex")
});

/// A generic `.env`-shaped assignment line: `UPPER_SNAKE=value`. Two or more of
/// these in one string is a `.env` dump (the keys themselves need not be
/// secret-named).
static ENV_LINE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?m)^\s*[A-Z][A-Z0-9_]{2,}=\S").expect("valid env-line regex")
});

/// A standalone high-entropy token: a >=40-char run over the base64/secret
/// alphabet that mixes a lowercase letter, an uppercase letter, AND a digit
/// (so it is not ordinary prose, a pure-hex hash — already handled by the PII
/// scrubber — or a dotted path). Redacted inline rather than rejecting the whole
/// payload (the borderline-token policy; clear structural dumps are rejected).
static HIGH_ENTROPY_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[A-Za-z0-9+/=_\-]{40,}").expect("valid high-entropy regex"));

/// Apply the canonical PII scrubber to a string (FR-FBR-10). Thin wrapper over
/// `feedbackmonk_tracing::scrub` so the runner shares the ONE workspace
/// chokepoint rather than re-implementing PII patterns.
#[must_use]
pub fn scrub_pii(text: &str) -> String {
    feedbackmonk_tracing::scrub(text)
}

/// Does this string carry a *secret/source dump* (as opposed to a reference or a
/// conclusion)? Checked on the RAW string BEFORE PII-scrub, so a scrub sigil can
/// never mask the structural shape of a dump.
fn is_secret_dump(s: &str) -> bool {
    // PEM-armoured key material.
    if s.contains("-----BEGIN") && s.contains("PRIVATE KEY") {
        return true;
    }
    // A secret-named assignment (single line is enough — it IS a leaked secret).
    if SECRET_ASSIGN_RE.is_match(s) {
        return true;
    }
    // A `.env`-shaped dump: two or more generic UPPER_SNAKE=value lines.
    if ENV_LINE_RE.find_iter(s).count() >= 2 {
        return true;
    }
    false
}

/// Redact standalone high-entropy tokens embedded in otherwise-legitimate prose
/// (the borderline leg of the redact-or-reject policy). Runs AFTER PII-scrub so
/// it only sees what the canonical scrubber left behind.
fn redact_high_entropy(text: &str) -> String {
    HIGH_ENTROPY_RE
        .replace_all(text, |caps: &regex::Captures| {
            let tok = &caps[0];
            // Require letters-of-both-cases AND a digit to count as a secret
            // token — this spares pure-lowercase words, pure-hex (already
            // scrubbed), and long dotted paths.
            let has_lower = tok.chars().any(|c| c.is_ascii_lowercase());
            let has_upper = tok.chars().any(|c| c.is_ascii_uppercase());
            let has_digit = tok.chars().any(|c| c.is_ascii_digit());
            if has_lower && has_upper && has_digit {
                "[redacted-secret]".to_string()
            } else {
                tok.to_string()
            }
        })
        .into_owned()
}

/// **THE outbound egress chokepoint.** Sanitize a payload before it crosses the
/// wire, in this order: (c) references-not-dumps size guard → (b) secret-dump
/// reject (raw) → (a) canonical PII-scrub → (b') high-entropy redact.
///
/// Every outbound payload the runner POSTs (the implementer `result_ref`, the
/// analyst recommendations) routes through here — the single egress chokepoint
/// the `feedback-as-data-audit` Probe B asserts.
///
/// # Errors
/// - [`SanitizeError::SecretDump`] when a string carries source/secret content
///   (PEM key material, a secret-named or `.env`-shaped assignment dump) or
///   exceeds [`MAX_STR_CHARS`] / the payload exceeds [`MAX_PAYLOAD_BYTES`]
///   (references-not-dumps: a dump blows past a reference's size).
pub fn sanitize_outbound(value: &Value) -> Result<Value, SanitizeError> {
    // Whole-payload size ceiling (references-not-dumps, gross).
    if value.to_string().len() > MAX_PAYLOAD_BYTES {
        return Err(SanitizeError::SecretDump);
    }
    sanitize_value(value)
}

/// Recursively sanitize a JSON value: reject dumps, scrub PII, redact tokens.
fn sanitize_value(value: &Value) -> Result<Value, SanitizeError> {
    match value {
        Value::String(s) => {
            // (c) references-not-dumps: an oversize string is a dump, not a ref.
            if s.chars().count() > MAX_STR_CHARS {
                return Err(SanitizeError::SecretDump);
            }
            // (b) secret-dump reject — on the RAW string, before scrubbing.
            if is_secret_dump(s) {
                return Err(SanitizeError::SecretDump);
            }
            // (a) canonical PII-scrub, then (b') borderline high-entropy redact.
            // NB: the canonical scrubber faithfully rewrites bare UUIDs too — the
            // egress chokepoint scrubs ALL content it is handed. Trusted routing/
            // correlation IDs (work_order_id is a URL path param; cluster_id /
            // sweep_id are host-attached AFTER this chokepoint — analyst/ingest.rs)
            // are kept OUT of the sanitized content rather than carved out here, so
            // the runner egress stays faithful to the canonical PII discipline.
            Ok(Value::String(redact_high_entropy(&scrub_pii(s))))
        }
        Value::Array(items) => Ok(Value::Array(
            items.iter().map(sanitize_value).collect::<Result<_, _>>()?,
        )),
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                out.insert(k.clone(), sanitize_value(v)?);
            }
            Ok(Value::Object(out))
        }
        other => Ok(other.clone()),
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

    #[test]
    fn references_and_conclusions_pass() {
        // The legitimate `ResultRef` shape: pointers + counts + a short summary.
        let payload = json!({
            "pr_url": "https://git.example/pr/1",
            "branch": "fbm/wo-123",
            "diff_stat": {"files": 2, "insertions": 10, "deletions": 3},
            "verification": {"tests_passed": true, "finalize_status": "passed"},
            "summary": "Fixed the null check in src/auth.rs:42; tests pass."
        });
        let out = sanitize_outbound(&payload).expect("references + conclusions pass");
        assert_eq!(out["branch"], "fbm/wo-123");
        assert_eq!(out["diff_stat"]["files"], 2);
    }

    #[test]
    fn rejects_secret_named_assignment() {
        let payload = json!({"summary": "done; AWS_SECRET_ACCESS_KEY=AKIAIOSFODNN7EXAMPLEKEY"});
        assert_eq!(sanitize_outbound(&payload), Err(SanitizeError::SecretDump));
    }

    #[test]
    fn rejects_pem_private_key_dump() {
        let payload = json!({
            "summary": "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIB...\n-----END RSA PRIVATE KEY-----"
        });
        assert_eq!(sanitize_outbound(&payload), Err(SanitizeError::SecretDump));
    }

    #[test]
    fn rejects_dotenv_shaped_dump() {
        let payload = json!({"summary": "DB_HOST=db.internal\nDB_PORT=5432\nFEATURE_X=on"});
        assert_eq!(sanitize_outbound(&payload), Err(SanitizeError::SecretDump));
    }

    #[test]
    fn rejects_oversize_string_as_dump() {
        let payload = json!({"summary": "x".repeat(MAX_STR_CHARS + 1)});
        assert_eq!(sanitize_outbound(&payload), Err(SanitizeError::SecretDump));
    }

    #[test]
    fn redacts_borderline_high_entropy_token_in_prose() {
        // Not a structural dump, but a stray secret-looking token in the summary.
        let token = "aB3xK9zQ7mN2pL5rT8wY1vC4dE6gH0jU3kM7nP2qR5sT8w"; // mixed case + digits, >=40
        let payload = json!({"summary": format!("Rotated the key to {token}; done.")});
        let out = sanitize_outbound(&payload).expect("prose with a stray token is redacted, not rejected");
        let s = out["summary"].as_str().unwrap();
        assert!(!s.contains(token), "token must be redacted: {s}");
        assert!(s.contains("[redacted-secret]"));
    }

    #[test]
    fn scrubs_bare_uuid_content_faithfully() {
        // The egress chokepoint scrubs ALL content per the canonical PII scrubber
        // — including a bare UUID. Trusted routing IDs are kept out of the content
        // (host-attached after the chokepoint), not carved out here.
        let payload = json!({"note": "saw c4a56b02-a13a-48e3-a388-7655b456d2db in the body"});
        let out = sanitize_outbound(&payload).unwrap();
        assert!(out["note"].as_str().unwrap().contains("[uuid]"));
    }

    #[test]
    fn rejects_oversize_whole_payload() {
        let big: Vec<_> = (0..2000).map(|i| json!(format!("ref-{i}-src/file.rs:1"))).collect();
        let payload = json!({"refs": big});
        assert_eq!(sanitize_outbound(&payload), Err(SanitizeError::SecretDump));
    }
}
