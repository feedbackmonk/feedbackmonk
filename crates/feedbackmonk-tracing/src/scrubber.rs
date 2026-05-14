//! PII scrubber — canonical 20-pattern set.
//!
//! Byte-for-byte port from
//! `gitcellar-service/src/feedback_logs/scrubber.rs` (DEC-FBR-07: GitCellar is
//! a read-only reference; this is a port, not an import). The regex strings
//! and replacement strings are identical. The only structural difference: we
//! promote the human-readable pattern name into the slice as a third field
//! so the `pii-scrub-audit` oracle can name offenders + so reordering rows
//! is detectable in the SHA-256 even if the regex/replacement pair is
//! unchanged.
//!
//! ## Pattern ORDER matters
//!
//! The Bearer-token rule (#2) fires before the JWT rule (#3) because a JWT
//! inside a `Authorization: Bearer <jwt>` header would otherwise miss the
//! `Bearer` prefix. The `user_id=<uuid>` rule (#9) fires before the bare-UUID
//! rule (#10) so `user_id: <uuid>` normalises to `user_id=[uuid]` rather than
//! leaving `user_id: [uuid]`. See GitCellar's PBT-driven bug fix in the
//! upstream module docs.
//!
//! ## Idempotence
//!
//! Each replacement string uses a bracketed sigil (`[email]`, `[uuid]`, etc.)
//! that no pattern in the set matches. `scrub(scrub(x)) == scrub(x)` by
//! construction — the integration test `idempotent_double_scrub` asserts this.
//!
//! ## Drift detection
//!
//! `pii-scrub-audit` (Probandurgy oracle) computes SHA-256 of the
//! line-serialised `(name, regex, replacement)` rows and compares to
//! `.claude/oracles/pii-scrub-audit/expected_hash.txt`. The `canonical_hash`
//! test in `tests/scrubber_patterns.rs` reproduces the same serialisation
//! Rust-side and prints the digest, so authors can refresh
//! `expected_hash.txt` after intentional pattern changes.

use once_cell::sync::Lazy;
use regex::Regex;

/// Canonical 20-pattern set — `(name, regex, replacement)`. ORDER MATTERS;
/// see module docs.
///
/// The `pii-scrub-audit` oracle parses this slice via a regex over its
/// source-text form, so every tuple MUST stay on a single line and use the
/// `("name", r"regex", "replacement")` shape exactly (raw string for the
/// regex; cooked string for name + replacement). The oracle hash also
/// includes the order, so re-sorting this slice without bumping
/// `expected_hash.txt` produces an oracle FAIL.
pub(crate) static CANONICAL_PATTERNS: &[(&str, &str, &str)] = &[
    ("dsn", r"https?://[a-f0-9]{32,}@[a-zA-Z0-9.\-]+/\d+", "[dsn]"),
    ("bearer_token", r"(?i)bearer\s+[A-Za-z0-9_\-\.=:+/]{20,}", "Bearer [token]"),
    ("jwt", r"\beyJ[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{5,}\.[A-Za-z0-9_\-]{5,}\b", "[jwt]"),
    ("email", r"\b[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}\b", "[email]"),
    ("windows_user_path", r"[A-Za-z]:\\Users\\[^\\\s]+", "[user-path]"),
    ("mac_user_path", r"/Users/[^/\s]+", "/Users/[user]"),
    ("linux_user_path", r"/home/[^/\s]+", "/home/[user]"),
    ("windows_drive_path", r"[A-Za-z]:\\[A-Za-z][\w]*\\[^\\\s]+", "[drive-path]"),
    ("user_id_uuid", r"user_id[=:]\s*[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}", "user_id=[uuid]"),
    ("uuid", r"\b[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}\b", "[uuid]"),
    ("machine_id", r"machine_id[=:]\s*\S+", "machine_id=[redacted]"),
    ("forge_username", r"\busername[=:]\s*\S+", "username=[redacted]"),
    ("repo_path", r"\brepo[=:]\s*[\w\-]+/[\w\-]+\b", "repo=[redacted]"),
    ("hash64", r"\b[A-Fa-f0-9]{64}\b", "[hash64]"),
    ("hash40", r"\b[a-f0-9]{40}\b", "[hash40]"),
    ("s3_access_key", r"\bAKIA[0-9A-Z]{16}\b", "[s3-access-key]"),
    ("b2_app_key", r"\bK\d{3}_[a-zA-Z0-9_\-]{30,}\b", "[b2-app-key]"),
    ("b2_key_id", r"\bK[0-9a-fA-F]{27,}\b", "[b2-key-id]"),
    ("ipv4", r"\b(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)(?:\.(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)){3}\b", "[ip]"),
    ("ipv6", r"(?i)(?:[0-9a-f]{1,4}:){7}[0-9a-f]{1,4}|::(?:[0-9a-f]{1,4}:){0,6}[0-9a-f]{1,4}|(?:[0-9a-f]{1,4}:){1,7}:", "[ipv6]"),
];

struct CompiledRule {
    re: Regex,
    replacement: &'static str,
}

static RULES: Lazy<Vec<CompiledRule>> = Lazy::new(|| {
    CANONICAL_PATTERNS
        .iter()
        .map(|(_, regex, replacement)| CompiledRule {
            re: Regex::new(regex).expect("valid scrubber regex"),
            replacement,
        })
        .collect()
});

/// Scrub a single string against every canonical pattern. Idempotent
/// (`scrub(scrub(x)) == scrub(x)`) by construction — bracketed sigils
/// never match any rule.
#[must_use]
pub fn scrub(input: &str) -> String {
    if input.is_empty() {
        return String::new();
    }
    let mut current = std::borrow::Cow::Borrowed(input);
    for rule in RULES.iter() {
        let next = rule.re.replace_all(&current, rule.replacement);
        match next {
            std::borrow::Cow::Borrowed(_) => {
                // No match — leave `current` alone.
            }
            std::borrow::Cow::Owned(s) => {
                current = std::borrow::Cow::Owned(s);
            }
        }
    }
    current.into_owned()
}

/// Number of canonical patterns. Exposed for the `canonical_hash` test.
#[must_use]
pub fn pattern_count() -> usize {
    CANONICAL_PATTERNS.len()
}

/// Canonical SHA-256 input bytes — `name\tregex\treplacement\n` per row,
/// UTF-8. Used by the `canonical_hash` test (Rust side) and by
/// `.claude/oracles/pii-scrub-audit/oracle.py` (Python side). The two
/// implementations MUST stay byte-identical.
#[must_use]
pub fn canonical_serialised() -> Vec<u8> {
    let mut out = Vec::with_capacity(CANONICAL_PATTERNS.len() * 96);
    for (name, regex, replacement) in CANONICAL_PATTERNS {
        out.extend_from_slice(name.as_bytes());
        out.push(b'\t');
        out.extend_from_slice(regex.as_bytes());
        out.push(b'\t');
        out.extend_from_slice(replacement.as_bytes());
        out.push(b'\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_exactly_twenty_patterns() {
        assert_eq!(pattern_count(), 20, "canonical set is locked at 20 patterns");
    }

    #[test]
    fn pattern_names_are_unique() {
        let mut names: Vec<&str> = CANONICAL_PATTERNS.iter().map(|(n, _, _)| *n).collect();
        names.sort_unstable();
        let pre_dedup = names.len();
        names.dedup();
        assert_eq!(names.len(), pre_dedup, "duplicate pattern name in CANONICAL_PATTERNS");
    }

    #[test]
    fn rules_compile_eagerly() {
        // Force lazy compile + assert all 20 compiled.
        assert_eq!(RULES.len(), 20);
    }

    #[test]
    fn empty_input_unchanged() {
        assert_eq!(scrub(""), "");
    }

    #[test]
    fn pii_free_text_unchanged() {
        let txt = "Service started OK. Listening for webhooks.";
        assert_eq!(scrub(txt), txt);
    }

    // Per-pattern positive-match tests — each of the 20 canonical patterns
    // gets at least one input that contains the pattern + asserts the
    // replacement appears in the output.

    #[test]
    fn dsn_scrubbed() {
        let out = scrub("GLITCHTIP_DSN=https://abc123def456abc123def456abc123de@gt.example.com/42");
        assert!(out.contains("[dsn]"), "got: {out}");
    }

    #[test]
    fn bearer_token_scrubbed() {
        let out = scrub("Authorization: Bearer abc123def456ghi789jkl012mno345");
        assert!(out.contains("Bearer [token]"), "got: {out}");
    }

    #[test]
    fn jwt_scrubbed() {
        let out = scrub("token=eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJ4In0.abcdef");
        assert!(out.contains("[jwt]"), "got: {out}");
    }

    #[test]
    fn email_scrubbed() {
        assert_eq!(scrub("user mark@example.com pinged"), "user [email] pinged");
    }

    #[test]
    fn windows_user_path_scrubbed() {
        assert_eq!(
            scrub(r"C:\Users\Carbonadmin\AppData\Local"),
            r"[user-path]\AppData\Local"
        );
    }

    #[test]
    fn mac_user_path_scrubbed() {
        assert_eq!(scrub("/Users/alice/work"), "/Users/[user]/work");
    }

    #[test]
    fn linux_user_path_scrubbed() {
        assert_eq!(scrub("/home/bob/repo.git"), "/home/[user]/repo.git");
    }

    #[test]
    fn windows_drive_path_scrubbed() {
        let out = scrub(r"D:\Developer\sub\file.txt");
        assert!(out.contains("[drive-path]"), "got: {out}");
    }

    #[test]
    fn user_id_uuid_scrubbed_with_normalisation() {
        // user_id rule runs before bare-UUID rule; `:` normalises to `=`.
        let out = scrub("user_id: 550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(out, "user_id=[uuid]");
    }

    #[test]
    fn bare_uuid_scrubbed() {
        assert_eq!(
            scrub("feedback id=550e8400-e29b-41d4-a716-446655440000 created"),
            "feedback id=[uuid] created"
        );
    }

    #[test]
    fn machine_id_scrubbed() {
        let out = scrub("machine_id=abc123xyz running");
        assert!(out.contains("machine_id=[redacted]"), "got: {out}");
    }

    #[test]
    fn forge_username_scrubbed() {
        let out = scrub("username=alice on host");
        assert!(out.contains("username=[redacted]"), "got: {out}");
    }

    #[test]
    fn repo_path_scrubbed() {
        let out = scrub("clone repo=acme/widgets ok");
        assert!(out.contains("repo=[redacted]"), "got: {out}");
    }

    #[test]
    fn hash64_scrubbed() {
        // 64-char lowercase hex.
        let out = scrub("sig=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef end");
        assert!(out.contains("[hash64]"), "got: {out}");
    }

    #[test]
    fn hash40_scrubbed() {
        // SHA-1 commit hash.
        let out = scrub("commit 1234567890abcdef1234567890abcdef12345678 by");
        assert!(out.contains("[hash40]"), "got: {out}");
    }

    #[test]
    fn s3_access_key_scrubbed() {
        let out = scrub("ACCESS_KEY=AKIAIOSFODNN7EXAMPLE used");
        assert!(out.contains("[s3-access-key]"), "got: {out}");
    }

    #[test]
    fn b2_app_key_scrubbed() {
        let out = scrub("B2_APP_KEY=K003_abcdef0123456789abcdef0123456789abcdef ok");
        assert!(out.contains("[b2-app-key]"), "got: {out}");
    }

    #[test]
    fn b2_key_id_scrubbed() {
        // B2 key id pattern: K + 27+ hex/alphanumeric. Distinct from app-key
        // (no underscore prefix).
        let out = scrub("key=K0123456789abcdef0123456789abcdef end");
        assert!(out.contains("[b2-key-id]"), "got: {out}");
    }

    #[test]
    fn ipv4_scrubbed() {
        assert_eq!(scrub("connecting to 192.168.1.42:9876"), "connecting to [ip]:9876");
    }

    #[test]
    fn ipv6_scrubbed() {
        let out = scrub("dst=2001:0db8:85a3:0000:0000:8a2e:0370:7334 ok");
        assert!(out.contains("[ipv6]"), "got: {out}");
    }

    // Near-miss / no-match cases — input LOOKS similar to a pattern but
    // doesn't match. Output is unchanged.

    #[test]
    fn near_miss_uuid_too_short_unchanged() {
        // Missing the final 12-char group.
        let txt = "feedback id=550e8400-e29b-41d4-a716- created";
        assert_eq!(scrub(txt), txt);
    }

    #[test]
    fn near_miss_email_no_tld_unchanged() {
        // No dot in domain part.
        let txt = "user mark@localhost pinged";
        assert_eq!(scrub(txt), txt);
    }

    #[test]
    fn near_miss_short_hex_not_a_hash_unchanged() {
        // 8 hex chars — far below SHA-1 (40) / SHA-256 (64) thresholds.
        let txt = "abc12345 in the buffer";
        assert_eq!(scrub(txt), txt);
    }

    #[test]
    fn near_miss_ipv4_octet_out_of_range_unchanged() {
        // 999 is > 255; the strict octet alternation rejects it.
        let txt = "999.999.999.999 not an ip";
        assert_eq!(scrub(txt), txt);
    }

    #[test]
    fn near_miss_short_bearer_token_unchanged() {
        // Bearer requires 20+ chars after; "abc123" is too short.
        let txt = "Authorization: Bearer abc123 short";
        assert_eq!(scrub(txt), txt);
    }

    #[test]
    fn near_miss_short_jwt_unchanged() {
        // Only one segment after eyJ — not a 3-part JWT.
        let txt = "token=eyJabcdefghijklmnop here";
        assert_eq!(scrub(txt), txt);
    }

    #[test]
    fn near_miss_dsn_short_key_does_not_match_dsn_rule() {
        // DSN regex requires 32+ hex chars in the key segment. A 6-char key
        // skips the DSN rule. The email rule will still fire (the
        // `abc123@host.example.com` substring), but the `[dsn]` sigil must
        // NOT appear — proving the DSN-specific rule did not match.
        let out = scrub("https://abc123@host.example.com/42");
        assert!(!out.contains("[dsn]"), "DSN rule false-fired: {out}");
    }

    #[test]
    fn near_miss_unix_share_directory_unchanged() {
        // /Users/share — the rule requires non-/ + non-whitespace after,
        // and the GitCellar rule's bound is `[^/\s]+`. "share" matches so
        // this WOULD scrub. The genuine near-miss is bare `/Users` with no
        // trailing component.
        let txt = "checking /Users alone";
        assert_eq!(scrub(txt), txt);
    }

    #[test]
    fn near_miss_ipv6_just_three_groups_unchanged() {
        let txt = "addr=ab:cd:ef short";
        assert_eq!(scrub(txt), txt);
    }

    // Idempotence — the central property.

    #[test]
    fn idempotent_double_scrub() {
        let inputs = [
            "user mark@example.com from 10.0.0.1 with id=550e8400-e29b-41d4-a716-446655440000",
            "Bearer abc123def456ghi789jkl012mno345",
            r"C:\Users\Carbonadmin opening /home/alice and AKIAIOSFODNN7EXAMPLE",
            "machine_id=abc-xyz user_id=550e8400-e29b-41d4-a716-446655440000",
        ];
        for input in inputs {
            let once = scrub(input);
            let twice = scrub(&once);
            assert_eq!(once, twice, "non-idempotent for input: {input}");
        }
    }

    #[test]
    fn multiple_patterns_in_one_line() {
        let out = scrub(
            "user mark@example.com from 10.0.0.1 with id=550e8400-e29b-41d4-a716-446655440000",
        );
        assert!(out.contains("[email]"));
        assert!(out.contains("[ip]"));
        assert!(out.contains("[uuid]"));
    }

    #[test]
    fn canonical_serialised_is_stable() {
        // Sanity check: line count matches pattern count.
        let bytes = canonical_serialised();
        let newlines = bytes.iter().filter(|b| **b == b'\n').count();
        assert_eq!(newlines, CANONICAL_PATTERNS.len());
    }
}
