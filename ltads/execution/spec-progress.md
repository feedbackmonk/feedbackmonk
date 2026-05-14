# Spec Progress — feedbackmonk v1 Arc (P0 → P4)

| FR | Description | Phase | Stage | Status | Witness |
|---|---|---|---|---|---|
| FR-FBR-01 | Multi-tenant data model + tenant-scoped repository | P0 | S1 | **DONE** | Stage 1 commit `dbbe04a`; 19 → 21 repo tests pass; `multi-tenant-isolation-check` oracle GREEN; Contract C1 frozen |
| FR-FBR-02 | Customer signup + onboarding | P0 | S2 | **DONE** | PODS collab-20260513-221600; signup/verify-email/projects/signing-keys; argon2 + HMAC-signed admin session; lettre Mailpit/SMTP mailer |
| FR-FBR-03 | Submission API (JWT + anonymous) | P0 | S2 | **DONE** | `POST /api/v1/projects/{id}/feedback` with auth-mode dispatch; Contract C3 |
| FR-FBR-05 | JWT EdDSA verification | P0 | S2 | **DONE** | `crates/feedbackmonk-jwt/`; all 6 Contract C2 hard invariants enforced; 24-test fixture corpus |
| FR-FBR-06 | Anonymous submission mode | P0 | S2 | **DONE** | `crates/feedbackmonk-anon/` AnonGate + BLAKE3 hash + 22-char cookie; 11 tests |
| FR-FBR-18 | Health + structured logging | P0 | S3 | **DONE** | `/health` + `/health/ready`; tracing JSON + request-id; e2e P0 witness PASS 7/7 |
| FR-FBR-07 | Status workflow + admin transitions | P1 | S2 | **DONE** | PODS collab-20260514-001500 Worker A; admin transition + reply endpoints |
| FR-FBR-08 | Admin UI feedback drawer | P1 | S2 | **DONE** | PODS Worker B; React+Vite admin-ui on :14204; StatusControls + a11y smoke |
| FR-FBR-09 | Email replies | P1 | S2 | **DONE** | `feedback_replies` migration 00004 + plain-text email templates with tenant brand |
| FR-FBR-10 | PII scrub | P1 | S1+S3 | **DONE** | `pii-scrub-audit` Verification Oracle GREEN; `feedbackmonk-tracing` chokepoint + 20-pattern scrubber; e2e P1 witness PASS |
| FR-FBR-04 | Embeddable widget (<30KB) | P2 | — | **DONE** | P2; `widget-bundle-size` oracle GREEN at 16,829B / 30,720 cap (45% headroom) |
| FR-FBR-11 | Public roadmap | P2 | — | **DONE** | P2; `/roadmap` route + ProjectRoadmap component |
| FR-FBR-12 | Voting + Q24 promote (byte-for-byte invariant) | P2 | — | **DONE** | P2; promote.rs render functions + q24_* tests (PERMANENT — DO NOT MODIFY) |
| FR-FBR-13 | Status emails | P1/P2 | — | **DONE** | Subsumed by FR-FBR-09 (P1) |
| FR-FBR-14 | Tier enforcement (caps + footer) | P3 | S1+S2 | **DONE (S1 backend)** | **Stage 1 (this commit)**: backend tier model (`crates/feedbackmonk-core/src/tier.rs` Contract C19), `TierQuotaRepo::check_tier_quota` chokepoint (Contract C17), `ApiError::TierCapExceeded` 402/409 (Contract C18), tier-aware `get_widget_brand` (free-tier footer), `GET /api/v1/admin/tier`, `migrations/00008_tenant_tier_check.sql` defense-in-depth, `tier-enforcement-status` Verification Oracle 3-probe active-PASS. **Stage 2 (UPCOMING)**: admin UI tier display + stub Upgrade button. |
| FR-FBR-15 | Polar billing integration | P3 | — | **DEFERRED** | DEC-FBR-DEFER-01 ratified (added in this commit); stub at `docs/deferred/polar-integration.md`; operator promotion via SQL helper at `docs/operations/TIER_OVERRIDE.md` until Polar lands; not blocking P4 |
| FR-FBR-16 | Marketing site (Astro) | P4 | — | NOT_STARTED | — |
| FR-FBR-17 | Self-host docker compose | P4 | — | NOT_STARTED | — |

## P3 Stage 1 — Tasks (CLOSED in this commit)

| Task ID | Phase | Description | Status | Notes |
|---|---|---|---|---|
| P3-S1-T0 | Phase 0 | Build `tier-enforcement-status` oracle (Probes A+B; C gated behind `--full`) | DONE | Task Zero; Probe C upgraded to active-PASS via smoke trio (D-FBR-20) |
| P3-S1-T1 | Phase 1 | `crates/feedbackmonk-core/src/tier.rs` — Tier enum, ResourceKind, TierQuotas, `tier_quotas()` const fn | DONE | Contract C19; 11 unit tests |
| P3-S1-T2 | Phase 2 | `tenants.rs` extensions: `get_tier`, tier-aware `get_widget_brand`, `count_projects`, `count_feedback_in_window` | DONE | 4 sqlx::test integration tests |
| P3-S1-T3 | Phase 2 | `crates/feedbackmonk-repository/src/tier_quota.rs` — `TierQuotaRepo` trait + `SqlxTierQuotaRepo` impl | DONE | Contract C17; 6 sqlx::test + allowlist append for `SqlxTierQuotaRepo::new` |
| P3-S1-T4 | Phase 3 | Extend `AppState` with `tier_quotas` field; author test-mod justification artifact enumerating ALL fixture sites | DONE | 5 fixture sites enumerated upfront in YAML frontmatter; cross-checked at exit (D-FBR-21) |
| P3-S1-T5 | Phase 3 | `ApiError::TierCapExceeded { tier, resource, current, limit, upgrade_hint }` → 402/409 | DONE | Contract C18; 3 unit tests |
| P3-S1-T6 | Phase 4 | Wire `check_tier_quota(scope, ResourceKind::Project)` into project-create handler | DONE | Probe A coverage GREEN |
| P3-S1-T7 | Phase 4 | Wire `check_tier_quota(scope, ResourceKind::FeedbackInRollingMonth)` into feedback submission handler | DONE | Probe A coverage GREEN |
| P3-S1-T8 | Phase 5 | `crates/feedbackmonk-api/src/handlers/admin_tier.rs` — `GET /api/v1/admin/tier` | DONE | AdminSession-gated; 3 handler unit tests |
| P3-S1-T9 | Phase 6 | `docs/operations/TIER_OVERRIDE.md` (dogfood SQL helper + capability matrix) | DONE | Operator workflow until Polar lands |
| P3-S1-T10 | Phase 6 | `docs/deferred/polar-integration.md` (stub + port reference to gitcellar) | DONE | Webhook envelope + event→tier mapping + schema migration shape captured |
| P3-S1-T11 | Phase 6 | `DEC-FBR-DEFER-01` to `docs/specs/DECISIONS.md` documenting Polar deferral | DONE | Ratified by user direction at plan boundary |
| P3-S1-T12 | Phase 7 | Author `docs/planning/handoffs/p3-stage1-to-stage2.md` freezing Contracts C17/C18/C19 verbatim + TS starter kit | DONE | Stage 1 exit gate hard requirement |
| P3-S1-T13 | Phase 7 | `/0-uldf-finalize --skip-push` (workspace verification + commit) | DONE | This commit |

**Bonus**: `tier_enforcement_smoke` integration test crate (3 tests, 297 LOC) authored eagerly so Probe C asserts the cap-firing trio at the integration layer rather than relying on a manual verification trail. D-FBR-20 documents the pattern.

## P3 Stage 2 — Tasks (UPCOMING)

| Task ID | Phase | Description | Status | Notes |
|---|---|---|---|---|
| P3-S2-T0 | Phase 0 | Read `docs/planning/handoffs/p3-stage1-to-stage2.md` for frozen contracts | TODO | Stage 2 entry — Contracts C17/C18/C19 are frozen; do NOT renegotiate |
| P3-S2-T1 | Phase 1 | Reconcile `admin-ui/src/shared/types.gen.ts` against handoff TS starter kit | TODO | TS types for `Tier`, `ResourceKind`, `TierStatus`, `TierCapExceededBody` |
| P3-S2-T2 | Phase 1 | `fetchTierStatus()` to `ApiClient.ts` + error handling for `TierCapExceededBody` | TODO | Consume `GET /api/v1/admin/tier` |
| P3-S2-T3 | Phase 2 | `UsageMeter.tsx` + `UpgradePrompt.tsx` + `TierSettings.tsx` + route wiring | TODO | Stub Upgrade button per DEC-FBR-DEFER-01 (points at email until Polar lands) |
| P3-S2-T4 | Phase 3 | Cap-aware error rendering in feedback submission + project create surfaces | TODO | Map 402/409 → user-friendly messaging with upgrade_hint |
| P3-S2-T5 | Phase 4 | a11y smoke + e2e Playwright witness for tier display + Upgrade stub click | TODO | Mirror P1 admin-ui Playwright pattern |

## Quality Witnesses (P3 Stage 1 close)

- `cargo build --workspace`: **GREEN** (1m 34s)
- `cargo clippy --workspace --all-targets -- -D warnings`: **GREEN** (cached)
- `cargo test --workspace --no-fail-fast` (DATABASE_URL set): **302/302 pass** (P2 closed at 271; +31 net-new from P3 Stage 1: 11 tier-core + 6 tier_quota + 4 tenant tier extensions + 3 admin_tier + 3 smoke + 4 ApiError)
- `.claude/oracles/multi-tenant-isolation-check/oracle.sh`: **PASS** (Probe A + Probe B clean; allowlist extended for `SqlxTierQuotaRepo::new` + `set_tier_for_test`)
- `.claude/oracles/pii-scrub-audit/oracle.sh`: **PASS** (no tracing changes)
- `.claude/oracles/widget-bundle-size/oracle.sh`: **PASS** (16,829B / 30,720B; widget unchanged)
- `.claude/oracles/tier-enforcement-status/oracle.sh --full`: **PASS** (Probe A handler coverage clean; Probe B Contract-C19 shape clean; Probe C `cargo test --test tier_enforcement_smoke` GREEN)
