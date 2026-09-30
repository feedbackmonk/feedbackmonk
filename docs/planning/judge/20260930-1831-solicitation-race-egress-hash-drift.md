---
schema: judge/1
verdict: VOUCH
vouched_by: judge
judge_model: claude-opus-5-5
judge_seat: same-model
ruled_at: 2026-09-30T18:40:50Z
working_session: 875e346e-f50a-4eb2-bfda-6d482ce62811
diff_form: staged
test_diff_sha256: c91b5809ab3798630be0dc4205e671d65b6e58b91b313214735c9c3b1786b6f5
code_diff_sha256: 4a252461cc3169dbeb469fece862b4d1757ef757452ad0fcc1c02df54d3c77d3
tests: ["crates/feedbackmonk-api/tests/attachment_pii_corpus.rs", "crates/feedbackmonk-repository/src/solicitations.rs", "crates/feedbackmonk-runner/src/report.rs", "crates/feedbackmonk-tracing/tests/canonical_pattern_hash.txt", "crates/feedbackmonk-tracing/tests/scrubber_patterns.rs"]
---

## Reason

No existing assertion was weakened. In solicitations.rs the three existing repo tests gain only an extra `.unwrap()`, forced by the code diff changing `SolicitationRepo::upsert` to return `Result<Option<_>>` alongside a new SQL `WHERE status <> 'opted_out' OR EXCLUDED.status = 'opted_out'` guard; their assertions are unchanged, and a new test `stale_write_cannot_overwrite_opted_out` pins the new guard. In report.rs a new test covers the new `failure_reason_for_egress` (secret-shaped and 10k-dump reasons withheld, benign reason kept). `canonical_hash_matches_expected_file` is made stricter: it used to pass silently when the hash file was missing, empty or "placeholder", and now panics in those cases, with the same equality assertion. That makes it a full replacement for the pii-scrub-audit oracle's Probe B, which the same diff deletes, and CI runs it under `cargo test --workspace`. The hash file moved with its content unchanged (R100). attachment_pii_corpus.rs changes only comments and a message string, and its pinned constant and assertions are unchanged.

## Rationale submitted

(1) solicitations.rs: the `upsert` trait now returns Result<Option<SolicitationRecord>> — None when the stored row is opted_out and the new status is not (SQL-level guard closing a read-then-write race that could overwrite opted_out, DEC-FBR-IMPL-24). Existing repo tests gain one `.unwrap()` for the Option (assertions unchanged); a NEW test `stale_write_cannot_overwrite_opted_out` asserts the stale write is refused, row stays opted_out with prompt_count 1, and re-opt-out stays idempotent. (2) report.rs: new pure fn `failure_reason_for_egress` routes the runner failure reason through the egress sanitizer (it was only PII-scrubbed); NEW unit test asserts a secret-shaped reason and a 10k dump are withheld and a benign reason passes through. (3) scrubber_patterns.rs: the pattern-hash drift test used to read the hash file from the retired oracle's directory and PASS SILENTLY (eprintln only) when the file was missing/empty/'placeholder'. It now reads tests/canonical_pattern_hash.txt beside the crate and FAILS (panic) when missing or empty; the equality assertion is unchanged. This strengthens it. The hash file moved (git mv) with identical content. (4) attachment_pii_corpus.rs: comment-only edits repointing the hash file path and naming the tracing test instead of the retired oracle Probe B; the pinned constant and assertions are unchanged.
