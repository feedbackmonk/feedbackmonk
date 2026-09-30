# pii-scrub-audit

## Summary

Proves that every log line goes through the PII scrubber (FR-FBR-10). The scrubber is the
only global subscriber, installed by `feedbackmonk_tracing::install_global_subscriber`, so
this oracle fails when any crate other than `feedbackmonk-tracing` builds or installs a
subscriber of its own. Come here after touching logging setup in any crate.

## Probe

It walks `crates/**/*.rs`, skipping `crates/feedbackmonk-tracing/` and `target/`, test
code included. Comments are stripped first. It flags any of these:

- `tracing_subscriber::fmt` (the builder, `fmt()`, `fmt::init`, `fmt::Subscriber`)
- `tracing_subscriber::registry(`
- `FmtSubscriber` and `SubscriberBuilder`
- `set_global_default(`, `tracing::subscriber::set_default(` and `with_default(`
- `.init()` or `.try_init()` in a file that uses `tracing_subscriber`
- `impl ... Layer<...> for ...`, a hand-rolled layer that could skip scrubbing

Test code counts too, because a test that installs its own subscriber is how an unscrubbed
pattern gets copied into production.

A second check, that the pattern set has not drifted, is a test and not a probe:
`crates/feedbackmonk-tracing/tests/scrubber_patterns.rs` compares the SHA-256 of
`canonical_serialised()` with `tests/canonical_pattern_hash.txt`. The test fails when that
file is missing. `feedbackmonk-api/tests/attachment_pii_corpus.rs` pins the same digest.

## Invocation

```bash
python .claude/project-oracles/pii-scrub-audit/oracle.py [--root <repo>]
```

It exits 0 on pass, 1 on fail (offenders listed as `file:line`), and 2 when the tracing
crate is missing. `--root` points it at a copy of the tree, which the self-test uses.

## Adversarial self-test (2026-09-30)

These ran against a scratch copy of `crates/`, each appended to `feedbackmonk-api/src/main.rs`:

- `tracing_subscriber::fmt::init();` fails (exit 1).
- `FmtSubscriber::new()` plus `tracing::subscriber::set_global_default(..)` fails on both.

The previous probe passed both. The unchanged tree passes.

## Decisions

- **The hash leg moved to a test (2026-09-30).** Probe B hashed the pattern slice, which
  two Rust tests already asserted, and one of them passed silently when the hash file was
  missing. The file now lives beside the scrubber, and that silent pass is now a failure.
- **Probe A was widened (2026-09-30).** It used to match only `fmt(`, `registry(` and
  `impl Layer`, which missed `fmt::init()`, `FmtSubscriber` and `set_global_default`.
- **The `impl Layer` regex stays specific.** A loose `impl.*Layer.*for` would
  false-positive on tower-http's `TraceLayer::new_for_http()`.
