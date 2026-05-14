# Test Modification Justification — P1 Stage 1 (Foundation Contracts + PII Oracle)

**Date**: 2026-05-13
**Session**: S001 (autopilot:continuous, mid-arc)
**Plan**: `docs/planning/plans/20260513T231115-feedbackmonk-p1-closes-the-loop.md`
**Worker**: Orchestrated Execution worker (terminated cleanly, dev-complete report at `ltads/execution/development-complete.md`)

## Summary

P1 Stage 1 NEW test authoring for NEW code (FR-FBR-10 PII scrubber + status workflow contracts C6/C7/C8/C9/C10). The 118 pre-existing P0 tests are UNCHANGED. Net: 118 → 185 (+67 new tests).

## Per-Contract Justification

Each new test verifies a documented invariant from the P1 plan's frozen contracts:

| Contract | Invariant verified | New tests |
|---|---|---|
| **C6** (status state machine, FR-FBR-08) | 6 canonical kebab-case status values; `legal_transitions_from` returns only legal targets; `TransitionError` on illegal transitions | 7 new unit tests in `crates/feedbackmonk-core/src/status.rs` |
| **C7** (status transition API + audit row contract) | Atomic write to `feedback.status` + insert into `feedback_status_history`; reason/actor required; tenant-scope discipline | 4 new sqlx::test cases in `crates/feedbackmonk-repository/src/feedback_status_history.rs` |
| **C8** (admin list/get API) | `FeedbackRepo::list_for_admin` returns only tenant-scoped feedback with status filter; `get_with_history` returns Feedback + ordered history rows | New sqlx::test cases in `crates/feedbackmonk-repository/src/feedback.rs` (cross-tenant negatives + status_filter + history ordering) |
| **C9** (PII scrubber, FR-FBR-10) | Each of the 20 canonical patterns matches positive cases and rejects near-miss cases; SHA-256 lock detects drift; `install_global_subscriber` chokepoint covers all log emission paths | 41 unit tests in `crates/feedbackmonk-tracing/src/scrubber.rs` (positive + near-miss per pattern) + 7 integration tests in `crates/feedbackmonk-tracing/tests/scrubber_patterns.rs` (end-to-end via subscriber install + bilateral SHA-256 check) |
| **C10** (email-brand parameters) | `TenantRepo::get_brand` returns scope-bound `EmailTenantBrand`; `update_brand` writes only within tenant scope; `create` populates defaults matching migration 00005 backfill | New sqlx::test cases in `crates/feedbackmonk-repository/src/tenants.rs` |

## Anti-Reward-Hacking Posture

- **No pre-existing tests modified.** P0's 118 tests run unchanged; `cargo test --workspace` shows them all green.
- **No assertions weakened to make passing easier.** New assertions verify documented contract invariants from the P1 plan, not implementation details.
- **Bilateral verification on PII scrubber.** The 20-pattern set has its SHA-256 locked in `.claude/oracles/pii-scrub-audit/expected_hash.txt` AND in a bilateral runtime check at `crates/feedbackmonk-tracing/tests/scrubber_patterns.rs::canonical_hash_matches_expected_file`. Either source of truth drifting (oracle file or source code) fails the test. This is the matrix `MATRIX-CAT-GOLDEN-OUTPUT` shape with a deterministic backstop.
- **Verification Oracle defense-in-depth.** `pii-scrub-audit` Probe A (AST-level: forbid `impl Layer<...> for ...` outside crate) + Probe B (hash drift) — independent of the unit tests, catches a different drift surface (semantic-similarity false negatives in tests would still flunk Probe A).
- **Cross-tenant negatives** added for every new repo method (scope discipline preserved per `multi-tenant-isolation-check` oracle, which remains GREEN).

## Categorization (matrix-aware)

| New test file | Feature shape | Matrix category | Lane |
|---|---|---|---|
| `feedbackmonk-core/src/status.rs` (7 tests) | Pure function (state machine) | `MATRIX-CAT-PBT` adjacent (exhaustive enumeration over the 6-value state space is finite — equivalent to PBT for small state) | fast-inner |
| `feedbackmonk-repository/src/feedback_status_history.rs` (4 tests) | Stateful component | `MATRIX-CAT-METAMORPHIC` (atomic write invariant) + scope-discipline negatives | slow-outer (sqlx::test) |
| `feedbackmonk-repository/src/feedback.rs` (new cases) | Stateful component | scope-discipline negatives + ordering invariants | slow-outer (sqlx::test) |
| `feedbackmonk-repository/src/tenants.rs` (new cases) | Stateful component + brand defaults | scope-discipline + invariant (defaults match migration backfill) | slow-outer (sqlx::test) |
| `feedbackmonk-tracing/src/scrubber.rs` (41 tests) | Pure function (regex apply) | `MATRIX-CAT-PBT`-adjacent (positive + near-miss per pattern) | fast-inner |
| `feedbackmonk-tracing/tests/scrubber_patterns.rs` (7 tests) | Cross-cutting concern (logging) | `MATRIX-CAT-GOLDEN-OUTPUT` (canonical pattern set hash) + end-to-end install | slow-outer |

## Risk Tier

Per `claude-template/templates/TEST_CATEGORIZATION_MATRIX.md` Part 5: all new tests are Tier 1 (new-test authoring against frozen contracts; no modification of existing baselines). No Tier 2+ gate triggers.

## Justification String (for Phase 0.5)

> P1 Stage 1 NEW test authoring for NEW code per docs/planning/plans/20260513T231115-feedbackmonk-p1-closes-the-loop.md (FR-FBR-10 + status workflow contracts). New tests: 20 pattern unit tests + 7 scrubber integration tests + 4 FeedbackStatusHistoryRepo cases + new TenantRepo brand cases + FeedbackRepo list_for_admin/get_with_history cases. P0's 118 tests are UNCHANGED. Each new test verifies a documented invariant from the P1 plan's Contract C6/C7/C8/C9/C10 (state machine legal transitions, audit row atomicity contract, PII scrub canonical-pattern coverage, scope-discipline cross-tenant negatives).
