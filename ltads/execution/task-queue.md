# Task Queue — feedbackmonk P3 Commercial Gate (Stage 1)

## Active Stage: P3 Stage 1 — Backend tier model + cap enforcement + `tier-enforcement-status` oracle

**Strategy**: STAGED (Stage 1 = backend + oracle; Stage 2 = admin UI; sequential between stages).
**Topology**: Orchestrated single worker (Tier 1 ~70% budget per plan § Context Budget Assessment).
**Exit gate**: All four Verification Oracles GREEN; `cargo test --workspace` GREEN; `cargo clippy --workspace --all-targets -- -D warnings` GREEN; `cargo sqlx prepare --workspace -- --all-targets` clean; smoke (free-tier 2nd project → 409, free-tier 51st submission → 402, widget-config tier-aware footer); `docs/planning/handoffs/p3-stage1-to-stage2.md` written with Contracts C17/C18/C19 frozen verbatim.

### Phase 0 — Task Zero (precedes all tier-cap wiring)
- [ ] P3-S1-T0: Build `tier-enforcement-status` Verification Oracle in `.claude/oracles/tier-enforcement-status/` (Python canonical + sh/ps1 shims + manifest.json/toml + README + allowlist.toml). Three probes: A (AST scan of `crates/feedbackmonk-api/src/handlers/`), B (`tier_quotas()` shape), C (integration smoke gated behind `--full`). Cold-start vacuous-PASS confirmed (Probe A passes by allowlist before wiring; C gated). Register in `.claude/oracles/INDEX.md` under `### tiers`.

### Phase 1 — Tier model
- [ ] P3-S1-T1: `crates/feedbackmonk-core/src/tier.rs` — `enum Tier { Free, Starter, Pro, SelfHost }` (serde lowercase + sqlx TEXT codec via FromStr); `enum ResourceKind { Project, FeedbackInRollingMonth }`; `struct TierQuotas { ... }`; `pub const fn tier_quotas(tier: Tier) -> TierQuotas` per Contract C19. Unit tests: serde round-trip + sqlx FromStr round-trip + per-tier shape.
- [ ] P3-S1-T1b: Re-export from `crates/feedbackmonk-core/src/lib.rs`.

### Phase 2 — Repository extensions
- [ ] P3-S1-T2: Extend `crates/feedbackmonk-repository/src/tenants.rs`:
  - `async fn get_tier(&self, scope: &TenantScope) -> Result<Tier>`
  - Modify `get_widget_brand` to read tier and pick `footer_text` (Free → `Some("powered by feedbackmonk")`; others → `None`)
  - `async fn count_projects(&self, scope: &TenantScope) -> Result<i64>`
  - `async fn count_feedback_in_window(&self, scope: &TenantScope, window_days: i64) -> Result<i64>`
  - 4 sqlx::test integration tests
- [ ] P3-S1-T3: Create `crates/feedbackmonk-repository/src/tier_quota.rs`:
  - `pub trait TierQuotaRepo: Send + Sync` per Contract C17 (`check_tier_quota`, `get_tier_status`)
  - `pub struct SqlxTierQuotaRepo { pool: PgPool }` + impl
  - 6 sqlx::test integration tests covering each (tier × resource) combination
  - Append `SqlxTierQuotaRepo::new` to `.claude/oracles/multi-tenant-isolation-check/allowlist.toml`
- [ ] P3-S1-T3b: `cargo sqlx prepare --workspace -- --all-targets` — capture new queries.

### Phase 3 — AppState + error variant
- [ ] P3-S1-T4: Extend `AppState` in `crates/feedbackmonk-api/src/state.rs` with `pub tier_quotas: Arc<dyn TierQuotaRepo>`. Extend `build_state` in `main.rs`. Author `docs/test-modifications/20260514-p3-appstate-tier-quotas.md` with YAML frontmatter enumerating ALL fixture sites: `crates/feedbackmonk-api/src/handlers/admin_feedback.rs`, `tests/handlers.rs`, `tests/router_submission_integration.rs`, plus any TenantRepo mock impls.
- [ ] P3-S1-T5: Add `ApiError::TierCapExceeded { tier, resource, current, limit, upgrade_hint }` to `crates/feedbackmonk-api/src/error.rs` per Contract C18. Map to 402 (FeedbackInRollingMonth) or 409 (Project).

### Phase 4 — Cap enforcement wiring
- [ ] P3-S1-T6: Wire `check_tier_quota(scope, ResourceKind::Project)` into project-create handler (`crates/feedbackmonk-api/src/handlers/projects.rs`) BEFORE the INSERT. On `allowed: false` → return `ApiError::TierCapExceeded`.
- [ ] P3-S1-T7: Wire `check_tier_quota(scope, ResourceKind::FeedbackInRollingMonth)` into feedback submission handler (`crates/feedbackmonk-api/src/handlers/feedback.rs`) — same pattern.
- [ ] P3-S1-T7b: Re-run `tier-enforcement-status --full` → Probe C GREEN. Re-run `multi-tenant-isolation-check` + `pii-scrub-audit` → must remain GREEN.

### Phase 5 — Admin tier-status endpoint
- [ ] P3-S1-T8: `crates/feedbackmonk-api/src/handlers/admin_tier.rs` — `GET /api/v1/admin/tier` (AdminSession-gated). Returns `TierStatus` JSON per Contract C17. 3 handler unit tests. Wire into `mod.rs`, `lib.rs`, `main.rs` route merge.

### Phase 6 — Operations + deferred docs
- [ ] P3-S1-T9: `docs/operations/TIER_OVERRIDE.md` — dogfood SQL helper (`UPDATE tenants SET tier='self_host' WHERE …`); per-tier capability matrix; link to `tier_quotas()` source of truth.
- [ ] P3-S1-T10: `docs/deferred/polar-integration.md` — webhook receiver shape (`POST /api/billing/polar/webhook`); Polar customer/subscription ID columns (NOT migrated yet); `subscription.created/updated/cancelled` → `tenants.tier` mapping; reference to `gitcellar-cloud/src/billing/polar.rs` for port pattern.
- [ ] P3-S1-T11: Add `DEC-FBR-DEFER-01: Polar billing deferred from P3` to `docs/specs/DECISIONS.md` (one paragraph; references this plan + user direction).

### Phase 7 — Verification + contract freeze
- [ ] P3-S1-T12: Author `docs/planning/handoffs/p3-stage1-to-stage2.md` — freeze Contracts C17 (TierQuotaRepo trait), C18 (TierCapExceededBody HTTP shape), C19 (`tier_quotas()` static config) verbatim from plan § Interface Contracts. Include TypeScript starter kit for Stage 2's `types.gen.ts`.
- [ ] P3-S1-T13: Worker exits; orchestrator runs `/0-uldf-finalize --skip-push` (workspace verification + commit). NOT a worker task.

## Stage 1 exit witnesses (durable; verified by orchestrator at finalize)

- [ ] `cargo test --workspace --no-fail-fast` GREEN (target: ≥285 tests; +14 from P2's 271)
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` GREEN
- [ ] All four Verification Oracles GREEN: `multi-tenant-isolation-check`, `pii-scrub-audit`, `widget-bundle-size`, `tier-enforcement-status --full`
- [ ] `cargo sqlx prepare --workspace -- --all-targets` clean (Tier sqlx codec captured)
- [ ] Smoke trio passes (free-tier 2nd project → 409; free-tier 51st submission → 402; widget-config tier-aware footer flip)
- [ ] `check_tier_quota` signature frozen — `docs/planning/handoffs/p3-stage1-to-stage2.md` written with C17/C18/C19 verbatim
- [ ] CLAUDE.md Oracle table flips `tier-enforcement-status` row to ✅ LIVE

## Upstream (Stage 2, post-Stage-1 convergence — NOT in this session's brief)

- Admin UI tier settings page (TierSettings.tsx, UsageMeter.tsx, UpgradePrompt.tsx)
- ApiClient extension + error rendering for `TierCapExceededBody`
- Vitest (+5 new) + Playwright + axe a11y (0 violations on idle + 4 tier-views)
- `crates/feedbackmonk-api/src/handlers/admin_tier.rs` already shipped in Stage 1, but Stage 2 may extend
