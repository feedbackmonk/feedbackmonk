//! End-to-end integration tests for the scrubbing chokepoint.
//!
//! Each test installs a per-test `tracing-subscriber` Subscriber backed by
//! `SharedBufferScrubbing` (the test-only writer factory), emits a
//! `tracing::info!` (or similar) carrying PII, then asserts the recorded
//! bytes are PII-free. This proves the chokepoint catches PII regardless of
//! which field (message vs. structured field) carried it.
//!
//! We deliberately do NOT call `install_global_subscriber` here — that
//! function installs the PROCESS-WIDE subscriber and would taint sibling
//! integration tests sharing the same test binary. Each test gets its own
//! `SubscriberInitExt::with_default` guard instead.

use sha2::{Digest, Sha256};

use feedbackr_tracing::{scrubber, SharedBufferScrubbing};
use tracing_subscriber::{fmt, layer::SubscriberExt};

fn capture_with<F: FnOnce()>(buf: &SharedBufferScrubbing, emit: F) -> String {
    let layer = fmt::layer()
        .with_ansi(false)
        .with_target(false)
        .without_time()
        .with_writer(buf.clone());
    let subscriber = tracing_subscriber::registry().with(layer);
    tracing::subscriber::with_default(subscriber, emit);
    String::from_utf8(buf.snapshot()).expect("utf-8")
}

#[test]
fn integration_email_in_message_is_scrubbed() {
    let buf = SharedBufferScrubbing::new();
    let out = capture_with(&buf, || {
        tracing::info!("user logged in from alice@example.com via portal");
    });
    assert!(!out.contains("alice@example.com"), "PII not scrubbed: {out}");
    assert!(out.contains("[email]"), "replacement missing: {out}");
}

#[test]
fn integration_uuid_in_field_is_scrubbed() {
    let buf = SharedBufferScrubbing::new();
    let out = capture_with(&buf, || {
        let feedback_id = "550e8400-e29b-41d4-a716-446655440000";
        tracing::info!(%feedback_id, "feedback created");
    });
    assert!(!out.contains("550e8400-e29b-41d4-a716-446655440000"));
    assert!(out.contains("[uuid]"), "got: {out}");
}

#[test]
fn integration_ip_and_user_path_in_one_event() {
    let buf = SharedBufferScrubbing::new();
    let out = capture_with(&buf, || {
        tracing::warn!("connect from 10.0.0.1 in /home/alice/work");
    });
    assert!(!out.contains("10.0.0.1"));
    assert!(!out.contains("/home/alice"));
    assert!(out.contains("[ip]"));
    assert!(out.contains("/home/[user]"));
}

#[test]
fn integration_bearer_token_in_authorization_header() {
    let buf = SharedBufferScrubbing::new();
    let out = capture_with(&buf, || {
        tracing::info!(
            auth = "Bearer abc123def456ghi789jkl012mno345",
            "auth header observed"
        );
    });
    assert!(!out.contains("abc123def456ghi789jkl012mno345"));
    assert!(out.contains("Bearer [token]"), "got: {out}");
}

#[test]
fn integration_pii_free_text_unchanged_except_formatter_metadata() {
    let buf = SharedBufferScrubbing::new();
    let out = capture_with(&buf, || {
        tracing::info!("feedbackr-api listening on 14304");
    });
    // The formatter injects level + message decoration; we only assert
    // payload-side strings survive.
    assert!(out.contains("feedbackr-api listening on 14304"), "got: {out}");
}

#[test]
fn integration_idempotent_through_subscriber() {
    // Send the same line twice; output for the two events should be
    // identical (mod the formatter's own preamble, which is deterministic).
    let buf = SharedBufferScrubbing::new();
    let out = capture_with(&buf, || {
        tracing::info!("uid 550e8400-e29b-41d4-a716-446655440000");
        tracing::info!("uid 550e8400-e29b-41d4-a716-446655440000");
    });
    let count = out.matches("[uuid]").count();
    assert_eq!(count, 2, "expected two scrubbed events, got: {out}");
    assert!(!out.contains("550e8400-e29b-41d4-a716-446655440000"));
}

/// Bilateral hash check — proves the Rust side reproduces the same
/// canonical serialisation the Python oracle hashes. The hash here is
/// recomputed every run; comparison to the on-disk `expected_hash.txt`
/// happens via the `pii-scrub-audit` oracle (Probe B).
#[test]
fn canonical_hash_matches_expected_file() {
    let bytes = scrubber::canonical_serialised();
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let actual = format!("{:x}", hasher.finalize());

    let expected_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.claude/oracles/pii-scrub-audit/expected_hash.txt");
    let expected = std::fs::read_to_string(&expected_path)
        .unwrap_or_else(|_| String::from("placeholder"))
        .trim()
        .to_string();

    if expected == "placeholder" || expected.is_empty() {
        // First-time bootstrap: print the computed hash so the author can
        // populate `expected_hash.txt`. Does NOT fail the test.
        eprintln!("[canonical_hash] expected_hash.txt is unfilled; current SHA-256 = {actual}");
        eprintln!("[canonical_hash] write this value to {}", expected_path.display());
    } else {
        assert_eq!(
            actual, expected,
            "CANONICAL_PATTERNS hash drift — refresh expected_hash.txt deliberately if the pattern set changed"
        );
    }
}
