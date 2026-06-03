---
session_id: interactive-20260603-login
date: 2026-06-03
class: new-tests-for-new-code + mechanical-fixture-extension
tests_modified:
  - path: crates/feedbackmonk-api/tests/handlers.rs
    kind: NEW login test suite (5 tests) + AppState struct-literal extension (login_gate)
  - path: crates/feedbackmonk-api/src/handlers/admin_feedback.rs
    kind: AppState struct-literal extension (build_test_state fixture — login_gate)
  - path: crates/feedbackmonk-api/src/handlers/admin_tier.rs
    kind: AppState struct-literal extension (build_test_state fixture — login_gate)
  - path: crates/feedbackmonk-api/src/handlers/promote.rs
    kind: AppState struct-literal extension (build_test_state fixture — login_gate)
  - path: crates/feedbackmonk-api/tests/me_feedback_isolation.rs
    kind: AppState struct-literal extension (build_test_state fixture — login_gate)
  - path: crates/feedbackmonk-api/tests/router_submission_integration.rs
    kind: AppState struct-literal extension (build_test_state fixture — login_gate)
  - path: crates/feedbackmonk-api/tests/tier_enforcement_smoke.rs
    kind: AppState struct-literal extension (build_test_state fixture — login_gate)
  - path: crates/feedbackmonk-anon/src/lib.rs
    kind: NEW LoginGate unit tests (3 tests) added to existing #[cfg(test)] module
code_modified:
  - path: crates/feedbackmonk-api/src/handlers/login.rs (NEW — POST /api/v1/login handler)
  - path: crates/feedbackmonk-api/src/handlers/mod.rs + router.rs (route wiring)
  - path: crates/feedbackmonk-api/src/state.rs + main.rs (AppState.login_gate field + env wiring)
  - path: crates/feedbackmonk-anon/src/lib.rs (NEW LoginGate primitive)
---

# Test modification: admin login endpoint tests + AppState `login_gate` fixture extension (DEC-FBR-IMPL-10)

**Date**: 2026-06-03
**Modification type**: (a) NET-NEW tests for NET-NEW functionality, and (b) mechanical
struct-initializer extension. **NO existing assertion was changed or weakened.**

## What changed

### A. Net-new tests for net-new code (legitimate added coverage)

- `crates/feedbackmonk-anon/src/lib.rs` — **3 new** `LoginGate` unit tests
  (`login_key_hash_is_deterministic_and_input_sensitive`, `login_rate_limit_trips_after_quota`,
  `login_rate_limit_buckets_are_independent`) appended to the existing `#[cfg(test)] mod tests`.
- `crates/feedbackmonk-api/tests/handlers.rs` — **5 new** integration tests for the new
  `POST /api/v1/login` endpoint (happy-path → working admin-gated request, wrong-password 401,
  unknown-email generic 401, unverified-but-correct-password 403, rate-limit 429-before-argon2)
  plus a `login_request` helper. These test code that did not exist before this commit.

### B. Mechanical AppState struct-literal extension (7 fixture sites — `login_gate`)

DEC-FBR-IMPL-10 added one required field to the shared `AppState` struct
(`login_gate: LoginGate`). Every construction site must supply it or the crate cannot compile.
Each fixture supplies the same value:

```rust
login_gate: feedbackmonk_anon::LoginGate::with_default_quota(),
```

Sites (all `build_test_state`-style fixtures): `handlers/admin_feedback.rs`, `handlers/admin_tier.rs`,
`handlers/promote.rs`, `tests/handlers.rs`, `tests/me_feedback_isolation.rs`,
`tests/router_submission_integration.rs`, `tests/tier_enforcement_smoke.rs`. (`main.rs` is the
production construction site — not a test fixture.)

## §1 — Why this is not a Read-Only-Tests-Mode / reward-hacking violation

Anti-Reward-Hacking Gate § 0.5 defends against editing existing tests *as assertions* to mask a
regression. This change does the opposite on both counts:

1. **Part A is purely additive.** New tests for a new endpoint and a new rate-limiter; they add
   coverage and cannot mask any existing regression. They assert real behavior (401/403/429 codes,
   a login-minted cookie authenticating an `AdminSession`-gated route, the 429 firing *before* the
   argon2 verify).
2. **Part B is mechanical, no assertion change.** The `login_gate` field is required by struct
   shape; existing fixtures cannot compile without it. Every site supplies the identical default-quota
   value. No existing test's inputs, expected outputs, or assertions changed. The pre-existing tests
   keep their exact green/red verdicts on their own assertions.
3. **Identical to established precedent** — the P2 `roadmap_*`/`voting_cache` and P3 `tier_quotas`
   AppState extensions (`20260514-p2-appstate-roadmap-fields.md`,
   `20260514-p3-appstate-tier-quotas.md`) are the same mechanical class. This is the next such
   extension; the pattern is established.

## §2 — Validation

- `cargo check -p feedbackmonk-anon -p feedbackmonk-api --all-targets` (SQLX_OFFLINE): GREEN.
- `cargo clippy …  --all-targets` (pedantic, deny-warnings): GREEN.
- `cargo test -p feedbackmonk-anon -p feedbackmonk-api`: **all GREEN, 0 failed**. anon lib 14 passed
  (was 11; +3 LoginGate), api `handlers.rs` 18 passed (was 13; +5 login), api lib 126, plus
  cors 4 / me_feedback 7 / submission 5 / tier 3 / pii-corpus 3 / email 1 — all unchanged and green.
- Verification Oracles: `multi-tenant-isolation-check` (no new unscoped repo method — reuses the
  already-allowlisted `find_by_email`), `cors-allowlist-enforcement`, `feedback-parity-status` 4/4,
  `pii-scrub-audit` — all GREEN.

## §3 — No `.sqlx` change

The login handler calls only the pre-existing, already-cached `TenantRepo::find_by_email`; it adds
no `sqlx::query!` macro. The offline `.sqlx` cache is untouched.
