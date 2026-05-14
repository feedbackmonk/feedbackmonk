# Execution Plan

**Source**: /0-uldf-ldis-plan
**Generated**: 2026-05-14T13:48:16Z
**Task**: feedbackmonk P3 — Commercial Gate (tiers + caps + footer, **Polar billing deferred**)
**Strategy**: STAGED (2 stages, sequential between stages; tasks within Stage 1 may parallelize at execution time if /0-uldf-proceed picks PODS)
**Intake Source**: docs/planning/intakes/20260512T221154-extract-feedback-system-as-standalone-product.md
**Arc Plan**: docs/planning/plans/20260513T185711-feedbackmonk-v1-build-arc.md (§P3)

---

## Strategy Rationale

### Why STAGED, not PARALLEL, not SEQUENTIAL

The arc plan flagged P3 as "SEQUENTIAL or 2-worker PODS depending on how much Polar setup is already known territory." With **FR-FBR-15 (Polar billing) deferred** per user direction ("we just don't need to set up billing yet for consumers"), the P3 scope shrinks roughly in half. What remains is FR-FBR-14 (tier model + caps + free-tier footer) plus the `tier-enforcement-status` Verification Oracle.

**STAGED** wins over **PARALLEL** here because:

1. **One load-bearing interface contract** (`check_tier_quota` predicate) — backend authoritative, admin UI consumes. Freeze it once at end of Stage 1, no mid-stream contract-freeze handshake needed.
2. **Smaller scope than P2** (~15 files vs P2's ~50). PODS coordination ceremony (channels, status updates, file-tracking) costs more than 2 workers' parallel speedup at this size.
3. **Backend tier work has tight internal coupling** — the tier-cap predicate, schema-level enforcement, repo `get_widget_brand` flip, and oracle Probe A all touch the same surface. Single focused agent attention is better than splitting Rust work.
4. **Admin UI is genuinely separate** — clean React stack, depends only on Stage 1's API surface, no Rust touches.

**STAGED** wins over **SEQUENTIAL** because:

1. **Quality benefit from staged review** — Stage 1's exit gate (oracle green + workspace tests pass + tier-cap unit tests) is a natural checkpoint where Stage 2 can begin with a known-good API surface.
2. **Stage 2 is genuinely independent work** — admin UI tier settings page is a self-contained surface with its own a11y/vitest gates; no need to keep Stage 1's full Rust context loaded while writing TSX.
3. **Allows topology re-decision at Stage 2 boundary** — if Stage 2 grows (e.g., usage-history charts, multi-project switcher) it can fan out via /0-uldf-proceed without re-planning.

### Collaboration Value Assessment (P3 phase)

| Factor | Score | Why |
|---|---|---|
| **Specialization** | 3 | Rust backend + React frontend benefit from per-stack attention, but each is small enough for one agent |
| **Quality** | 3 | Two stages get fresh review at Stage 1 exit; PODS-level cross-checking adds little here |
| **Discovery** | 2 | Tier-cap pattern is mechanical (schema already has the column); footer flip is one-line; oracle pattern is well-established |
| **Speed** | 3 | Stage 2 cannot start before Stage 1's contract freeze; parallelism is bounded by data flow |
| **Total Value** | **11/20** | Medium |
| **Boundary Clarity** | 5 | Rust crates vs admin-ui — perfectly disjoint surfaces |
| **Coupling** | 4 | One typed API contract is the entire cross-stream surface |
| **Total Friction (higher=less)** | **9/10** | Low |
| **Net Score** | **11 - 4.5 = 6.5** | Range: STAGED/PARALLEL with careful boundaries — STAGED chosen for ceremony economy at this scope |

### Topology re-confirmation at Stage 1 execution time

The execution command at Stage 1 boundary is `/0-uldf-proceed`. If at that point the agent assesses Stage 1's work-shape as 3+ independent Rust subtasks (e.g., tier model + cap enforcement + oracle each owned by a distinct worker), `/0-uldf-proceed` may upgrade to PODS. STAGED is the default; PODS-within-Stage-1 is a permitted escalation if the work-shape supports it.

---

## Context Budget Assessment

**Stage 1 worker** (Rust backend + oracle):
- Assigned scope: ~7 files (tier model, quotas table, repo extension, project_create handler tier-check, submission handler tier-check, oracle build, get_widget_brand tier-flip)
- Sibling summaries needed: P2 widget bundle (already shipped, READMEs available), feedback handler chain (P0 reference)
- Interface contracts: `check_tier_quota` predicate signature (FROZEN at Stage 1 exit), structured `TierCapExceeded` error shape (Stage 1 emits → Stage 2 consumes)
- Reasoning reserve: 25% (tier-cap predicate has nuanced semantics — what counts as a "feedback submission" for volume cap; how grace works post-downgrade)
- **Estimated budget**: ~70% — comfortable

**Stage 2 worker** (admin UI tier settings):
- Assigned scope: ~5 files (tier settings page, usage-vs-cap component, upgrade-prompt component, ApiClient extension, vitest + a11y)
- Sibling summaries needed: P1 admin-ui patterns (FeedbackList, StatusControls), P2 PublicRoadmap layout conventions
- Interface contracts: consumes Stage 1's frozen `check_tier_quota` API + `TierCapExceeded` error shape
- Reasoning reserve: 15% (UI work is well-patterned)
- **Estimated budget**: ~50% — comfortable

Both stages fit within `agent_budget ≤ 85%` per ULADP. No further decomposition needed.

---

## Oracle Pre-Build Plan

| Oracle | Question | Consumer(s) | Timing | Status |
|---|---|---|---|---|
| `tier-enforcement-status` | Confirms (Probe A) every domain write path goes through `check_tier_quota`; (Probe B) `tier_quotas()` returns expected shape per tier; (Probe C) integration smoke — Free creates 2nd project → 409, Free submits 51st in window → 402, footer reflects tier | Stage 1 worker (build); ongoing CI gate | **Build during Stage 1 Task Zero** (before tier-cap wiring) — same pattern as `widget-bundle-size` at P2 Task Zero | not yet built |

**Rationale**: `tier-enforcement-status` is the dominant code-level invariant for the entire commercial-product surface. Without it, tier drift is silent until a customer notices on their invoice (or for now, until a dogfood tenant notices a free-tier cap that shouldn't apply to them). Pre-building at Stage 1 Task Zero means every subsequent Stage 1 commit closes its develop/test/fix loop against the oracle.

The oracle has three probes:

- **Probe A (static, code-level)**: AST-scan every domain write handler under `crates/feedbackmonk-api/src/handlers/`. Each handler that mutates per-tenant state MUST invoke `check_tier_quota` with the appropriate `ResourceKind` before the mutation. Allowlist for handlers that legitimately don't (e.g., signup creates the first tenant — no tier to check yet; admin-session ops don't consume quota). Same shape as `multi-tenant-isolation-check` Probe A.
- **Probe B (static, config-level)**: Read `tier_quotas()` constant; assert each tier (`Free | Starter | Pro | SelfHost`) has the canonical shape (projects_per_org, monthly_feedback_volume, custom_branding bool, custom_domain bool, eu_residency bool, footer_text Option<&'static str>). Defends against accidental edits like setting Free to unlimited.
- **Probe C (integration, end-to-end)**: Three smoke tests. Free-tier tenant creates 2nd project → 409 with structured `TierCapExceeded` body. Free-tier tenant submits 51st feedback within rolling 30-day window → 402 + same error shape. `GET /widget-config` for Free tenant returns `footer_text: Some("powered by feedbackmonk")`; for Pro tenant returns `footer_text: None`.

**Deferrals** (evaluated, not scheduled):

- `feedbackmonk-tier-quotas` project-state oracle (current per-org usage dashboard) — listed in spec § Oracles as optional. Defer to v1.1; not load-bearing for P3 exit gate. Admin UI's usage-vs-cap component (Stage 2) covers the live-display need without a separate oracle.

---

## Component Breakdown

### Stage 1: Backend tier model + enforcement + oracle

| Component | New / Modified | LOC est. | Notes |
|---|---|---|---|
| `crates/feedbackmonk-core/src/tier.rs` | **NEW** | ~80 | `Tier` enum (Free/Starter/Pro/SelfHost), serde + sqlx codec (TEXT round-trip mirroring DEC-FBR-03 lowercase wire values), `tier_quotas()` const fn |
| `crates/feedbackmonk-core/src/lib.rs` | modified | +1 | `pub mod tier; pub use tier::{Tier, TierQuotas, ResourceKind, tier_quotas};` |
| `crates/feedbackmonk-repository/src/tenants.rs` | modified | +60 | `get_tier(scope) -> Tier`, `get_widget_brand` now tier-aware (footer_text per tier), `count_projects(scope)`, `count_feedback_in_window(scope, days)` |
| `crates/feedbackmonk-repository/src/tier_quota.rs` | **NEW** | ~120 | `check_tier_quota(scope, resource) -> Result<QuotaStatus>`; reads tier + counts + matches against `tier_quotas()` |
| `crates/feedbackmonk-api/src/error.rs` | modified | +25 | Add `ApiError::TierCapExceeded { tier, resource, current, limit, upgrade_hint }` → HTTP 402 (volume) or 409 (projects) with structured body |
| `crates/feedbackmonk-api/src/handlers/projects.rs` (or wherever project-create lives) | modified | +15 | Wire `check_tier_quota(scope, ResourceKind::Project)` at top of create handler |
| `crates/feedbackmonk-api/src/handlers/feedback.rs` (submission handler) | modified | +15 | Wire `check_tier_quota(scope, ResourceKind::FeedbackInRollingMonth)` at top |
| `.claude/oracles/tier-enforcement-status/` | **NEW** | ~280 | Full oracle (Python canonical + sh/ps1 shims + manifest.json/toml + README + allowlist.toml). Three-probe: AST + config-shape + integration |
| `.claude/oracles/INDEX.md` | modified | +2 | Register under `### tiers` subsection |
| `CLAUDE.md` | modified | +1 | Flip `tier-enforcement-status` row to ✅ LIVE |
| `docs/operations/TIER_OVERRIDE.md` | **NEW** | ~40 | Dogfood SQL helper: `UPDATE tenants SET tier='self_host' WHERE …`; documents per DEC-FBR-03 tier matrix |
| `docs/deferred/polar-integration.md` | **NEW** | ~80 | Stub-and-defer note: webhook receiver shape (`POST /api/billing/polar/webhook`), Polar customer/subscription ID columns (NOT migrated yet), `subscription.created/updated/cancelled` → `tenants.tier` mapping, reference to `gitcellar-cloud/src/billing/polar.rs` for port pattern |

**Stage 1 exit gate**:
- `cargo test --workspace --no-fail-fast` GREEN
- `cargo clippy --workspace --all-targets -- -D warnings` GREEN
- `tier-enforcement-status` oracle GREEN (all three probes pass)
- All four canonical Verification Oracles GREEN (`multi-tenant-isolation-check`, `pii-scrub-audit`, `widget-bundle-size`, `tier-enforcement-status`)
- `cargo sqlx prepare --workspace -- --all-targets` clean (Tier enum sqlx codec captured)
- Smoke: free-tier tenant tries 2nd project → 409; free-tier tenant tries 51st submission → 402; widget-config for free tenant returns footer, paid tenant returns None
- `check_tier_quota` signature frozen — Stage 2 contract handshake document at `docs/planning/handoffs/p3-stage1-to-stage2.md`

### Stage 2: Admin UI tier settings + cap-aware error rendering

| Component | New / Modified | LOC est. | Notes |
|---|---|---|---|
| `admin-ui/src/pages/settings/TierSettings.tsx` | **NEW** | ~180 | Current tier card (Free/Starter/Pro/Self-host) + usage-vs-cap row (1/1 projects, N/50 monthly feedback), "Upgrade" CTA (stub text, no Polar yet per DEC-FBR-DEFER-01) |
| `admin-ui/src/pages/settings/UsageMeter.tsx` | **NEW** | ~80 | Visual bar component for "current vs cap" with color states (green <70%, amber 70-95%, red >95%); aria-valuenow + aria-valuetext for screen readers |
| `admin-ui/src/pages/settings/UpgradePrompt.tsx` | **NEW** | ~60 | Reusable upgrade-CTA component; rendered both in TierSettings AND as a toast when a write fails with `TierCapExceeded` |
| `admin-ui/src/shared/ApiClient.ts` | modified | +30 | `fetchTierStatus()` → `{tier, quotas: {projects, feedback_volume_monthly}, usage: {…}}`; refine error handling to surface `TierCapExceeded` shape from Stage 1 |
| `admin-ui/src/shared/types.gen.ts` | modified | +30 | Type mirror for `Tier`, `TierQuotas`, `TierCapExceededBody` |
| `admin-ui/src/App.tsx` | modified | +5 | Route `/admin/settings/tier` → `<TierSettings/>` |
| `admin-ui/src/pages/settings/__tests__/TierSettings.test.tsx` | **NEW** | ~120 | Vitest: renders all 4 tiers correctly; shows usage; upgrade button hidden on Self-host; aria-valuenow correct |
| `admin-ui/e2e/tier-settings-a11y.spec.ts` | **NEW** | ~80 | Playwright + axe: 0 WCAG 2.1 AA violations on idle + after switching tier-view (fake API serves each tier) |
| `crates/feedbackmonk-api/src/handlers/admin_tier.rs` | **NEW** | ~80 | `GET /api/v1/admin/tier` (AdminSession-gated) returns current tier + quotas + live usage |
| `crates/feedbackmonk-api/src/handlers/mod.rs` | modified | +1 | `pub mod admin_tier;` |
| `crates/feedbackmonk-api/src/lib.rs` | modified | +1 | Re-export `admin_tier_router` |
| `crates/feedbackmonk-api/src/main.rs` | modified | +1 | `.merge(admin_tier_router(state.clone()))` |

**Stage 2 exit gate**:
- All Stage 1 gates still GREEN
- `admin-ui` `tsc --noEmit` strict GREEN
- `admin-ui` `npx vite build` GREEN
- `admin-ui` `npx vitest run` GREEN (+5 new tests on top of 25 from P2 → 30 total)
- `admin-ui` `npx playwright test tier-settings-a11y` GREEN (0 axe violations)
- Manual smoke: log in as dogfood tenant (set to `self_host` via TIER_OVERRIDE.md SQL), visit `/admin/settings/tier`, verify usage display

---

## Testability Gate Findings

Per Probandurgy Principle 2.13. Each FR or major component scored on the 5-question gate.

### FR-FBR-14 — Tier enforcement (caps + footer)

| # | Question | Score | Note |
|---|----------|-------|------|
| Q1 | Iteration cost — verify+fix cycle? | **2** | Tier predicate is unit-testable in isolation; oracle Probe C smoke is the only integration cost (5-10s per run via sqlx::test) |
| Q2 | Fidelity risk — verifier-miss fraction? | **3** | Probe A AST-scan can miss handlers that should consult the predicate (especially new handlers added post-P3). MEDIUM concern: this is the same fidelity profile as multi-tenant-isolation-check, which we trust |
| Q3 | Critical path? | **4** | FR-FBR-14 is the commercial-product gate — blocks P4 launch (DEC-FBR-10 Stage 2 trigger), nothing ships publicly without it |
| Q4 | Scaffolding leverage — fixture/oracle/probe halves cost? | **5** | The `tier-enforcement-status` oracle is the canonical scaffolding — Probe A catches missing predicate calls at commit time, Probe C asserts end-to-end cap firing. Without it, every write-path bug surfaces only at integration time. Massive leverage |
| Q5 | Drift detection if scaffolding used? | **2** | Oracle Probe B asserts `tier_quotas()` shape — catches drift in the tier table itself. Oracle Probe A's allowlist (handlers exempted from tier check) is the drift surface — every new exemption must be justified in the allowlist comment. Same drift discipline as multi-tenant-isolation-check |
| **Composite** | | **16/25** | **FLAGGED for scaffolding pairing** — composite >12, and the scaffolding (oracle) is already planned. Q4=5 with Q5=2 is the "high-leverage scaffolding + sound drift detection" sweet spot. Proceed with the planned oracle |

### Tier-cap predicate signature design

| # | Question | Score | Note |
|---|----------|-------|------|
| Q1 | Iteration cost? | **2** | Single function, pure aside from DB reads; unit-testable with mock repos |
| Q2 | Fidelity risk? | **2** | Predicate semantics are precise: input is `(scope, resource_kind)`, output is `QuotaStatus { allowed: bool, current: i32, limit: i32, tier: Tier }`. Hard to get wrong silently |
| Q3 | Critical path? | **4** | Every write path consumes it; getting the signature wrong forces wide refactor |
| Q4 | Scaffolding leverage? | **3** | Unit tests + Probe C integration suffice; no additional scaffolding warranted |
| Q5 | Drift detection? | **2** | Probe A asserts every handler calls it; signature drift surfaces at compile time |
| **Composite** | | **13/25** | **FLAGGED (barely)** — composite >12 driven by Q3 critical-path weight. Mitigation: freeze the signature at Stage 1 exit (handoff doc) and treat changes as breaking |

### Free-tier footer flip in widget brand

| # | Question | Score | Note |
|---|----------|-------|------|
| Q1 | Iteration cost? | **1** | One-line change in `TenantRepo::get_widget_brand` (read tier, branch on it) |
| Q2 | Fidelity risk? | **3** | Risk of inverted logic (paid tier showing footer instead of free) is real. Probe C catches this |
| Q3 | Critical path? | **3** | Paid customers seeing a free-tier footer is a brand bug; visible but recoverable |
| Q4 | Scaffolding leverage? | **3** | Probe C integration test covers it; unit test on get_widget_brand suffices |
| Q5 | Drift detection? | **2** | sqlx::test integration + Probe C |
| **Composite** | | **12/25** | **NOT FLAGGED** — at the threshold but no specific risk warranting redesign |

### `tier-enforcement-status` Verification Oracle (itself)

| # | Question | Score | Note |
|---|----------|-------|------|
| Q1 | Iteration cost? | **2** | Probe A is fast (filesystem AST scan); Probe B is instant; Probe C is ~10s |
| Q2 | Fidelity risk? | **3** | Probe A's allowlist is the fidelity surface — under-allowlisted means false positives, over-allowlisted means missed handlers. Pattern matches multi-tenant-isolation-check (well-trusted) |
| Q3 | Critical path? | **4** | Oracle gates every Stage 1 commit |
| Q4 | Scaffolding leverage? | **4** | Oracle IS the scaffolding for FR-FBR-14. Self-reflective question is trivially answered: yes, building it halves iteration cost vs ad-hoc test runs |
| Q5 | Drift detection? | **2** | Oracle's own README + allowlist comments + Phase 11 of finalize revalidates manifest hashes |
| **Composite** | | **15/25** | **FLAGGED for fixture/oracle pairing** — fits the same "high-leverage oracle is the right call" cell as FR-FBR-14 itself. Proceed |

### Items NOT flagged

- **Stage 2 admin UI tier settings page**: composite ~10 (Q3=2 — UI bug is recoverable, not load-bearing). Standard vitest + Playwright a11y coverage suffices.
- **TIER_OVERRIDE.md operations doc**: composite ~6 — pure documentation, no code surface to test.

**Findings summary**: Two items (FR-FBR-14, oracle itself) flagged for scaffolding pairing — the planned `tier-enforcement-status` oracle IS that scaffolding. One item (predicate signature) flagged for freeze discipline at Stage 1 exit. No items require redesign. No items require dropping scaffolding.

---

## Ripple Analysis

**Modified interfaces**:

| Surface | Owner | Consumers (existing) | Change shape |
|---|---|---|---|
| `TenantRepo::get_widget_brand(&TenantScope)` | feedbackmonk-repository | `widget_config` handler (P2); P1 email path uses `get_email_brand` not this | Body change only — signature unchanged. Behaviour change: now reads `tenants.tier` and returns `footer_text: None` on non-Free tiers |
| `AppState` | feedbackmonk-api | All handlers; 3 test fixtures (admin_feedback, tests/handlers, tests/router_submission_integration) | **Append-only** — adding `tier_quotas` repo handle alongside existing fields. Identical shape to P2's roadmap fields addition |
| `ApiError` enum | feedbackmonk-api | Every handler that returns `ApiError`; admin-ui error rendering | **Add variant** `TierCapExceeded { … }` — Rust forces exhaustive match update in error.rs; no other handler needs to construct it |
| `crates/feedbackmonk-api/src/handlers/projects.rs` create endpoint | (existing) | admin-ui project creation flow | Pre-mutation gate added; on cap-exceed returns 409 (was 200/400). admin-ui must surface the new error shape |
| `crates/feedbackmonk-api/src/handlers/feedback.rs` submission endpoint | (existing) | widget submission, admin manual creation | Pre-mutation gate added; on cap-exceed returns 402 (was 200/400) |
| `multi-tenant-isolation-check/allowlist.toml` | (existing) | oracle | **Append-only** — add `SqlxTierQuotaRepo::new` structural-mirror constructor entry. Pre-authorized per allowlist policy (constructor entries mirror existing pattern) |

**Documentation cascade**:

- `crates/feedbackmonk-repository/src/tenants.rs` module README — update with `get_tier`, `count_projects`, `count_feedback_in_window` entries; document the tier-aware widget_brand semantics
- `crates/feedbackmonk-api/src/handlers/` module README — update with tier-cap enforcement chokepoint
- `CLAUDE.md` Oracle table — flip `tier-enforcement-status` row to ✅ LIVE (Stage 1 exit task)
- `docs/specs/SPECIFICATION.md` — flip FR-FBR-14 to DONE; leave FR-FBR-15 as PROPOSED with explicit "DEFERRED per session direction" note
- `docs/specs/DECISIONS.md` — add `DEC-FBR-DEFER-01: Polar billing deferred from P3` (one paragraph documenting the user-direction-driven deferral; references this plan)

**Test impact**:

- 3 AppState test fixtures need `tier_quotas` field addition (same shape as P2's roadmap fields extension). Documented as mechanical fixture extension at `docs/test-modifications/{date}-p3-appstate-tier-quotas-field.md`.
- 1 TenantRepo mock (`tests/email_integration.rs` if it implements TenantRepo, plus any other fake impls in unit tests) needs `get_tier`, `count_projects`, `count_feedback_in_window` stubs (`unimplemented!()` is fine for email-path tests). Documented in same test-mods file.

**No removals**: this phase is purely additive on the existing P0-P2 surface. No deprecations, no rewrites.

**Blast radius**: 🟡 **Medium** — touches every domain write handler (cap-check wiring is the broadest surface), but each touch is mechanical (add `check_tier_quota` line at top, propagate `?` error). Predicate signature freeze at Stage 1 exit prevents cascade.

---

## Interface Contracts

Frozen at Stage 1 exit, consumed by Stage 2. Document at `docs/planning/handoffs/p3-stage1-to-stage2.md` before spawning Stage 2 worker.

### Contract C17: Tier-cap predicate

```rust
// feedbackmonk-repository
pub trait TierQuotaRepo: Send + Sync {
    async fn check_tier_quota(
        &self,
        scope: &TenantScope,
        resource: ResourceKind,
    ) -> Result<QuotaStatus, RepoError>;

    async fn get_tier_status(
        &self,
        scope: &TenantScope,
    ) -> Result<TierStatus, RepoError>;  // current + quotas + live usage; for admin UI
}

// feedbackmonk-core
pub enum ResourceKind {
    Project,                  // counted: SELECT COUNT(*) FROM projects WHERE tenant_id = ?
    FeedbackInRollingMonth,   // counted: SELECT COUNT(*) FROM feedback WHERE project_id IN (...) AND created_at > now() - interval '30 days'
}

pub struct QuotaStatus {
    pub tier: Tier,
    pub resource: ResourceKind,
    pub current: i64,
    pub limit: Option<i64>,   // None = unlimited (Pro / SelfHost projects)
    pub allowed: bool,        // false → handler returns ApiError::TierCapExceeded
}

pub struct TierStatus {
    pub tier: Tier,
    pub usage: TierUsage,
    pub quotas: TierQuotas,   // static — from tier_quotas() for this tier
}

pub struct TierUsage {
    pub projects: i64,
    pub feedback_monthly: i64,
    pub period_start: chrono::DateTime<chrono::Utc>,
}
```

### Contract C18: TierCapExceeded HTTP error body

```typescript
// admin-ui/src/shared/types.gen.ts mirror
interface TierCapExceededBody {
  error: "tier_cap_exceeded";
  tier: "free" | "starter" | "pro" | "self_host";
  resource: "project" | "feedback_in_rolling_month";
  current: number;
  limit: number;
  upgrade_hint: string;  // e.g., "Upgrade to Starter for 3 projects" — Stage 1 emits, Stage 2 renders verbatim
}
```

- HTTP 409 (Conflict) for `resource: "project"` (state conflict — too many projects)
- HTTP 402 (Payment Required) for `resource: "feedback_in_rolling_month"` (idiomatic for paywall)

### Contract C19: Tier static config

```rust
// feedbackmonk-core::tier
pub const fn tier_quotas(tier: Tier) -> TierQuotas {
    match tier {
        Tier::Free     => TierQuotas { projects_per_org: Some(1),  monthly_feedback_volume: Some(50),    custom_branding: false, custom_domain: false, eu_residency: false, footer_text: Some("powered by feedbackmonk") },
        Tier::Starter  => TierQuotas { projects_per_org: Some(3),  monthly_feedback_volume: Some(500),   custom_branding: true,  custom_domain: false, eu_residency: false, footer_text: None },
        Tier::Pro      => TierQuotas { projects_per_org: None,     monthly_feedback_volume: Some(10000), custom_branding: true,  custom_domain: true,  eu_residency: true,  footer_text: None },
        Tier::SelfHost => TierQuotas { projects_per_org: None,     monthly_feedback_volume: None,        custom_branding: true,  custom_domain: true,  eu_residency: true,  footer_text: None },
    }
}
```

**Note on Starter/Pro caps**: DEC-FBR-03 specifies Free at "~50/mo" and gives no concrete numbers for Starter/Pro. Numbers chosen here (500, 10000) are reasonable defaults aligned with the pricing tier spread; Stage 1 worker should validate against any updated guidance and surface as a Deferred Decision if user wants different values.

---

## Detailed Plan

### Stage 1: Backend tier model + enforcement + oracle

**Phase 0 (Task Zero — first 30 min of Stage 1)**:
1. Build `tier-enforcement-status` Verification Oracle from the canonical pattern (mirror `widget-bundle-size` / `multi-tenant-isolation-check` structure).
   - `oracle.py` Python canonical with Probe A (AST scan of handlers/), Probe B (tier_quotas shape), Probe C (integration smoke; sqlx::test fixture-tenant scenarios)
   - `oracle.sh` + `oracle.ps1` shims
   - `manifest.json` + `manifest.toml` mirror; `allowlist.toml` for legitimately-non-tier-checked handlers
   - `README.md` documenting Probes / Three-leg defense / Invocation
   - Register in `.claude/oracles/INDEX.md` under `### tiers` subsection
   - Cold-start vacuous-PASS confirmed (zero handlers wired yet → Probe A passes by allowlist, Probe B passes on bare `tier_quotas()` const, Probe C uses fixture and asserts Free-tier cap behavior — fails until Phase 4 lands)
2. Decision: keep Probe C in cold-start as the "what we're building toward" assertion, OR gate it behind a `--full` flag and let cold-start Probe A+B only. **Recommendation**: gate C behind `--full` so cold-start is vacuous-PASS; oracle CI re-runs `--full` after each wiring step. Reduces Stage 1 inner-loop friction.

**Phase 1 (tier model)**:
3. Create `crates/feedbackmonk-core/src/tier.rs`:
   - `enum Tier { Free, Starter, Pro, SelfHost }` with serde (lowercase strings) + sqlx codec (TEXT → enum via FromStr; `'free' | 'starter' | 'pro' | 'self_host'`)
   - `enum ResourceKind { Project, FeedbackInRollingMonth }`
   - `struct TierQuotas { projects_per_org: Option<i64>, monthly_feedback_volume: Option<i64>, custom_branding: bool, custom_domain: bool, eu_residency: bool, footer_text: Option<&'static str> }`
   - `pub const fn tier_quotas(tier: Tier) -> TierQuotas` per Contract C19
   - Unit tests: serde round-trip, sqlx FromStr round-trip, `tier_quotas()` shape per tier
4. Re-export from `crates/feedbackmonk-core/src/lib.rs`

**Phase 2 (repository extensions)**:
5. Extend `crates/feedbackmonk-repository/src/tenants.rs`:
   - `async fn get_tier(&self, scope: &TenantScope) -> Result<Tier>` — SELECT tier FROM tenants WHERE id = scope.tenant_id()
   - Modify `get_widget_brand` to read tier and select `footer_text` accordingly:
     - Free → `Some("powered by feedbackmonk".to_string())`
     - Starter | Pro | SelfHost → `None`
   - `async fn count_projects(&self, scope: &TenantScope) -> Result<i64>`
   - `async fn count_feedback_in_window(&self, scope: &TenantScope, window_days: i64) -> Result<i64>` — joins projects→feedback for the tenant; configurable window for testability
   - 4 sqlx::test integration tests covering each method's happy path + an empty-tenant case
6. Create `crates/feedbackmonk-repository/src/tier_quota.rs`:
   - `pub trait TierQuotaRepo: Send + Sync { … }` per Contract C17
   - `pub struct SqlxTierQuotaRepo { pool: PgPool }` with constructor
   - Impl `check_tier_quota`: reads tier + count via `count_projects` or `count_feedback_in_window`, matches against `tier_quotas(tier)`, produces `QuotaStatus`
   - Impl `get_tier_status`: returns tier + live usage + static quotas (no DB write — read-only)
   - 6 sqlx::test integration tests covering each (tier × resource) combo
7. Add `SqlxTierQuotaRepo::new` to `multi-tenant-isolation-check/allowlist.toml` (structural-mirror constructor entry; pre-authorized)
8. `cargo sqlx prepare --workspace -- --all-targets` to capture new queries

**Phase 3 (AppState + error variant)**:
9. Extend `AppState` in `crates/feedbackmonk-api/src/state.rs` — add `pub tier_quotas: Arc<dyn TierQuotaRepo>` field
10. Extend `crates/feedbackmonk-api/src/main.rs::build_state` to construct the repo
11. Author `docs/test-modifications/20260514-p3-appstate-tier-quotas.md` (mechanical fixture extension justification — covers 3 AppState fixture sites + any TenantRepo mock that needs `get_tier`/`count_*` stubs)
12. Add `ApiError::TierCapExceeded { tier, resource, current, limit, upgrade_hint }` to `crates/feedbackmonk-api/src/error.rs` per Contract C18; map to 402 (feedback volume) or 409 (project cap)

**Phase 4 (cap enforcement wiring)**:
13. Wire `check_tier_quota(scope, ResourceKind::Project)` into project-create handler — before the INSERT. On `QuotaStatus { allowed: false, … }` → return `ApiError::TierCapExceeded`
14. Wire `check_tier_quota(scope, ResourceKind::FeedbackInRollingMonth)` into feedback submission handler — same shape
15. Re-run `tier-enforcement-status --full` oracle → Probe C should now pass
16. Re-run `multi-tenant-isolation-check` + `pii-scrub-audit` → must remain GREEN (the new code goes through scope-bound repo methods; no raw SQL)

**Phase 5 (admin tier-status endpoint)**:
17. `crates/feedbackmonk-api/src/handlers/admin_tier.rs` — `GET /api/v1/admin/tier` (AdminSession-gated), returns `TierStatus` JSON per Contract C17. 3 handler unit tests.

**Phase 6 (operations + deferred docs)**:
18. `docs/operations/TIER_OVERRIDE.md` — SQL helper for dogfooding (`UPDATE tenants SET tier='self_host' WHERE …`), per-tier capability matrix, link to `tier_quotas()` source of truth
19. `docs/deferred/polar-integration.md` — webhook receiver shape, customer/subscription ID columns (not migrated yet), `subscription.*` → `tenants.tier` mapping, port reference to gitcellar
20. `docs/specs/DECISIONS.md` — add `DEC-FBR-DEFER-01` documenting the Polar deferral (one paragraph; references this plan + user direction)

**Phase 7 (verification + contract freeze)**:
21. Full workspace verification: build / clippy `--all-targets -- -D warnings` / test (target: ≥285 tests; +14 from P2's 271 — tier model unit + repo + handler + 3 oracle integration cases)
22. All four oracles GREEN (`multi-tenant-isolation-check`, `pii-scrub-audit`, `widget-bundle-size`, `tier-enforcement-status --full`)
23. Author `docs/planning/handoffs/p3-stage1-to-stage2.md` — freeze Contracts C17/C18/C19 verbatim + TypeScript starter kit for Stage 2's `types.gen.ts`
24. `/0-uldf-finalize --skip-push` commits Stage 1
25. `/0-uldf-proceed` to Stage 2

### Stage 2: Admin UI tier settings + cap-aware error rendering

**Phase 0** (Stage 2 worker setup):
1. Read `docs/planning/handoffs/p3-stage1-to-stage2.md` for frozen contracts
2. Read existing admin-ui patterns: `FeedbackList.tsx`, `StatusControls.tsx`, `PublicRoadmap.tsx` (for component conventions)

**Phase 1 (types + API client)**:
3. Reconcile `admin-ui/src/shared/types.gen.ts` against handoff's TypeScript starter kit
4. Add `fetchTierStatus()` to `ApiClient.ts`
5. Refine error handling: detect `TierCapExceededBody` shape from any 402/409, route to `UpgradePrompt` toast

**Phase 2 (UI components)**:
6. `UsageMeter.tsx` — visual bar with aria-valuenow + aria-valuetext + color states (green/amber/red)
7. `UpgradePrompt.tsx` — reusable upgrade-CTA; stub button text since Polar deferred ("Contact support to upgrade" — no checkout flow)
8. `TierSettings.tsx` — current tier card + UsageMeter rows for projects + monthly feedback + capability list (custom branding ✓/✗, custom domain ✓/✗, etc.) + UpgradePrompt at bottom
9. Wire route in `App.tsx`: `/admin/settings/tier` → `<TierSettings/>`
10. Optional: add a nav-bar link to tier settings (out of scope if existing nav lives elsewhere)

**Phase 3 (tests)**:
11. `__tests__/TierSettings.test.tsx` — vitest: renders all 4 tiers correctly; meter colors correct; upgrade button hidden on Self-host; correct aria-valuenow values
12. `e2e/tier-settings-a11y.spec.ts` — Playwright + axe (fake API intercepts `/api/v1/admin/tier`): 0 WCAG 2.1 AA violations on idle + after switching to each of 4 tier-views

**Phase 4 (verification)**:
13. `npx tsc --noEmit` (strict) GREEN
14. `npx vite build` GREEN
15. `npx vitest run` GREEN (target: 30/30; +5 new)
16. `npx playwright test tier-settings-a11y` GREEN (0 axe violations)
17. Manual smoke: dogfood tenant set to `self_host` via TIER_OVERRIDE.md, visit `/admin/settings/tier`, verify display
18. `/0-uldf-finalize --skip-push` commits Stage 2 → P3 close

---

## Deferred Decisions

| Decision | Defer Until | Default if Unresolved | Why |
|---|---|---|---|
| **FR-FBR-15 Polar billing integration** (full) | Post-launch / user direction | Document at `docs/deferred/polar-integration.md`; ship "Contact support" stub CTA | Per user: dogfooding their own projects first; no consumer billing yet. Deferral does NOT block P4 (marketing site can claim launch readiness without commerce live) |
| **Starter/Pro concrete cap numbers** (500/mo, 10000/mo) | Stage 1 implementation | Use the defaults from Contract C19 | DEC-FBR-03 specifies only Free (~50/mo); Stage 1 worker may surface as discussion point if user has stronger preference |
| **Project-cap behavior on downgrade** (have 3 projects, downgrade to Free) | Post-P3 / pre-launch | Grandfather existing projects; block creation only until count ≤ cap | Real-world scenario but only matters once Polar is live and downgrades can happen |
| **`feedbackmonk-tier-quotas` project-state oracle** (admin dashboard) | v1.1 | Defer | Admin UI Stage 2's TierSettings component covers the live-display need without a separate oracle |
| **Feedback volume window — rolling 30-day vs calendar month** | Stage 1 Phase 2 | Rolling 30-day (`now() - interval '30 days'`) | Simpler; matches Plausible/standard SaaS pattern. Calendar month requires explicit period_start tracking |
| **Custom domain feature** (Pro tier capability) | P4 / post-launch | Tier-config flag is `true` but no implementation | Per DEC-FBR-08 OUT list and arc plan §306 |
| **EU residency selectable** (Pro tier capability) | Post-v1 | Tier-config flag is `true` but no implementation | Compliance/ops work; not blocking commercial-product look |

---

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| **Probe A false negatives** — new handler added without `check_tier_quota`, allowlist not updated | Medium | High (silent cap bypass) | Probe A scans ALL handlers under `crates/feedbackmonk-api/src/handlers/`; new handlers either consult the predicate OR are added to allowlist with comment justifying. Same defensive pattern as multi-tenant-isolation-check |
| **Tier-cap predicate latency** (each write does 2 DB roundtrips: tier read + count) | Low | Medium | Tier read is single-row by PK (sub-ms). Count is index-supported. If observed latency >5ms p99 in dogfood, cache tier in admin-session cookie (orthogonal addition; not required for P3 close) |
| **Monthly window edge case** — submission at 30-days-and-1-second after period start | Low | Low | Rolling 30-day vs calendar month is consistent — neither has cliffs. Document the rolling-window semantics in `tenants.rs` module README |
| **Stage 2 starts before Stage 1 contract freeze** | Medium (chain pressure) | High (rework if signature changes) | Stage 1 exit gate explicitly requires `docs/planning/handoffs/p3-stage1-to-stage2.md` written before /0-uldf-proceed to Stage 2 |
| **Admin UI error rendering misses the new `TierCapExceededBody` shape** | Medium | Medium (cap fires but user sees raw error) | Stage 2 Phase 1 includes error-handling refinement; vitest test verifies a 402 with `tier_cap_exceeded` error string renders the UpgradePrompt toast |
| **AppState fixture extension misses sites** (P2 had this — 3 AppState fixtures + 1 TenantRepo mock; 4 sites total) | High | Medium | Document the justification artifact upfront enumerating ALL fixture sites in `tests_modified[]` frontmatter; cross-check `git diff --name-only` against the list before Stage 1 exit (lesson from P2 D-FBR-17) |
| **Polar deferral creates "ghost" upgrade button** that misleads users | Low (dogfood) | Low | Stub button reads "Contact support to upgrade" not "Upgrade" — no checkout flow implied. Document at `docs/deferred/polar-integration.md` what the button does NOT do |
| **Schema reality check** — does P0's `tenants.tier TEXT DEFAULT 'free'` constrain any value? | Low | Low | Verify in Stage 1 Phase 1: `CHECK (tier IN ('free', 'starter', 'pro', 'self_host'))` should be added as a migration if not present, OR enforced at the sqlx codec layer. Stage 1 worker decides which is cleaner |

---

## Execution Commands

**Stage 1 entry**:
```bash
/0-uldf-proceed
# → /0-uldf-proceed picks topology per § Strategy Rationale
#   - Default: HERE (this session) at low context, OR HANDOFF→Orchestrated at higher context
#   - If at execution time agent assesses Stage 1 work as decomposable 3+ ways: escalate to PODS
```

**Stage 1 → Stage 2 boundary**:
```bash
# Stage 1 finalizer commits → Stage 1 handoff doc written → /0-uldf-proceed
/0-uldf-proceed
# → Picks topology for Stage 2 (single-stream React work; usually HERE or ORCHESTRATED)
```

**P3 close → P4 entry**:
```bash
# Stage 2 finalizer commits → P3 close
/0-uldf-ldis-plan "feedbackmonk P4 — Go-Public (self-host docker + marketing site)"
```

---

## Notes for Downstream Consumers

- **Stage 1 worker context-loading priority**: this plan → `docs/planning/plans/20260513T185711-feedbackmonk-v1-build-arc.md` §P3 → `docs/specs/SPECIFICATION.md` (FR-FBR-14 + § Oracles) → `docs/specs/DECISIONS.md` DEC-FBR-03 (pricing tiers) → `crates/feedbackmonk-core/src/models.rs` `WidgetBrand` docs → P2's `widget-bundle-size` oracle as the canonical Verification Oracle pattern. Skim, don't deep-read.
- **Stage 2 worker context-loading priority**: `docs/planning/handoffs/p3-stage1-to-stage2.md` (Contracts C17/C18/C19) → existing admin-ui patterns (`FeedbackList.tsx`, `PublicRoadmap.tsx`) for component conventions → this plan §Stage 2 detail.
- **DEC-FBR-DEFER-01** (Polar deferral) will be added to `docs/specs/DECISIONS.md` during Stage 1 Phase 6. Do not deep-implement Polar wiring; the `docs/deferred/polar-integration.md` stub is the entire deliverable for the Polar surface at this phase.
- **GitCellar reference is for the FUTURE Polar work** — DO NOT modify or extract from `E:/Developer/SourceControlled/Apps/GitCellar/gitcellar-cloud/src/billing/` during P3. Read-only reference, port-when-needed pattern per DEC-FBR-07.
- **P3 exit gate flexibility**: arc plan's original exit gate included "Polar webhook → tier flip end-to-end on Polar sandbox" — this gate is RELAXED for our P3 by DEC-FBR-DEFER-01. The relaxed gate is: tier caps fire correctly + footer tier-flip works + oracle GREEN + admin UI displays current tier and usage. DEC-FBR-10 Stage 1 dogfood-alpha trigger remains valid (founder can triage own feedback through it).
