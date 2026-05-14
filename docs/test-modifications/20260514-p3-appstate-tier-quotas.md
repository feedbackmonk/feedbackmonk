---
title: P3 Stage 1 — AppState `tier_quotas` field extension
author: P3 Stage 1 worker (session-20260514-102233-006)
date: 2026-05-14
mode: mechanical-fixture-extension
related_contract: C17 (P3 plan §Interface Contracts)
related_decision: DEC-FBR-03 (pricing tier matrix); DEC-FBR-DEFER-01 (Polar deferral)
lineage: docs/planning/plans/20260514T134816-feedbackmonk-p3-commercial-gate.md §Stage 1 Phase 3
oracle_gates:
  - tier-enforcement-status (Probe A handler coverage)
  - multi-tenant-isolation-check (Probe B + allowlist for SqlxTierQuotaRepo::new)
tests_modified:
  - crates/feedbackmonk-api/src/handlers/admin_feedback.rs (test-only `build_test_state` constructor)
  - crates/feedbackmonk-api/src/handlers/promote.rs (test-only `build_test_state` constructor)
  - crates/feedbackmonk-api/tests/handlers.rs (top-level fixture)
  - crates/feedbackmonk-api/tests/router_submission_integration.rs (top-level fixture)
  - crates/feedbackmonk-api/tests/email_integration.rs (FakeTenantRepo impl — 3 method stubs)
shape: append-only struct field; identical pattern to P2 D-FBR-17's roadmap fields extension
---

# AppState extension for tier-cap predicate

P3 Stage 1 adds a `tier_quotas: Arc<dyn TierQuotaRepo>` field to
`crates/feedbackmonk-api/src/state.rs::AppState`. The field is required
by every domain-write handler post-Phase-4 (Contract C17), and is
constructed in `main.rs::build_state` from a `SqlxTierQuotaRepo` wrapping
the binary's `PgPool`.

This is a **mechanical fixture extension** — every `AppState { ... }`
literal in the workspace gains the `tier_quotas:` field. No test
assertions change; the new field is invisible to existing test paths
(none of which exercise the cap-check predicate before Phase 4 wiring).

## Fixture sites enumerated upfront (lesson from P2 D-FBR-17)

The P2 worker (`feedbackmonk` P2 Customer-Facing) missed one fixture
site during the AppState roadmap-fields extension, surfacing as a
compile error mid-test-run. This time we enumerate the full set before
editing, then `git diff --name-only` cross-checks the list at exit.

### 4 production fixture sites (sources of `AppState { ... }` literal)

| Path | Kind | New field | Tier value |
|---|---|---|---|
| `crates/feedbackmonk-api/src/main.rs` (`build_state`) | binary fixture | `tier_quotas = Arc::new(SqlxTierQuotaRepo::new(pool.clone()))` | Real predicate (Free default) |
| `crates/feedbackmonk-api/src/handlers/admin_feedback.rs` (test-only `build_test_state`) | test fixture | same | tenants seed at Free; tests stay under cap |
| `crates/feedbackmonk-api/src/handlers/promote.rs` (test-only `build_test_state`) | test fixture | same | tenants seed at Free; tests stay under cap |
| `crates/feedbackmonk-api/tests/handlers.rs` (top-level `build_test_state`) | top-level test fixture | same | tenants seed at Free; tests create ≤1 project, 0 feedback |
| `crates/feedbackmonk-api/tests/router_submission_integration.rs` (top-level `build_test_state`) | top-level test fixture | same | tenants submit ≤4 feedback; Free cap = 50 |

That is 5 total `AppState { ... }` constructions in the workspace; one
binary + four test sites.

### 1 trait-impl mock that needs the new `TenantRepo` methods stubbed

| Path | Kind | New methods | Stub |
|---|---|---|---|
| `crates/feedbackmonk-api/tests/email_integration.rs::FakeTenantRepo` | trait impl mock | `get_tier`, `count_projects`, `count_feedback_in_window` | `unimplemented!()` (the file's test path exercises only `get_brand`) |

Mailpit integration test was the only `impl TenantRepo for ...` outside
`SqlxTenantRepo`; verified via:

```text
grep -rn "impl.*TenantRepo for" crates/ --include="*.rs"
```

→ 2 results: `crates/feedbackmonk-api/tests/email_integration.rs:50` +
`crates/feedbackmonk-repository/src/tenants.rs:132` (the real impl).

### `git diff --name-only` cross-check (Stage 1 exit gate)

At Phase 7 verification, the worker re-runs:

```bash
git diff --name-only | grep -E '(state\.rs|main\.rs|handlers/(admin_feedback|promote)\.rs|tests/(handlers|router_submission_integration|email_integration)\.rs)$'
```

The matched set MUST equal the four production fixture sites + the one
mock site listed above. Mismatch → fail Stage 1 exit gate; re-audit.

## Why a test-mod justification artifact?

Per ULDF Probandurgy discipline + DEC-FBR-IMPL-* "test modifications
require justification". The risk surface is twofold:

1. **Test fixture rot** — appending a field silently to a test fixture
   means the test no longer asserts the production wiring shape. The
   `git diff --name-only` cross-check is the drift defender.
2. **Mock divergence** — the `FakeTenantRepo` in `email_integration.rs`
   stubs three new methods with `unimplemented!()`. If the email-send
   path ever reaches one of those at runtime, the test panics loudly
   rather than silently producing a wrong answer.

The Stage 1 worker reads this doc at Phase 3 of the brief, executes the
edits, then re-verifies at Phase 7. The cross-check command is the
exit-gate artifact.

## Mode classification

**Mechanical fixture extension**. No assertion semantics change. The
field is required to satisfy the `AppState` struct literal — Rust's
exhaustive-fields check is the type-system guarantee that no fixture
site was missed at compile time.

## Rollback plan

If P3 Stage 1 needs to be reverted, remove the `tier_quotas` field from
`AppState`, remove `SqlxTierQuotaRepo::new` from
`multi-tenant-isolation-check/allowlist.toml`, and re-run the oracle
suite. The five fixture sites un-need the field automatically once the
struct definition is reverted.
