# DISCOVERIES

Insights surfaced during Feedbackr implementation that are worth preserving beyond the session-completion report. Append-only; newest at bottom.

---

## P0 Stage 1 (2026-05-13)

### D-FBR-01: Verification Oracle authoring — Python beats pure shell when parsing crosses lines

**Surfaced by**: `multi-tenant-isolation-check` oracle authoring (Stage 1 worker).

**What was discovered**: A pure-bash implementation of Probe B (Rust signature first-arg parsing) produced 25 false positives on a clean tree due to POSIX shell's inability to track context across line continuations. The fix — Python 3.8+ as canonical with shell shims — was clean and uncovered a reusable pattern.

**Generalizable insight**: Verification Oracles that need to parse syntax across lines (signatures, brace-balanced blocks, multi-line config) should default to **Python canonical + thin OS shims** rather than pure shell. The mental model:

| Parser need | Tool |
|---|---|
| Single-line grep / pattern match | Pure shell (`grep`, `awk`) |
| Cross-line / balanced-delimiter parsing | Python (`re` + manual tokenization), shell shims forward |
| Structural AST manipulation | tree-sitter or syn-cli (Rust-specific) |

**Where this pays off again**: Future Feedbackr Verification Oracles likely to need this pattern — `pii-scrub-audit` (P1, drift-detection over a canonical pattern set with regex alternation requiring balanced parens), `tier-enforcement-status` (P3, parses Rust enum + match-arm structure to verify cap-check completeness). Documented in DEC-FBR-IMPL-03.

---

### D-FBR-02: Three-leg defense pattern generalizes — type system + AST oracle + lint baseline

**Surfaced by**: P0 Stage 1's tenant-isolation defense design.

**What was discovered**: The strongest defenses against silent fidelity failures (Q2=5 surfaces in Testability Gate language) combine **three independent legs**:

1. **Type system** — newtypes / sealed traits / `pub(crate)` constructors that make the invariant unrepresentable in incorrect code (when it works).
2. **AST oracle** — Verification Oracle that greps for forbidden patterns and checks structural invariants on every commit (catches what types miss, e.g. raw-SQL strings that the type system cannot see through).
3. **Lint baseline** — clippy / cargo-deny / project-specific lints that catch foot-guns at compile time and refuse to let the build pass with warnings.

The legs are **independent** — a bug that defeats one likely does not defeat the others. Two legs alone is fragile (every safety-critical exploit history is "the one mechanism failed"); three legs is resilient.

**Generalizable insight**: For any future high-Q2 invariant in Feedbackr (JWT verifier alg-confusion, PII scrubber pattern drift, tier-cap enforcement, AGPL header presence), design the defense as a three-leg architecture from the start. Pick what each leg checks; pick patterns that are genuinely independent (a single underlying bug should not invalidate two legs at once).

**Where this pays off again**:

- **FR-FBR-05 JWT verification** (Stage 2 Worker B): leg 1 = JWT-library API + per-aud type guard; leg 2 = JWT fixture corpus with named tests per invariant; leg 3 = clippy + a possible drift-detection oracle over the corpus signature.
- **FR-FBR-10 PII scrub** (P1): leg 1 = central scrubber function with a single chokepoint; leg 2 = `pii-scrub-audit` oracle; leg 3 = clippy/build-time pattern-count check.
- **FR-FBR-14 tier enforcement** (P3): leg 1 = enum-exhaustive `match` on tier; leg 2 = `tier-enforcement-status` oracle; leg 3 = workspace pattern check that every cap is paired with a tier check.

---

### D-FBR-03: Pre-auth allowlist is a recurring shape — name the boundary explicitly

**Surfaced by**: `scope_for` allowlist debate during P0 Stage 1.

**What was discovered**: Type-system isolation (e.g. `TenantScope` with `pub(crate)` constructor) **necessarily** has a small set of methods that mint the first scope from a verified caller. The naive instinct ("just have private fields and let the compiler enforce it") doesn't work because *something* has to bridge "verified credential" → "scope value" at the auth boundary.

The pattern: name those bridge methods explicitly, gate them through an allowlist with per-entry rationale, and trigger oracle re-runs when the allowlist changes. This makes the "what crosses the boundary" question audit-friendly (a 30-line `allowlist.toml` shows the entire attack surface) instead of buried in source code.

**Generalizable insight**: Any type-system isolation boundary in Feedbackr (signing-key access, anonymous-mode rate-limit counters, future RBAC scopes, future organization-level admin scopes) should follow the same pattern: `pub(crate)` constructor + named allowlist of bridge methods + oracle that enforces.

**Where this pays off again**: Stage 2 Worker B's `verify()` on the JWT crate is a similar boundary — only one method mints "verified payload" from raw token bytes. That allowlist would be a single entry but the same shape (explicit, rationale-bearing, oracle-triggered).

---

### D-FBR-04: Test-categorization opportunity — property-based tests for cross-tenant rejection

**Surfaced by**: Phase 5 (Test Maintenance) categorization during this finalize.

**What was discovered**: Stage 1's 19 tests are all example-based (specific tenants A and B with specific projects). They are **correct** and pass per the matrix as `MATRIX-CAT-DIFFERENTIAL` (cross-tenant: same input, two implementations — implementations of "isolate by tenant" via repo trait — must produce non-overlapping outputs). What is **not yet present**:

- **`MATRIX-CAT-PBT` test (property-based)** for `ProjectRepo::open()` — assert: "for all `(tenant_a, tenant_b)` with `a ≠ b` and any `project_id` belonging to `b`, `open(&tenant_a_scope, project_id)` returns `TenantProjectMismatch`." Currently covered by 1 example; PBT with `proptest` would generalize.
- **`MATRIX-CAT-PBT` test for `FeedbackId::generate`** — assert: "for all 1000 generated IDs, none collide and all match `FB-\d+` format." Currently covered by uniqueness test that runs once.

**Generalizable insight**: Whenever the Testability Gate scores Q2=5 on a requirement, follow up Stage-N example tests with Stage-N+1 property tests using the same trait surface. The PBT companion crate (`proptest`) is a cheap add (one Cargo.toml line) and converts each invariant into a "for all" statement instead of a "for this one" statement.

**Status**: Recorded as a recommendation for Stage 2 Worker B's test addition (matches the JWT fixture corpus shape mandated by the Testability Gate for FR-FBR-05). Existing 19 tests stay IMMUTABLE per project CLAUDE.md byte-for-byte rule.

---
