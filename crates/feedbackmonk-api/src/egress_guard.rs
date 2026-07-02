//! Server-side secret-dump reject for owner-reachable runner output (scrutiny
//! P2-9). **Defense-in-depth over the runner's egress sanitizer.**
//!
//! The "source never leaves" guarantee (Contract C27 / FR-FBR-25c) is enforced
//! primarily by [`feedbackmonk_runner::sanitizer::sanitize_outbound`] on the
//! runner side — but that is CLIENT-side. A compromised runner (or a leaked
//! runner write-token) could POST a `result_ref` carrying raw secrets, and
//! `runner_transition` persists it verbatim (COALESCE) into
//! `work_orders.result_ref`, which is then surfaced to the owner in the admin UI.
//!
//! This module adds a SERVER-side reject at the persistence boundary: a
//! `result_ref` that structurally carries key material (PEM), a secret-named
//! assignment, or a `.env`-shaped dump is refused with a `400` BEFORE it can
//! land in owner-reachable context. It is deliberately a *reject*, not a
//! redact — the server does not rewrite runner output, it refuses secret-laden
//! output. Proportionate mirror of the runner's `is_secret_dump` leg; the API
//! crate has no Rust dependency on the runner crate (architectural decoupling),
//! so the pattern set is reproduced here rather than imported.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use crate::error::ApiError;

/// Hard ceiling on the serialized `result_ref` — a legitimate result reference
/// carries pointers + counts + a short summary, never a file/source dump. A
/// blob past this ceiling is treated as a dump.
const MAX_RESULT_REF_BYTES: usize = 32 * 1024;

/// Hard ceiling on any single string inside the `result_ref`. Mirrors the
/// runner-side `MAX_STR_CHARS` references-not-dumps discipline.
const MAX_STR_CHARS: usize = 2_048;

/// A secret-shaped key/value assignment (name containing a secret-ish token
/// immediately assigned a non-empty value). Matches a leaked `.env` line such
/// as `AWS_SECRET_ACCESS_KEY=...` regardless of casing. Mirrors the runner's
/// `SECRET_ASSIGN_RE`.
static SECRET_ASSIGN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b[A-Z0-9_]*(?:SECRET|PASSWORD|PASSWD|PRIVATE[_-]?KEY|API[_-]?KEY|ACCESS[_-]?KEY|AUTH[_-]?TOKEN|CREDENTIAL|PASSPHRASE)[A-Z0-9_]*\s*[:=]\s*\S",
    )
    .expect("valid secret-assignment regex")
});

/// A generic `.env`-shaped assignment line: `UPPER_SNAKE=value`. Two or more in
/// one string is a `.env` dump. Mirrors the runner's `ENV_LINE_RE`.
static ENV_LINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*[A-Z][A-Z0-9_]{2,}=\S").expect("valid env-line regex"));

/// Does this string structurally carry a secret/source dump?
fn is_secret_dump(s: &str) -> bool {
    // PEM-armoured key material.
    if s.contains("-----BEGIN") && s.contains("PRIVATE KEY") {
        return true;
    }
    // A secret-named assignment (a single line is a leaked secret).
    if SECRET_ASSIGN_RE.is_match(s) {
        return true;
    }
    // A `.env`-shaped dump: two or more generic UPPER_SNAKE=value lines.
    if ENV_LINE_RE.find_iter(s).count() >= 2 {
        return true;
    }
    false
}

fn scan_value(value: &Value) -> Result<(), ApiError> {
    match value {
        Value::String(s) => {
            if s.chars().count() > MAX_STR_CHARS || is_secret_dump(s) {
                return Err(reject());
            }
            Ok(())
        }
        Value::Array(items) => items.iter().try_for_each(scan_value),
        Value::Object(map) => map.values().try_for_each(scan_value),
        _ => Ok(()),
    }
}

fn reject() -> ApiError {
    // Deliberately generic body — do not echo the offending content back.
    ApiError::BadRequest(
        "result_ref rejected: it appears to carry secret or source material (references only, never contents)".into(),
    )
}

/// Reject a `result_ref` that structurally carries secret/source material
/// before it is persisted into owner-reachable context (scrutiny P2-9).
///
/// # Errors
/// Returns [`ApiError::BadRequest`] (`400`) when the payload exceeds the size
/// ceiling or any nested string is a PEM key, a secret-named assignment, or
/// part of a `.env`-shaped dump.
pub fn reject_secret_laden_result_ref(result_ref: &Value) -> Result<(), ApiError> {
    if result_ref.to_string().len() > MAX_RESULT_REF_BYTES {
        return Err(reject());
    }
    scan_value(result_ref)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn legitimate_result_ref_passes() {
        let payload = json!({
            "pr_url": "https://git.example/pr/1",
            "branch": "fbm/wo-123",
            "diff_stat": {"files": 2, "insertions": 10, "deletions": 3},
            "summary": "Fixed the null check in src/auth.rs:42; tests pass."
        });
        reject_secret_laden_result_ref(&payload).expect("references + conclusions pass");
    }

    #[test]
    fn rejects_secret_named_assignment() {
        let payload = json!({"summary": "done; AWS_SECRET_ACCESS_KEY=AKIAIOSFODNN7EXAMPLEKEY"});
        assert!(reject_secret_laden_result_ref(&payload).is_err());
    }

    #[test]
    fn rejects_pem_private_key() {
        let payload = json!({
            "summary": "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIB...\n-----END RSA PRIVATE KEY-----"
        });
        assert!(reject_secret_laden_result_ref(&payload).is_err());
    }

    #[test]
    fn rejects_dotenv_dump() {
        let payload = json!({"summary": "DB_HOST=db.internal\nDB_PORT=5432\nFEATURE_X=on"});
        assert!(reject_secret_laden_result_ref(&payload).is_err());
    }

    #[test]
    fn rejects_oversize_string() {
        let payload = json!({"summary": "x".repeat(MAX_STR_CHARS + 1)});
        assert!(reject_secret_laden_result_ref(&payload).is_err());
    }

    #[test]
    fn rejects_nested_secret() {
        let payload = json!({"outer": {"inner": ["ok", "API_KEY=deadbeefdeadbeef"]}});
        assert!(reject_secret_laden_result_ref(&payload).is_err());
    }
}
