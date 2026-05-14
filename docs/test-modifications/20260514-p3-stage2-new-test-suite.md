# Test Modification: P3 Stage 2 — Admin UI Tier Settings (new test suite)

**Date**: 2026-05-14
**Phase**: P3 Stage 2 (admin UI tier settings + cap-aware error rendering)
**Commit**: P3 close (this commit)
**Author**: agent (autopilot:continuous, S002 orchestrator → Stage 2 worker)

## Why this artifact exists

`/0-uldf-finalize` Phase 0.5 (Anti-Reward-Hacking Gate) fires on test+code co-edit because the staged set contains both new test files (`admin-ui/src/pages/settings/__tests__/TierSettings.test.tsx` + `admin-ui/e2e/tier-settings-a11y.spec.ts`) and non-test files (the new `pages/settings/` module + `ApiClient.ts` extensions + `types.gen.ts` generated additions + CSS tokens). Justification required so a reviewer can confirm no assertion strength was silently reduced to make the new code pass.

## Was anything silently reduced?

**No.** This change is entirely **net-new** on the test surface.

- **No existing tests modified.** `git diff --stat` shows zero changes to any pre-existing `*.test.tsx` / `*.spec.ts` file in the staged set.
- **No assertions weakened.** Both files are 100% new; nothing existed to weaken.
- **No `.skip` / `.todo` / `xit` introduced.** Vitest reports 13 NEW tests under `TierSettings.test.tsx`, all PASSING. Playwright reports 4 NEW tier-view tests under `tier-settings-a11y.spec.ts`, all PASSING with 0 axe-core violations.
- **No `data-testid` escape hatches.** All assertions go through `@testing-library/react` accessible queries (`findByRole`, `findByText`, `getAllByRole("progressbar")`, `getByRole("region", { name: /upgrade/i })`) — same accessibility-first strategy P1/P2 used.

## What was added

### `admin-ui/src/pages/settings/__tests__/TierSettings.test.tsx` (13 tests)

Vitest unit suite covering the new `pages/settings/` module:

| Test group | What it asserts | Contract reference |
|---|---|---|
| Free-tier render | Tier badge, projects 1/1 (warn state at 100%), feedback 12/50 (ok), footer capability ✗, upgrade prompt rendered with "Starter" copy | Contract C17 (`TierStatus`) + Contract C19 (`tier_quotas()` Free row) |
| Starter render | Tier badge, projects 2/3, feedback 480/500 (danger state at 96%), upgrade prompt with "Pro" copy | C17 + C19 (Starter row) |
| Pro render | Projects "12 / unlimited" (no progressbar), feedback 4500/10000, custom_domain ✓ + EU residency ✓ + (configurable; implementation pending) footnote, footer capability ✓ (no footer), upgrade prompt with "Self-host" copy | C17 + C19 (Pro row) |
| Self-host render | Both meters unlimited (no progressbars), all capabilities ✓, **no upgrade prompt** | C17 + C19 (Self-host row) |
| Loading + error | Pending state shows "Loading…", error state shows `role="alert"` + Retry button | n/a — UX surface |
| `extractTierCapExceeded` (interceptor seam) | Returns null on non-axios; returns null on axios with non-C18 body; returns body when 402/409 + tagged; returns body via belt-and-braces parse when un-tagged | Contract C18 (`TierCapExceededBody`) |

### `admin-ui/e2e/tier-settings-a11y.spec.ts` (4 tests)

Playwright + axe-core slow-lane suite — one a11y sweep per tier (Free / Starter / Pro / Self-host) with FAKE_API mode intercepting `/api/v1/admin/tier`. 0 violations across all four. Mirrors existing `public-roadmap-a11y.spec.ts` pattern.

## Stage-2-side drift surface (load-bearing)

The `tierStatus()` helper in `TierSettings.test.tsx` **inlines the four-tier Contract C19 quotas verbatim**. This is intentional and load-bearing:

> If Stage 1's `crates/feedbackmonk-core/src/tier.rs::tier_quotas()` is rebased to a new shape, this fixture FAILS — pairing the backend `tier-enforcement-status` Probe B (config-shape AST check) with a frontend rendering check that exercises the same canonical values.

Both sides MUST update together. The capability-list semantics (especially the **inverted** "no free-tier footer" — enabled when `quotas.footer_text === null`) are also asserted here so a footer-text-shape change doesn't silently flip the Free row.

## Categorization (Phase 5 matrix)

| File | Matrix category | Lane | Risk tier |
|---|---|---|---|
| `TierSettings.test.tsx` | `MATRIX-CAT-DIFFERENTIAL` (paired with backend Probe B) | fast-inner (vitest) | Tier 2 |
| `tier-settings-a11y.spec.ts` | `MATRIX-CAT-GOLDEN-OUTPUT` (axe-core 0-violation snapshot) | slow-outer (playwright) | Tier 2 |

Both Tier 2 → MEDIUM scope total → finalizer-inline maintenance is fine for this commit.

## Verification at finalize-time

- Vitest: 38/38 PASSING (was 25/25 pre-Stage-2; +13 new in this file).
- Playwright tier-settings-a11y: 4/4 PASSING (0 axe violations; verified during develop/test/fix loop).
- All 4 Verification Oracles GREEN at finalize: `multi-tenant-isolation-check`, `pii-scrub-audit`, `widget-bundle-size`, `tier-enforcement-status`.
