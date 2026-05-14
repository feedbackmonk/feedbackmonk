# feedbackr-tracing

> **Synopsis**: PII-scrubbing `tracing-subscriber` chokepoint (FR-FBR-10). All
> log emissions from the Feedbackr binary pass through the canonical
> 20-pattern PII scrubber at the WRITE boundary. `install_global_subscriber`
> is the sole public entry point; the `pii-scrub-audit` Verification Oracle
> enforces "no other tracing-subscriber setup anywhere in the workspace" at
> AST-grade.

## Purpose & Responsibilities

- Compose the process-wide `tracing-subscriber` Subscriber for the
  `feedbackr-api` binary (and any future binaries in the workspace).
- Apply the canonical 20-pattern PII scrubber to every emitted log byte.
- Provide a test-only `SharedBufferScrubbing` writer so integration tests
  can prove PII was scrubbed without polluting the global subscriber.
- Carry the canonical pattern set in a form the
  `.claude/oracles/pii-scrub-audit/` oracle can hash for drift detection.

## File Index

| File | What it does |
|---|---|
| `src/lib.rs` | Public surface: `install_global_subscriber`, `LogLevel`, `LogFormat`, `TracingError`, re-export `scrub`. |
| `src/scrubber.rs` | `CANONICAL_PATTERNS: &[(&str, &str, &str)]` (the 20-pattern set), the `scrub` function, `canonical_serialised` (bytes the oracle hashes), and pattern-by-pattern unit tests. |
| `src/layer.rs` | `StdoutScrubbing` (production `MakeWriter`) + `SharedBufferScrubbing` (test fixture). Buffers each event's bytes, scrubs on flush/drop. |
| `tests/scrubber_patterns.rs` | End-to-end integration tests through real `tracing::info!`/`warn!` emission + a bilateral SHA-256 check that the Rust-side hash matches `expected_hash.txt`. |

## Public API & Usage

```rust
use feedbackr_tracing::{install_global_subscriber, LogFormat, LogLevel};

fn main() -> anyhow::Result<()> {
    install_global_subscriber(LogLevel::Info, LogFormat::Json)?;
    tracing::info!("feedbackr-api listening on 14304");
    Ok(())
}
```

- `RUST_LOG`, if set, overrides the `level` argument (parsed as a
  full `EnvFilter` directive — supports per-crate granularity).
- `LogFormat::Json` is the production default (matches the P0 baseline).
- Calling `install_global_subscriber` twice in the same process returns
  `Err(TracingError::Init(_))` on the second call.

For tests that want to capture scrubbed bytes:

```rust
use feedbackr_tracing::SharedBufferScrubbing;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};

let buf = SharedBufferScrubbing::new();
let layer = fmt::layer().with_writer(buf.clone());
let subscriber = tracing_subscriber::registry().with(layer);
tracing::subscriber::with_default(subscriber, || {
    tracing::info!("uid 550e8400-e29b-41d4-a716-446655440000");
});
let bytes = buf.snapshot();
assert!(!bytes.windows(36).any(|w| w == b"550e8400-e29b-41d4-a716-446655440000"));
```

## Constraints & Business Rules

1. **Byte-for-byte pattern parity with GitCellar.** Patterns are a port of
   `gitcellar-service/src/feedback_logs/scrubber.rs`. Drift surfaces as a
   `pii-scrub-audit` Probe B failure (SHA-256 mismatch).
2. **No `tracing_subscriber::fmt()`, `tracing_subscriber::registry()`, or
   `impl Layer<...> for ...` outside this crate.** `pii-scrub-audit` Probe A
   enforces.
3. **`install_global_subscriber` is the sole composition seam.** Other
   binaries (if/when added) install via this function; never inline their
   own subscriber.
4. **Idempotent scrubber.** `scrub(scrub(x)) == scrub(x)` is asserted in
   `tests/scrubber_patterns.rs::integration_idempotent_through_subscriber`
   and in `src/scrubber.rs::tests::idempotent_double_scrub`. The bracketed
   replacement sigils (`[email]`, `[uuid]`, etc.) never match any pattern.
5. **WRITE-boundary chokepoint, not field-level rewriting.** The brief's
   wording calls for a custom `Layer` impl applying scrub to event fields.
   We chokepoint at the bytes a formatter has chosen to emit. This catches
   PII regardless of which field carried it and avoids the brittleness of a
   field-visitor approach. Documented deviation; same end-user property.

## Relationships & Dependencies

- **Consumed by**: `feedbackr-api` (via `bin/feedbackr-api/src/main.rs`,
  replacing the P0 inline `tracing_subscriber::fmt()` builder).
- **Forbids inside it**: every other backend crate (P1 + later) emitting
  logs implicitly relies on this chokepoint having been installed by the
  binary at startup; no in-crate tracing setup elsewhere.
- **Workspace deps**: `regex`, `once_cell`, `thiserror`, `tracing`,
  `tracing-subscriber`. `sha2` dev-only for the bilateral hash test.
- **External integration**: `.claude/oracles/pii-scrub-audit/` consumes
  `src/scrubber.rs` (Probe B parses `CANONICAL_PATTERNS`).

## Decision Log

- **Pattern slot shape**: `(name, regex, replacement)` 3-tuple. GitCellar's
  source uses `Rule { re, replacement }` with the name only in comments.
  Promoting `name` to a slice field lets the oracle name offenders and
  detects accidental re-ordering even if the regex/replacement pair is
  unchanged.
- **Chokepoint location: writer, not `Layer`**. See Constraint #5 above and
  the docstring in `src/layer.rs`. The semantic property the brief asks for
  (every emitted log line passes through scrub) is preserved; the
  implementation seam is more stable.
- **`SharedBufferScrubbing` is `pub` (not `pub(crate)`)**. Integration tests
  live in `tests/`, which see only the crate's `pub` surface. Exposing the
  test fixture in the public surface lets downstream crates (e.g.,
  `feedbackr-api` integration tests in P1 Stage 2) install per-test
  subscribers to assert their own logging behaviour without re-implementing
  the writer factory.
- **`RUST_LOG` overrides `level`**. Matches P0 main.rs behaviour. The
  function signature surfaces `level` as the default but defers to
  `RUST_LOG` for per-crate granularity, so operations can crank verbosity
  for one module without redeploying.
