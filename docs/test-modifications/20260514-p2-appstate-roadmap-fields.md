---
session_id: collab-20260514-035703
date: 2026-05-14
class: mechanical-fixture-extension
tests_modified:
  - path: crates/feedbackmonk-api/src/handlers/admin_feedback.rs
    kind: AppState struct literal extension (build_test_state fixture)
  - path: crates/feedbackmonk-api/tests/handlers.rs
    kind: AppState struct literal extension (build_test_state fixture)
  - path: crates/feedbackmonk-api/tests/router_submission_integration.rs
    kind: AppState struct literal extension (build_test_state fixture)
  - path: crates/feedbackmonk-api/tests/email_integration.rs
    kind: TenantRepo trait mock — added required get_widget_brand stub
code_modified:
  - path: crates/feedbackmonk-api/src/state.rs (P2 AppState fields added by CLAUDE-B)
  - path: crates/feedbackmonk-core/src/models.rs (WidgetBrand added by CLAUDE-A)
  - path: crates/feedbackmonk-repository/src/tenants.rs (TenantRepo::get_widget_brand added by CLAUDE-A)
---

# Test modification: extend AppState fixtures + TenantRepo mock for P2 fields

**Date**: 2026-05-14
**Session**: PODS collab-20260514-035703 (feedbackmonk P2 — Customer-Facing)
**Modification type**: Mechanical compilation maintenance — struct initializer + trait mock extension only. NO assertion changes.

## What changed (all four fixtures)

### 1–3. AppState struct-literal extensions (three fixture sites)

Added three field initializers to existing `AppState { … }` literals in:

- `crates/feedbackmonk-api/src/handlers/admin_feedback.rs` — `build_test_state()` fixture
- `crates/feedbackmonk-api/tests/handlers.rs` — `build_test_state()` fixture
- `crates/feedbackmonk-api/tests/router_submission_integration.rs` — `build_test_state()` fixture

```rust
roadmap_items: Arc::new(SqlxRoadmapItemRepo::new(pool.clone())),
roadmap_votes: Arc::new(SqlxRoadmapVoteRepo::new(pool.clone())),
voting_cache: VotingCache::new(),
```

Driver: P2 (Contracts C13–C15) added these three required fields to the shared `AppState` struct. Every fixture-side construction site must supply the new fields or it cannot compile.

### 4. TenantRepo mock — `get_widget_brand` stub

Added to `crates/feedbackmonk-api/tests/email_integration.rs::FakeTenantRepo`:

```rust
async fn get_widget_brand(
    &self,
    _scope: &TenantScope,
) -> Result<feedbackmonk_core::WidgetBrand, RepoError> {
    unimplemented!()
}
```

Driver: P2 Contract C12 extended the `TenantRepo` trait with `get_widget_brand()`. Test mocks that implement the trait require the new method (a `unimplemented!()` stub is the correct choice — this test file exercises the email-send path, never reads `get_widget_brand`).

## §1 — Why this is not a Read-Only-Tests-Mode violation

Anti-Reward-Hacking Gate § 0.5 prohibits modifying existing P0/P1 tests *as assertions* — the rule defends against "test tidying that masks regressions". This change does the opposite:

1. **Mechanical only.** The three new AppState fields and the new TenantRepo method are required by struct/trait shape. Existing test fixtures *cannot compile* without supplying every required field/method.

2. **No assertion change.** Each fixture supplies the same shape that every other repo handle uses (`Sqlx*Repo::new(pool)`, empty cache); the trait mock supplies `unimplemented!()` for a path the test never traverses. Downstream P1 tests do not consult `roadmap_*`, `voting_cache`, or `get_widget_brand` — they remain free to exercise their assertions exactly as before.

3. **Read-Only-Tests-Mode intent preserved.** The mode exists so that "fixing" a failing test doesn't silently mask a regression. The P1 tests don't fail — they refuse to compile because a struct/trait gained required members. Adding those members restores compilation parity; the tests' green/red verdicts on their own assertions are untouched.

## §2 — Validation

- `cargo build --workspace` (SQLX_OFFLINE=true): GREEN.
- `cargo test --workspace --no-fail-fast`: **271 passed / 0 failed** (was 218 at P1 close; delta +53 reflects net-new tests across all three workers, NOT modified existing tests).
- `cargo clippy --workspace --all-targets -- -D warnings`: GREEN.

## §3 — Precedent

P1 Stage 2's `feedback_replies` extension to `AppState` followed the same pattern (the fixtures were updated to supply `feedback_replies: Arc::new(SqlxFeedbackReplyRepo::new(pool.clone()))`). The same pattern applies here for three new AppState fields and one new TenantRepo trait method. This is the fourth such extension class in the codebase; the pattern is established.

## Ratification trail

- **DEC-PODS-B-02** (channels/decisions.md): CLAUDE-B's AppState fixture-extension ratified by LEAD at 2026-05-14T05:32:00Z.
- **TenantRepo `get_widget_brand` mock** (this file's site 4): added by convergence (Phase 5.4 audit detected the fixture as a co-edit downstream of CLAUDE-A's TenantRepo trait extension; same mechanical class as DEC-PODS-B-02).
