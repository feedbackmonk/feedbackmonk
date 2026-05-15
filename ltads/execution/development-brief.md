# Development Brief — feedbackmonk P3 Stage 1

## ⚠️ COMPLETION PROTOCOL

When ALL tasks complete:
1. Write completion report to `ltads/execution/development-complete.md` (tasks done, files touched, test counts, oracle pass/fail, blockers)
2. Include actual test command output in Test Results section
3. EXIT this session (close terminal)
4. Return to orchestrator terminal — orchestrator will run `/0-uldf-finalize --skip-push` and auto-spawn Stage 2

**CRITICAL**:
- Do NOT run `/0-uldf-ltads-stop` (orchestrator does this)
- Do NOT run `git commit` (orchestrator does this via finalize)
- Do NOT modify LTADS tracking files (`ltads/execution/spec-progress.md`, etc. — orchestrator does this)
- Implement, run all gates, write report, exit

## Session
- **ID**: S002 (worker sub-session)
- **Generated**: 2026-05-14T14:11:05Z
- **Strategy**: Orchestrated Execution (single Tier 1 worker)
- **Phase**: P3 (Commercial Gate), Stage 1 (Backend tier model + enforcement + oracle)

## Mission

Implement feedbackmonk P3 Stage 1 per `docs/planning/plans/20260514T134816-feedbackmonk-p3-commercial-gate.md` § Stage 1. Three deliverables:

1. **Backend tier model** — `Tier` enum (Free/Starter/Pro/SelfHost) with serde + sqlx codec; `tier_quotas()` const fn; `TierQuotaRepo` trait + `SqlxTierQuotaRepo` impl with `check_tier_quota` predicate.
2. **Cap-check enforcement** — wire `check_tier_quota` into project-create + feedback-submission handlers; new `ApiError::TierCapExceeded` variant → HTTP 402/409; admin tier-status endpoint.
3. **`tier-enforcement-status` Verification Oracle** — three-probe (AST scan + config-shape + integration smoke); Task Zero (built BEFORE cap-check wiring).

Polar billing is **DEFERRED** per user direction — write a stub at `docs/deferred/polar-integration.md` and add `DEC-FBR-DEFER-01` to DECISIONS.md. DO NOT implement Polar webhook receiver. Admin Upgrade button (Stage 2) will be a stub ("Contact support to upgrade").

## Context Files (read in this order; skim, don't deep-read)

1. `docs/planning/plans/20260514T134816-feedbackmonk-p3-commercial-gate.md` — **THE PLAN** (§ Stage 1 has phase-by-phase steps; § Interface Contracts has C17/C18/C19 verbatim text for the handoff doc)
2. `docs/planning/plans/20260513T185711-feedbackmonk-v1-build-arc.md` — § P3 (arc context)
3. `docs/specs/SPECIFICATION.md` — FR-FBR-14 + § Oracles table
4. `docs/specs/DECISIONS.md` — DEC-FBR-03 (pricing tier matrix; load-bearing for `tier_quotas()` shape)
5. `.claude/oracles/widget-bundle-size/` — **canonical Verification Oracle pattern** to mirror (oracle.py + manifest + sh/ps1 shims + README)
6. `.claude/oracles/multi-tenant-isolation-check/` — Probe A AST-scan reference + allowlist.toml pattern (you'll append `SqlxTierQuotaRepo::new` here)
7. `crates/feedbackmonk-core/src/lib.rs`, `models.rs` — module structure
8. `crates/feedbackmonk-repository/src/tenants.rs`, `scope.rs`, `lib.rs` — `get_widget_brand` lives here; you'll modify it
9. `crates/feedbackmonk-api/src/state.rs`, `error.rs`, `main.rs`, `lib.rs` — AppState + ApiError + build_state
10. `crates/feedbackmonk-api/src/handlers/{projects.rs, feedback.rs, mod.rs, admin_feedback.rs}` — cap-check wiring sites + fixture pattern
11. `crates/feedbackmonk-api/tests/handlers.rs`, `tests/router_submission_integration.rs` — AppState fixture sites (3 of 4)
12. `migrations/00001_p0_schema.sql` — verify `tenants.tier TEXT DEFAULT 'free'` shape (may need a `CHECK` constraint migration; decide which is cleaner — sqlx codec OR schema CHECK)
13. `CLAUDE.md` — project context (Q24 invariant, dev ports, Polar deferral, finalize --skip-push requirement)

## Tasks (14 total — execute in phase order; phases gate each other)

| ID | Phase | Description | Files to Create/Modify | Priority |
|---|---|---|---|---|
| P3-S1-T0 | 0 (Task Zero) | Build `tier-enforcement-status` oracle (Probes A+B always; C gated behind `--full`). Vacuous-PASS at cold-start. | `.claude/oracles/tier-enforcement-status/{oracle.py,oracle.sh,oracle.ps1,manifest.json,manifest.toml,allowlist.toml,README.md}`; `.claude/oracles/INDEX.md` | Must |
| P3-S1-T1 | 1 | Tier model | NEW `crates/feedbackmonk-core/src/tier.rs`; modify `lib.rs` | Must |
| P3-S1-T2 | 2 | Tenants repo extensions (`get_tier`, tier-aware `get_widget_brand`, `count_projects`, `count_feedback_in_window`) + 4 sqlx::test | modify `crates/feedbackmonk-repository/src/tenants.rs` | Must |
| P3-S1-T3 | 2 | TierQuotaRepo trait + impl + 6 sqlx::test | NEW `crates/feedbackmonk-repository/src/tier_quota.rs`; modify `lib.rs`; append `.claude/oracles/multi-tenant-isolation-check/allowlist.toml` (entry: `SqlxTierQuotaRepo::new`); `cargo sqlx prepare --workspace -- --all-targets` | Must |
| P3-S1-T4 | 3 | AppState extension + test-mod justification (YAML frontmatter enumerating ALL fixture sites: handlers/admin_feedback.rs, tests/handlers.rs, tests/router_submission_integration.rs, plus any TenantRepo mock impl needing `get_tier`/`count_*` stubs) | modify `crates/feedbackmonk-api/src/state.rs`, `main.rs`; touch all fixture sites; NEW `docs/test-modifications/20260514-p3-appstate-tier-quotas.md` | Must |
| P3-S1-T5 | 3 | `ApiError::TierCapExceeded { tier, resource, current, limit, upgrade_hint }` → 402 (FeedbackInRollingMonth) / 409 (Project) per Contract C18 | modify `crates/feedbackmonk-api/src/error.rs` | Must |
| P3-S1-T6 | 4 | Wire `check_tier_quota(scope, ResourceKind::Project)` BEFORE INSERT in project-create handler | modify `crates/feedbackmonk-api/src/handlers/projects.rs` | Must |
| P3-S1-T7 | 4 | Wire `check_tier_quota(scope, ResourceKind::FeedbackInRollingMonth)` BEFORE INSERT in feedback-submission handler | modify `crates/feedbackmonk-api/src/handlers/feedback.rs` | Must |
| P3-S1-T8 | 5 | Admin tier-status endpoint `GET /api/v1/admin/tier` (AdminSession-gated) returning `TierStatus` JSON per Contract C17 + 3 handler unit tests | NEW `crates/feedbackmonk-api/src/handlers/admin_tier.rs`; modify `mod.rs`, `lib.rs`, `main.rs` route merge | Must |
| P3-S1-T9 | 6 | Dogfood SQL helper + per-tier capability matrix | NEW `docs/operations/TIER_OVERRIDE.md` | Must |
| P3-S1-T10 | 6 | Polar deferred stub (webhook shape, customer/subscription columns NOT migrated, `subscription.*` → `tenants.tier` mapping, port reference to `gitcellar-cloud/src/billing/polar.rs`) | NEW `docs/deferred/polar-integration.md` | Must |
| P3-S1-T11 | 6 | `DEC-FBR-DEFER-01: Polar billing deferred from P3` — one paragraph documenting user-direction-driven deferral; references this plan | append `docs/specs/DECISIONS.md` | Must |
| P3-S1-T12 | 7 | **Stage 1 exit gate hard requirement** — freeze Contracts C17/C18/C19 verbatim from plan § Interface Contracts + TypeScript starter kit for Stage 2's `types.gen.ts` | NEW `docs/planning/handoffs/p3-stage1-to-stage2.md` | Exit-gate |
| P3-S1-T13 | 7 | Run all gates (see Test Command + Oracle Re-run below) and report results in completion doc — DO NOT commit (orchestrator commits) | (verification only) | Exit-gate |

## Patterns

### Pattern 1: Verification Oracle (Python canonical + sh/ps1 shims)

Mirror `.claude/oracles/widget-bundle-size/` exactly for `tier-enforcement-status`. Probes:

- **Probe A (AST scan)**: Walk every `.rs` file under `crates/feedbackmonk-api/src/handlers/`. For each handler function (axum-routed), assert it either (a) calls `check_tier_quota(scope, ResourceKind::*)` before any data write, OR (b) appears in `allowlist.toml` with a justification comment. Same shape as `multi-tenant-isolation-check` Probe A.
- **Probe B (config-shape)**: Import/parse `crates/feedbackmonk-core/src/tier.rs`; assert `tier_quotas()` returns the canonical shape per tier (Free.projects=Some(1), Free.footer=Some("powered by feedbackmonk"), Starter.footer=None, etc. per Contract C19).
- **Probe C (integration smoke)**: Gate behind `--full` flag. Sqlx::test fixtures for each scenario: Free tenant creates 2nd project → 409 + structured `tier_cap_exceeded` body; Free tenant submits 51st feedback in 30-day window → 402 + same shape; `GET /widget-config` for Free tenant has `footer_text: Some("powered by feedbackmonk")`, for Pro returns None.

Cold-start vacuous-PASS plan: Probe A passes by allowlist (Phase 4 not wired yet); Probe B passes on bare `tier_quotas()` const; Probe C gated behind `--full` so cold-start doesn't fire it. Re-run `--full` after each Phase 4 wiring step.

### Pattern 2: Tenant-scoped repository (DEC-FBR-03 invariant)

Every method on `TierQuotaRepo` takes `&TenantScope` as first non-`&self` arg. Any raw SQL outside the `crates/feedbackmonk-repository/` layer is a security incident. Constructor `SqlxTierQuotaRepo::new` must be appended to `multi-tenant-isolation-check/allowlist.toml` (structural-mirror entry).

### Pattern 3: Sqlx codec for Rust enums (TEXT round-trip)

```rust
impl sqlx::Type<sqlx::Postgres> for Tier {
    fn type_info() -> sqlx::postgres::PgTypeInfo { <&str as sqlx::Type<sqlx::Postgres>>::type_info() }
}
impl sqlx::Encode<'_, sqlx::Postgres> for Tier { /* serialize lowercase: "free" | "starter" | "pro" | "self_host" */ }
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for Tier { /* FromStr, mirror serde lowercase */ }
```

Wire values (`'free' | 'starter' | 'pro' | 'self_host'`) per DEC-FBR-03. Decide at Phase 1: add a migration with `CHECK (tier IN (...))` OR enforce solely at the sqlx codec layer. Document the decision.

### Pattern 4: AppState extension (mirror P2's roadmap-fields addition)

```rust
// crates/feedbackmonk-api/src/state.rs
pub struct AppState {
    // ... existing fields ...
    pub tier_quotas: Arc<dyn TierQuotaRepo>,  // append-only
}
```

In `main.rs::build_state`:
```rust
let tier_quotas: Arc<dyn TierQuotaRepo> = Arc::new(SqlxTierQuotaRepo::new(pool.clone()));
AppState { /* existing */, tier_quotas }
```

Fixture sites (4 known, enumerate ALL in test-mod YAML frontmatter): `crates/feedbackmonk-api/src/handlers/admin_feedback.rs` (test-only `AppState` constructor), `tests/handlers.rs`, `tests/router_submission_integration.rs`, plus any TenantRepo trait-impl mock that needs `get_tier`/`count_projects`/`count_feedback_in_window` stub methods (`unimplemented!()` is fine for tests not exercising tier checks). Lesson from P2 D-FBR-17: **enumerate before editing**, then `git diff --name-only` cross-check before exit.

### Pattern 5: Cap-check wiring shape

```rust
// At top of handler, BEFORE any data write:
let status = state.tier_quotas
    .check_tier_quota(&scope, ResourceKind::Project)
    .await?;
if !status.allowed {
    return Err(ApiError::TierCapExceeded {
        tier: status.tier,
        resource: status.resource,
        current: status.current,
        limit: status.limit.unwrap_or(0),
        upgrade_hint: format!("Upgrade to Starter for 3 projects"),
    });
}
// ... existing INSERT ...
```

## Testing Context

> **Testing Required**: YES (9 of 14 tasks; 5 doc-only tasks have no tests)
> **Test Command**: `DATABASE_URL=postgres://postgres:dev@localhost:5433/feedbackmonk_dev cargo test --workspace --no-fail-fast`
> **Test Framework**: cargo-test + sqlx::test fixtures + 4 Verification Oracles

### Testing Guidance

- **Tier model (T1)**: Serde round-trip per tier; sqlx FromStr round-trip per tier; `tier_quotas()` shape assertion per tier (matches Contract C19 verbatim).
- **Tenants repo (T2)**: 4 sqlx::test integration tests — `get_tier` happy path, `get_widget_brand` Free returns footer Some / non-Free returns None, `count_projects` empty + non-empty, `count_feedback_in_window` empty + non-empty + window boundary.
- **TierQuotaRepo (T3)**: 6 sqlx::test integration tests — 3 tier × 2 resource combinations (Free × Project under cap → allowed; Free × Project at cap → blocked; Free × FeedbackInRollingMonth at 49 → allowed, at 50 → blocked; Pro × Project unlimited; Pro × FeedbackInRollingMonth at 9999 → allowed; SelfHost × * → unlimited).
- **AppError (T5)**: HTTP mapping unit tests — `TierCapExceeded { resource: Project, .. }` → 409; `TierCapExceeded { resource: FeedbackInRollingMonth, .. }` → 402; body shape matches Contract C18.
- **Cap wiring (T6, T7)**: Covered by Probe C integration smoke (Free tenant 2nd project → 409; Free tenant 51st submission → 402). Plus existing handler tests must keep passing (3 fixture sites get the new field; mock impls get stubs).
- **Admin tier endpoint (T8)**: 3 unit tests — Free tenant returns Free + correct quotas + live usage; admin-session required (401 without); pro tenant returns Pro + None projects limit.
- **Q24 invariant**: DO NOT modify `crates/feedbackmonk-api/src/handlers/promote.rs` render functions or any `q24_*` test. The byte-for-byte invariant is permanent (CLAUDE.md § Privacy invariants).

## Oracle Re-run Schedule

After each task completion (or at minimum after each phase):

```bash
# Fast loop (no --full, no integration):
python .claude/oracles/multi-tenant-isolation-check/oracle.py
python .claude/oracles/pii-scrub-audit/oracle.py
python .claude/oracles/widget-bundle-size/oracle.py
python .claude/oracles/tier-enforcement-status/oracle.py        # Probes A+B only

# Full loop (Phase 4+ when cap-wiring lands):
python .claude/oracles/tier-enforcement-status/oracle.py --full # adds Probe C integration
```

All 4 oracles MUST be GREEN at Stage 1 exit (T13).

## Success Criteria

- [ ] `cargo build --workspace --all-targets` GREEN
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` GREEN
- [ ] `cargo test --workspace --no-fail-fast` GREEN (target: ≥285 tests; +14 from P2's 271)
- [ ] `cargo sqlx prepare --workspace -- --all-targets` clean (Tier sqlx codec captured; new queries cached)
- [ ] All 4 Verification Oracles GREEN: `multi-tenant-isolation-check`, `pii-scrub-audit`, `widget-bundle-size`, `tier-enforcement-status --full`
- [ ] Smoke trio passes: Free tenant 2nd project → 409 + `tier_cap_exceeded` body; Free tenant 51st submission → 402 + same shape; `GET /widget-config` returns `footer_text: Some("powered by feedbackmonk")` for Free, `None` for Pro
- [ ] `docs/planning/handoffs/p3-stage1-to-stage2.md` written with Contracts C17 (TierQuotaRepo trait), C18 (TierCapExceededBody HTTP shape), C19 (`tier_quotas()` static config) frozen verbatim from plan + TS starter kit for `admin-ui/src/shared/types.gen.ts`
- [ ] `docs/test-modifications/20260514-p3-appstate-tier-quotas.md` written with YAML frontmatter enumerating ALL fixture sites (4 expected); cross-checked against `git diff --name-only`
- [ ] `docs/deferred/polar-integration.md` stub written; `DEC-FBR-DEFER-01` added to `docs/specs/DECISIONS.md`; `docs/operations/TIER_OVERRIDE.md` dogfood SQL helper written
- [ ] CLAUDE.md Oracle table — `tier-enforcement-status` row flipped to ✅ LIVE
- [ ] Q24 invariant preserved: `crates/feedbackmonk-api/src/handlers/promote.rs` render functions and `q24_*` tests unmodified

## Constraints (NON-NEGOTIABLE)

- **Polar billing DEFERRED** — no webhook receiver, no billing wiring, no Polar SDK calls. Only the deferred-doc stub at `docs/deferred/polar-integration.md` + `DEC-FBR-DEFER-01`. The Stage 2 admin Upgrade button will be a stub ("Contact support to upgrade") — do not implement a checkout flow.
- **`/0-uldf-finalize --skip-push`** — push is BLOCKED until PF-REGISTER-01 clears (github.com/feedbackmonk org + feedbackmonk.com purchase still pending). Orchestrator runs finalize, not worker; but if you need to verify locally, NEVER push to remote.
- **Q24 byte-for-byte invariant from P2 is PERMANENT** — do NOT touch `crates/feedbackmonk-api/src/handlers/promote.rs` render functions or any `q24_*` test (CLAUDE.md § Privacy invariants).
- **Local Postgres dev container**: `feedbackmonk-pg-dev` on port 5433 (`DATABASE_URL=postgres://postgres:dev@localhost:5433/feedbackmonk_dev`). Used for `cargo sqlx prepare`. (Container renamed from `feedbackr-pg-dev` in PF-RENAME-03 at 2026-05-15 post-arc-terminus.)
- **Test-mod justification artifact MUST enumerate ALL fixture sites** in YAML frontmatter — lesson from P2 D-FBR-17 where AppState extension missed sites. Expected 4 sites: `crates/feedbackmonk-api/src/handlers/admin_feedback.rs`, `tests/handlers.rs`, `tests/router_submission_integration.rs`, plus TenantRepo mock(s). Cross-check with `git diff --name-only` before claiming completion.
- **Stage 1 exit gate REQUIRES `docs/planning/handoffs/p3-stage1-to-stage2.md`** — Stage 2 cannot begin without Contracts C17/C18/C19 frozen verbatim in the handoff doc + TS starter kit. This is the contract-freeze handshake.
- **Tenant-scoped repository is sole query path** — any raw SQL outside `crates/feedbackmonk-repository/` is a security incident (DEC-FBR-03). New `SqlxTierQuotaRepo::new` constructor must be appended to `multi-tenant-isolation-check/allowlist.toml`.

## Completion Instructions

When ALL tasks complete:

1. Write completion report to `ltads/execution/development-complete.md` containing:
   - Tasks completed (T0–T12; T13 is orchestrator action)
   - Files created (list paths)
   - Files modified (list paths)
   - Test results: actual `cargo test --workspace --no-fail-fast` output (final test count + pass/fail)
   - Clippy result: actual `cargo clippy --workspace --all-targets -- -D warnings` output
   - Oracle results: actual output for each of 4 oracles (incl. `tier-enforcement-status --full`)
   - Sqlx prepare result
   - Smoke trio result (manual or scripted): 409 + 402 + tier-aware footer
   - Any blockers encountered
   - Any deviations from the plan (none expected; if any, justify)
   - Confirmation that `docs/planning/handoffs/p3-stage1-to-stage2.md` is written and Contracts C17/C18/C19 are present verbatim
   - Confirmation that test-mod justification artifact lists all 4 fixture sites and `git diff --name-only` matches
2. EXIT this session (close terminal)
3. Return to orchestrator terminal — orchestrator will run `/0-uldf-finalize --skip-push` and auto-spawn Stage 2 worker

**CRITICAL RULES** (re-stated):
- Do NOT run `/0-uldf-ltads-stop` (orchestrator does this)
- Do NOT run `git commit` (orchestrator does this via `/0-uldf-finalize --skip-push`)
- Do NOT modify LTADS tracking files (`ltads/execution/spec-progress.md`, `ltads/sessions/current-session.md`, etc. — orchestrator does this)
- Do NOT push to remote under any circumstances (PF-REGISTER-01 not yet cleared)
- Implement, run all gates, write completion report, exit
