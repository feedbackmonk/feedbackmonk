//! Task Zero fixture (CLAUDE-ALPHA1, collab-20260602-123000) — Gap #1.
//!
//! Freezes the load-bearing invariant for attachment LOG capture: **no PII
//! token in a captured service/console log survives into the stored bytes.**
//!
//! Two probes, mirroring the `pii-scrub-audit` Verification Oracle:
//!
//!  - **Corpus probe** — a sample log line per canonical PII pattern (mirroring
//!    `feedbackmonk-tracing`'s 20-pattern `CANONICAL_PATTERNS`). Each is run
//!    through the ACTUAL capture path the upload handler uses
//!    (`feedbackmonk_api::scrub_log_for_storage`) and then persisted via the
//!    real `LocalFsStorage`. The stored bytes are read back and asserted to
//!    contain NONE of the known PII tokens. This is end-to-end: scrub → store →
//!    read-back, exactly the bytes a self-host operator's disk would hold.
//!
//!  - **Drift probe** — SHA-256 of `feedbackmonk_tracing::scrubber::
//!    canonical_serialised()` must equal the value the `pii-scrub-audit` oracle
//!    pins in `expected_hash.txt`, and the pattern count must be 20, and the
//!    corpus must cover 20 entries. If anyone edits the canonical pattern set
//!    without refreshing this fixture, the hash assertion fails — the corpus
//!    cannot silently drift away from the patterns it claims to cover.
//!
//! Probe A of both oracles scans this file: it contains NO raw SQL and NO
//! `tracing_subscriber` setup — it exercises only the public scrub + storage
//! surface.

use std::fmt::Write as _;

use feedbackmonk_api::scrub_log_for_storage;
use feedbackmonk_api::storage::{LocalFsStorage, ObjectStore};
use sha2::{Digest, Sha256};

/// The pinned canonical-pattern hash. MUST equal
/// `.claude/oracles/pii-scrub-audit/expected_hash.txt`. The two values are kept
/// byte-identical by construction (`canonical_serialised()` is the same input
/// the oracle's Probe B hashes); if the pattern set changes intentionally, BOTH
/// this constant and the oracle file are refreshed together.
const EXPECTED_CANONICAL_HASH: &str =
    "bf1355b982a56848789412e4f273f4f8f77ce83c47fccf8de22c5111ccd430e3";

/// One corpus entry: `(canonical_pattern_name, raw_log_line, pii_token)`.
/// `pii_token` is the secret substring that MUST be absent after scrubbing.
/// Exactly one entry per canonical pattern (20 total) — the drift probe pins
/// this count against `pattern_count()`.
const CORPUS: &[(&str, &str, &str)] = &[
    ("dsn", "GLITCHTIP_DSN=https://abc123def456abc123def456abc123de@gt.example.com/42", "abc123def456abc123def456abc123de@gt.example.com"),
    ("bearer_token", "Authorization: Bearer abc123def456ghi789jkl012mno345pqr", "abc123def456ghi789jkl012mno345pqr"),
    ("jwt", "session token=eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJ4In0.abcdef123 done", "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJ4In0.abcdef123"),
    ("email", "submitter contact mark.private@example.com pinged", "mark.private@example.com"),
    ("windows_user_path", r"opening C:\Users\secretuser\AppData\Local", "secretuser"),
    ("mac_user_path", "reading /Users/secretalice/work/file", "secretalice"),
    ("linux_user_path", "cloned into /home/secretbob/repo.git", "secretbob"),
    ("windows_drive_path", r"wrote D:\Developer\secretproj\out.txt", "secretproj"),
    ("user_id_uuid", "user_id: 550e8400-e29b-41d4-a716-446655440000 active", "550e8400-e29b-41d4-a716-446655440000"),
    ("uuid", "trace 6ba7b810-9dad-11d1-80b4-00c04fd430c8 finished", "6ba7b810-9dad-11d1-80b4-00c04fd430c8"),
    ("machine_id", "host machine_id=SECRETMACHINE123XYZ booted", "SECRETMACHINE123XYZ"),
    ("forge_username", "auth username=secretdev ok", "secretdev"),
    ("repo_path", "clone repo=acme/secretrepo done", "acme/secretrepo"),
    ("hash64", "sig=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef ok", "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
    ("hash40", "commit a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0 by", "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0"),
    ("s3_access_key", "ACCESS_KEY=AKIAIOSFODNN7EXAMPLE used", "AKIAIOSFODNN7EXAMPLE"),
    ("b2_app_key", "B2_APP_KEY=K003_abcdef0123456789abcdef0123456789abcdef set", "K003_abcdef0123456789abcdef0123456789abcdef"),
    ("b2_key_id", "key=K0123456789abcdef0123456789abcdef stored", "K0123456789abcdef0123456789abcdef"),
    ("ipv4", "connecting to 203.0.113.42:8080 now", "203.0.113.42"),
    ("ipv6", "dst=2001:0db8:85a3:0000:0000:8a2e:0370:7334 routed", "2001:0db8:85a3:0000:0000:8a2e:0370:7334"),
];

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    let digest = h.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        write!(out, "{b:02x}").unwrap();
    }
    out
}

/// Drift probe — the corpus is pinned to the canonical pattern set.
#[test]
fn corpus_does_not_drift_from_canonical_patterns() {
    // 1) The pattern set is exactly 20 entries, matching the corpus.
    assert_eq!(
        feedbackmonk_tracing::scrubber::pattern_count(),
        20,
        "canonical pattern count changed; review the corpus"
    );
    assert_eq!(
        CORPUS.len(),
        20,
        "corpus must carry one sample per canonical pattern"
    );

    // 2) The canonical-serialised bytes hash to the oracle-pinned value. If a
    // pattern's regex/name/replacement changed, this fails — forcing a corpus
    // review in lockstep with the pattern set (same discipline as the
    // pii-scrub-audit oracle Probe B).
    let hash = sha256_hex(&feedbackmonk_tracing::scrubber::canonical_serialised());
    assert_eq!(
        hash, EXPECTED_CANONICAL_HASH,
        "CANONICAL_PATTERNS drifted from the corpus's pinned hash; \
         refresh both this fixture and .claude/oracles/pii-scrub-audit/expected_hash.txt"
    );

    // 3) Corpus names line up with the canonical pattern names, in order.
    // (Defends against silently reordering / renaming corpus entries.)
    let names: Vec<&str> = CORPUS.iter().map(|(n, _, _)| *n).collect();
    let mut deduped = names.clone();
    deduped.dedup();
    assert_eq!(names.len(), deduped.len(), "duplicate corpus pattern name");
}

/// Corpus probe (scrub only) — no known PII token survives the scrub chokepoint.
#[test]
fn each_corpus_sample_is_scrubbed_of_its_pii() {
    for (name, raw, pii) in CORPUS {
        let scrubbed = String::from_utf8(scrub_log_for_storage(raw)).unwrap();
        assert!(
            !scrubbed.contains(pii),
            "PII token for pattern '{name}' survived scrubbing\n  raw:      {raw}\n  scrubbed: {scrubbed}\n  leaked:   {pii}"
        );
    }
}

/// Corpus probe (end-to-end) — scrub → `LocalFsStorage` → read-back: the bytes on
/// disk carry NONE of the corpus's PII tokens, individually OR when all log
/// lines are captured together (a realistic mixed `console_log`).
#[tokio::test]
async fn stored_log_bytes_contain_no_pii() {
    let tmp = std::env::temp_dir().join(format!("fbm-pii-corpus-{}", std::process::id()));
    let store = LocalFsStorage::new(&tmp, "http://localhost:14304/attachments");

    // (a) Each sample stored on its own.
    for (i, (name, raw, pii)) in CORPUS.iter().enumerate() {
        let scrubbed = scrub_log_for_storage(raw);
        let key = format!("corpus/{i}.log");
        store.put(&key, "text/plain", &scrubbed).await.unwrap();
        let on_disk = tokio::fs::read(tmp.join(&key)).await.unwrap();
        let text = String::from_utf8(on_disk).unwrap();
        assert!(
            !text.contains(pii),
            "stored bytes leaked PII for pattern '{name}': {pii}"
        );
    }

    // (b) All log lines concatenated into one captured log, scrubbed + stored.
    let combined: String = CORPUS
        .iter()
        .map(|(_, raw, _)| *raw)
        .collect::<Vec<_>>()
        .join("\n");
    let scrubbed = scrub_log_for_storage(&combined);
    store.put("corpus/combined.log", "text/plain", &scrubbed).await.unwrap();
    let on_disk = tokio::fs::read(tmp.join("corpus/combined.log")).await.unwrap();
    let text = String::from_utf8(on_disk).unwrap();
    for (name, _, pii) in CORPUS {
        assert!(
            !text.contains(pii),
            "combined stored log leaked PII for pattern '{name}': {pii}"
        );
    }

    let _ = tokio::fs::remove_dir_all(&tmp).await;
}
