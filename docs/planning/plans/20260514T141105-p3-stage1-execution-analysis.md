# P3 Stage 1 — Execution Analysis (LTADS Start)

**Source**: /0-uldf-ltads-start (orchestrated analysis subagent)
**Generated**: 2026-05-14T14:11:05Z
**Session**: S002 (continuation arc; predecessor S001 CONCLUDED at P2 close commit `9f1a28b`)
**Mode**: CONTINUATION (LTADS active; new session in autopilot:continuous arc)
**Plan consumed**: `docs/planning/plans/20260514T134816-feedbackmonk-p3-commercial-gate.md` (THE plan; § Stage 1 detail)
**Handoff brief consumed**: `.claude/handoff/handoff-20260514-140326-p3-stage1.md`

---

## Decomposition Rationale

The P3 plan pre-committed STAGED execution (2 stages, sequential). This analysis confirms STAGED is correct for Stage 1 entry and selects single-worker Orchestrated topology over PODS within Stage 1.

**Why Orchestrated single worker (not PODS within Stage 1)**:

1. **Tight internal coupling**. Tier model + repo extensions + AppState wiring + cap-check wiring + oracle Probe A all touch the same backend Rust surface. Splitting across 2+ PODS workers fragments attention without parallelism gain — every worker would need to read every other worker's interface to wire things up correctly.
2. **Same-directory overlap**. Most files live in `crates/feedbackmonk-api/src/handlers/` (cap wiring touches `projects.rs`, `feedback.rs`, new `admin_tier.rs`, with `mod.rs` as a barrel) and `crates/feedbackmonk-repository/src/` (modifies `tenants.rs`, adds `tier_quota.rs`). Per the parallelization scope analysis, same-directory work with shared barrel files = OVERLAPPING — PODS required if parallelizing, but the work shape doesn't justify the PODS ceremony for ~14 files of mostly-mechanical extensions.
3. **Capacity comfortably fits**. Per plan § Context Budget Assessment, Stage 1 worker estimate is ~70% of Tier 1 — single worker is correct sizing.
4. **Sequential dependency chain within Stage 1**. Phase 0 (oracle scaffolding) gates Phase 1 (tier model). Phase 1 gates Phase 2 (repo). Phase 2 gates Phase 3 (AppState/error). Phase 3 gates Phase 4 (wiring). Phases 5/6 are leaves. Phase 7 finalizes. There is no phase-pair that can run truly independently.

**Why Stage 2 is NOT in this brief**:

1. Stage 1 exit gate REQUIRES `docs/planning/handoffs/p3-stage1-to-stage2.md` with Contracts C17/C18/C19 frozen verbatim BEFORE Stage 2 can begin.
2. Stage 2 worker stack is React/TypeScript (admin-ui), structurally disjoint from Stage 1's Rust stack — fresh worker context appropriate.
3. Plan § Strategy Rationale explicitly defers Stage 2 topology re-decision to `/0-uldf-proceed` at Stage 1 exit. Per autopilot:continuous chain rule, the orchestrator auto-spawns Stage 2 at Stage 1 finalize completion.

---

## Parallelization Analysis (Phase 3.5)

### Dependency matrix (Stage 1 phases)

| Phase | Depends On | Parallel-safe with? |
|---|---|---|
| Phase 0 (oracle Task Zero) | (foundation) | Nothing — must precede all tier-cap wiring |
| Phase 1 (tier model) | Phase 0 (oracle exists for develop/test/fix loop) | No phase — Phase 2 needs Tier enum |
| Phase 2 (repo extensions) | Phase 1 (Tier enum imported) | No phase — Phase 3 needs TierQuotaRepo |
| Phase 3 (AppState + error) | Phase 2 (TierQuotaRepo trait) | No phase — Phase 4 needs both AppState and ApiError variant |
| Phase 4 (cap wiring) | Phase 3 (AppState + ApiError) | Phase 5 (admin_tier endpoint), Phase 6 (docs) |
| Phase 5 (admin tier endpoint) | Phase 3 (AppState) | Phase 4, Phase 6 |
| Phase 6 (operations + deferred docs) | Independent (pure docs) | Anything after Phase 1 |
| Phase 7 (verification + freeze) | All prior | (terminal) |

**Parallel opportunity**: Detected in late phases (4/5/6 can interleave), but a single worker doing them sequentially is faster than coordinating PODS workers across the same `handlers/` directory.

### Scope analysis

**Directory check**: SAME (most work in `crates/feedbackmonk-api/src/handlers/` + `crates/feedbackmonk-repository/src/`).

**Shared files**:
- `crates/feedbackmonk-api/src/handlers/mod.rs` (barrel — Phase 4 + Phase 5 both touch)
- `crates/feedbackmonk-api/src/state.rs` (AppState — Phase 3 + Phase 4 both touch)
- `crates/feedbackmonk-api/src/main.rs` (build_state + route merge — Phase 3 + Phase 5 both touch)
- `crates/feedbackmonk-api/src/error.rs` (Phase 3 only)
- `crates/feedbackmonk-api/src/lib.rs` (Phase 5 only)
- `crates/feedbackmonk-core/src/lib.rs` (Phase 1 only)
- `crates/feedbackmonk-repository/src/lib.rs` (Phase 2 only)
- `.claude/oracles/multi-tenant-isolation-check/allowlist.toml` (Phase 2 — append `SqlxTierQuotaRepo::new`)

**Scope status**: OVERLAPPING (same directory + multiple shared barrels). Per safety rules, this means PODS would be REQUIRED if parallelizing — but the work shape (sequential dependency chain + small file count) makes single-worker Orchestrated the right call.

**Recommendation**: Single Orchestrated worker, sequential phase execution. PODS not warranted at this scope.

---

## Capacity Estimation

### Context files needed by worker

1. `docs/planning/plans/20260514T134816-feedbackmonk-p3-commercial-gate.md` — THE plan (~6k)
2. `docs/planning/plans/20260513T185711-feedbackmonk-v1-build-arc.md` § P3 (~3k partial)
3. `docs/specs/SPECIFICATION.md` (FR-FBR-14 + § Oracles section) (~4k partial)
4. `docs/specs/DECISIONS.md` (DEC-FBR-03 pricing matrix) (~2k partial)
5. `docs/specs/ARCHITECTURE.md` (component table for new entries) (~2k partial)
6. `.claude/oracles/widget-bundle-size/` (canonical pattern: README, oracle.py, manifest.json, oracle.sh, oracle.ps1) (~8k)
7. `.claude/oracles/multi-tenant-isolation-check/` (allowlist pattern + Probe A reference) (~5k)
8. `crates/feedbackmonk-core/src/lib.rs`, `models.rs` (~2k)
9. `crates/feedbackmonk-repository/src/tenants.rs`, `scope.rs`, `lib.rs` (~6k)
10. `crates/feedbackmonk-api/src/state.rs`, `error.rs`, `main.rs`, `lib.rs` (~5k)
11. `crates/feedbackmonk-api/src/handlers/{projects.rs, feedback.rs, mod.rs, admin_feedback.rs}` (~10k)
12. `crates/feedbackmonk-api/tests/handlers.rs`, `tests/router_submission_integration.rs` (fixture sites) (~6k)
13. `migrations/00001_p0_schema.sql` (verify `tenants.tier` column shape) (~1k partial)
14. CLAUDE.md (project context) (~5k)

**Context cost**: ~65k tokens.

### Task classification (14 tasks)

- **Simple** (3-5k each): T1b (lib.rs re-export), T7b (oracle re-runs), T9 (TIER_OVERRIDE.md), T10 (polar deferred stub), T11 (one-paragraph DECISION) → 5 × ~4k = 20k
- **Medium** (10-20k each): T1 (tier.rs ~80 LOC + tests), T2 (tenants.rs extensions ~60 LOC + 4 tests), T5 (ApiError variant + HTTP mapping), T6 (project-create cap wiring + error propagation), T7 (feedback cap wiring + error propagation), T8 (admin_tier.rs ~80 LOC + 3 tests), T12 (handoff doc with C17/C18/C19 verbatim + TS starter kit) → 7 × ~14k = 98k
- **Complex** (25-40k each): T0 (full oracle build with three probes — Python + sh + ps1 + manifests + README + allowlist), T3 (tier_quota.rs ~120 LOC trait + impl + 6 sqlx::test cases), T4 (AppState + 3 fixture sites + TenantRepo mock stubs + test-mod justification artifact with YAML frontmatter) → 3 × ~30k = 90k

**Implementation cost**: ~208k tokens.

**Overhead**: ~15k.

**Total estimated need**: 65 + 208 + 15 = **288k tokens**.

### Method capacity check

- Tier 1 (900k): 288 / 900 = **32%** → GOOD FIT (well under 85% gate)
- Tier 2 (1100k): 288 / 1100 = **26%** → GOOD FIT
- PODS 2-worker (1400k): 288 / 1400 = **21%** per worker if split → GOOD FIT but unnecessary

**Recommended worker tier**: **Tier 1** — comfortable fit with ~50%+ headroom for develop/test/fix iteration cycles, oracle re-runs, and reasoning.

---

## Strategy Reasoning

**Selected**: Path 4 — Orchestrated single Tier 1 worker.

**Rationale**:

1. **Autopilot:continuous arc-chain rule** — at autopilot, /0-uldf-ltads-start auto-executes the recommended path without prompting. Recommended path is the smallest-ceremony option that fits the work shape.
2. **Capacity is not the constraint** (32% of Tier 1) — PODS would be over-engineering for this scope.
3. **OVERLAPPING scope** in `handlers/` + shared `state.rs`/`mod.rs` makes Task tool parallel UNSAFE. PODS would solve coordination but adds ceremony unjustified by the per-worker time savings (~14 files, mostly mechanical wiring).
4. **Sequential phase dependency chain** within Stage 1 means parallelism opportunities are limited to late phases (4/5/6) where single-worker sequential is faster than coordination overhead.
5. **Stage 2 fan-out boundary is at Stage 1 EXIT, not within Stage 1** — the contract-freeze handoff doc (P3-S1-T12) is the natural decoupling point.

**Pre-committed via plan**: STAGED. This analysis confirms STAGED + single Orchestrated worker for Stage 1.

---

## Files this analysis updated

| File | Status | Notes |
|---|---|---|
| `ltads/sessions/current-session.md` | OVERWRITTEN | New session S002 record (S001 CONCLUDED) |
| `ltads/execution/spec-progress.md` | OVERWRITTEN | Updated through P2 close (FR-FBR-04/11/12/13 → DONE); FR-FBR-14 → IN_PROGRESS; FR-FBR-15 → DEFERRED; P3 Stage 1+2 task tables added |
| `ltads/execution/task-queue.md` | OVERWRITTEN | New P3 Stage 1 task queue (was P0 Stage 1 stale) |
| `ltads/execution/S002/` | CREATED | Session execution dir |
| `docs/planning/plans/20260514T141105-p3-stage1-execution-analysis.md` | CREATED | This file (analysis-reasoning persistence per FOUNDATIONS Principle 2.6) |
| `ltads/execution/S002/analysis-reasoning.md` | TO BE CREATED | Symlink/copy of this file for session-specific reference |

**No commit-log entry written** — commit-log gets the next entry at `/0-uldf-finalize` time, not at session start.
