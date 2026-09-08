# pii-scrub-audit

**Kind**: Verification Oracle (Probandurgy — Task Zero leg 2 of three-leg defense).
**Question**: Does every emitted log line pass through the canonical 20-pattern PII
scrubber installed by `feedbackmonk_tracing::install_global_subscriber`? Has the
pattern set drifted from the canonical source ported byte-for-byte from
GitCellar's `gitcellar-service/src/feedback_logs/scrubber.rs`?

## Synopsis

Verification Oracle (P1 Task Zero) enforcing FR-FBR-10: every emitted log line must pass through the canonical 20-pattern PII scrubber installed by `feedbackmonk_tracing::install_global_subscriber`, and the pattern set must not drift from the source ported byte-for-byte from GitCellar's scrubber. AST-grade check; leg 2 of the three-leg PII-scrub defense. Re-run after touching `feedbackmonk-tracing` or any logging setup.

## Probes

### Probe A — no tracing-subscriber setup outside the scrubber crate

Walks `crates/**/*.rs` (excluding `crates/feedbackmonk-tracing/`) and flags any
match for the following patterns:

- `tracing_subscriber::fmt(` — the builder API that bypasses our scrubbing
  writer chokepoint
- `tracing_subscriber::registry(` — composing a registry outside the
  chokepoint reopens the door to unscrubbed layers
- `impl ... Layer<...> for ...` — a hand-rolled tracing-subscriber Layer
  outside the scrubber crate could elide scrubbing

Inline `//` comments are stripped before scanning so that doc-comments
mentioning the patterns don't false-fire. Multi-line `/* ... */` comments
are stripped too.

### Probe B — canonical pattern-set hash

Parses `CANONICAL_PATTERNS: &[(&str, &str, &str)]` from
`crates/feedbackmonk-tracing/src/scrubber.rs`, extracts each
`(name, regex, replacement)` tuple, serialises as `name\tregex\treplacement\n`
per row, computes SHA-256 over the UTF-8 bytes, and compares to the digest in
`expected_hash.txt`.

Any of: a new pattern, a missing pattern, a regex tweak, a replacement
tweak, or a slice re-ordering surfaces as a hash mismatch. Updating the
pattern set requires an explicit `expected_hash.txt` update commit — drift
becomes a reviewable change rather than a silent one.

## Three-leg defense (per D-FBR-02)

| Leg | Mechanism | File / location |
|---|---|---|
| 1. Type system chokepoint | `install_global_subscriber` is the sole public entry-point for tracing setup; all other items in `feedbackmonk-tracing` are `pub(crate)` or test-only. | `crates/feedbackmonk-tracing/src/lib.rs` |
| 2. AST / hash oracle (this file) | Probes A + B (this file) | `.claude/oracles/pii-scrub-audit/` |
| 3. Lint baseline | clippy `all = deny` workspace-wide; `cargo-deny` (post-P1) rejects direct `tracing_subscriber::fmt()` builder calls outside the binary entrypoint | `Cargo.toml` workspace lints |

## Invocation

```bash
# Unix / Git Bash on Windows / WSL
bash .claude/oracles/pii-scrub-audit/oracle.sh

# or Python directly:
python .claude/oracles/pii-scrub-audit/oracle.py
```

Exit `0` on PASS, `1` on FAIL, `2` on environment failure (Python not found).

## Output schema

```
PASS pii-scrub-audit
  Probe A (no tracing setup outside crates/feedbackmonk-tracing/): clean
  Probe B (CANONICAL_PATTERNS hash matches expected_hash.txt): clean
```

or

```
FAIL pii-scrub-audit (N offender(s))

Probe A offenders (...):
  <file>:<line>  forbidden tracing-subscriber setup '<label>' outside crates/feedbackmonk-tracing/

Probe B failure (canonical pattern-set hash):
  pattern-set hash drift: actual=<hex> expected=<hex> (parsed N patterns; review every tuple in <path>)
```

## Updating the pattern set

1. Edit `crates/feedbackmonk-tracing/src/scrubber.rs`. Keep the existing 20
   canonical patterns byte-for-byte unless the GitCellar source has changed.
2. Run `cargo test -p feedbackmonk-tracing canonical_hash` — the test prints the
   current SHA-256.
3. Copy the printed hash into `expected_hash.txt`.
4. Run `python .claude/oracles/pii-scrub-audit/oracle.py` — expect PASS.
5. Commit the scrubber change, the hash file change, and any new pattern test
   together — reviewers can audit the pattern delta in one diff.

## Lineage

- **FR-FBR-10** — PII scrubber with canonical 20-pattern regex set
- **DEC-FBR-01** Persona D — privacy
- **D-FBR-02** — three-leg defense pattern (type/oracle/lint)
- **DEC-FBR-IMPL-03** — Python-canonical oracle implementations
- **P1 plan §Oracle Pre-Build Plan**
- **GitCellar reference**: `gitcellar-service/src/feedback_logs/scrubber.rs`

## Decision log

- **Pattern shape**: `(&str, &str, &str)` — `(name, regex, replacement)`.
  GitCellar's source uses a `Rule { re, replacement }` struct with the name
  only in comments. We promote name to first slot so the oracle parser can
  extract diagnostics + the hash includes the human label (reordering rows
  is also caught even if regex/replacement are identical).
- **Hash serialisation**: tab-separated `name\tregex\treplacement\n` per row,
  UTF-8. Trivial format so Rust-side tests can reproduce it byte-for-byte
  without depending on the Python oracle's parser.
- **Probe A regex specificity**: the brief's loose `impl.*Layer.*for` would
  false-positive on `TraceLayer::new_for_http()` from tower-http. The
  tightened regex `\bimpl\b[^;{]*\bLayer\s*<[^>]*>\s+for\b` requires an
  actual `impl ... Layer<...> for ...` block opener.
