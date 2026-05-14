# `pages/settings/` — Admin tier settings page (P3 Stage 2)

**Synopsis**: `/admin/settings/tier` "Plan & usage" page — current-tier card, per-resource UsageMeter, capability matrix, UpgradePrompt CTA. Read-only consumer of `GET /api/v1/admin/tier` (Contract C17). Polar billing deferred per DEC-FBR-DEFER-01 — Upgrade button is the explicit "Contact support to upgrade" mailto stub.

## Purpose & Responsibilities

Renders the user-facing surface for FR-FBR-14 (Tier enforcement). Stage 1 (commit `d2266ae`) shipped the backend tier model + caps + free-tier footer + admin tier-status endpoint; this module closes the user-facing loop with:

- **Current-plan card** — tier badge (Free / Starter / Pro / Self-host)
- **Usage meters** — accessible progressbars for projects-per-org and monthly-feedback-volume, with WCAG 2.1 AA color + text dual-encoding
- **Capability matrix** — custom branding, custom domain, EU residency, free-tier footer (with inverted semantics, see Decision Log)
- **Upgrade prompt** — tier-aware CTA (mailto stub until Polar lands)

## File Index

| File | Role |
|---|---|
| `TierSettings.tsx` | Page component — orchestrates query + sections (card + meters + capabilities + upgrade) |
| `UsageMeter.tsx` | Reusable accessible progressbar with `unlimited` rendering convention |
| `UpgradePrompt.tsx` | Tier-aware upgrade CTA; renders nothing on Self-host |
| `__tests__/TierSettings.test.tsx` | Vitest suite — 13 tests; **inlined Contract C19 fixture is the Stage-2-side drift surface** paired with `tier-enforcement-status` Probe B |

E2E a11y coverage lives in `admin-ui/e2e/tier-settings-a11y.spec.ts` (Playwright + axe-core, 4/4 PASS, 0 violations on Free / Starter / Pro / Self-host).

## Public API & Usage

```tsx
// Mounted at /admin/settings/tier in App.tsx router
<Route path="/admin/settings/tier" element={<TierSettings />} />
```

Components:

```tsx
<UsageMeter label="Projects" current={1} limit={3} />        // bounded → progressbar rendered
<UsageMeter label="Projects" current={12} limit={null} />    // unlimited → "12 / unlimited", no bar

<UpgradePrompt currentTier="free" />                        // → mailto CTA with Starter copy
<UpgradePrompt currentTier="self_host" />                   // → renders nothing
```

Data shape consumed verbatim from Contract C17 (`TierStatusResponse`) — see `shared/types.gen.ts`.

## Constraints & Business Rules

- **Polar billing DEFERRED** (DEC-FBR-DEFER-01) — `UpgradePrompt` button copy is **"Contact support to upgrade"**, NOT "Upgrade". When Polar lands, this component gets a `<Link>` to the Polar checkout URL instead of the mailto fallback. Do NOT add a checkout flow without resurrecting DEC-FBR-15 first.
- **WCAG 2.1 AA on every tier-view** — verified by per-tier axe-core sweep returning 0 violations. CSS color tokens (`--meter-ok / --meter-warn / --meter-danger`) are tuned for AA contrast against `--surface` in both light and dark schemes; do not edit without re-running the a11y sweep.
- **Upgrade button is mailto, not href** — `mailto:support@feedbackmonk.com?subject=Upgrade%20request`. Do not add tracking parameters; DEC-FBR-02 brand promise (no third-party trackers) extends to the admin UI by spirit, not just the widget.

## Relationships & Dependencies

- **`shared/ApiClient.ts`** — `fetchTierStatus()` is the single read path. The 402/409 axios interceptor (`err.tierCapExceeded`) and `extractTierCapExceeded(err)` helper are NOT consumed in this module — they exist for future mutation `onError` callers.
- **`shared/types.gen.ts`** — `TierStatus` / `TierQuotas` / `Tier` / `TIER_LABELS` / `TierCapExceededBody` / `isTierCapExceeded` are all consumed.
- **Backend pair**: `crates/feedbackmonk-api/src/handlers/admin_tier.rs` is the server side of Contract C17. The `tier-enforcement-status` Verification Oracle (`.claude/oracles/tier-enforcement-status/`) Probe B asserts the canonical four-tier shape from the Rust side; **this module's `TierSettings.test.tsx` fixture asserts the same canonical shape from the React side**. Both must update together if Contract C19 rebases.

## Decision Log

### Current

#### WCAG 1.4.1 dual-encoding meter (color + explicit text label)

**Decision**: `UsageMeter` carries state via BOTH a CSS color token (`--meter-ok / --meter-warn / --meter-danger`) AND an explicit text status string ("OK" / "Approaching cap" / "Over cap") composed into `aria-valuetext`.

**Rationale**: WCAG 2.1 SC 1.4.1 (Use of Color) — color cannot be the sole means of conveying meaning. A pure-color bar would fail for monochrome displays, color-blind users, and screen readers reading the bar without seeing it.

**Trade-offs**: Slightly more verbose render. The state label appears redundant to a sighted user looking at a green bar; this is correct — that's the point.

**Implementation**: `UsageMeter.tsx` lines computing `state` and `stateLabel`; the `aria-valuetext` string `"X of Y label used (N%, state-lowercase)"` is the canonical screen-reader payload.

**Constraints**: Do not strip the state span from the visible UI to "tidy up" — that breaks 1.4.1 for users who can't perceive the color.

#### Unlimited rendering — no progressbar when `limit === null`

**Decision**: When `limit === null` (Pro projects, Self-host both axes), render `"X / unlimited"` as a flat text row with NO `role="progressbar"` element.

**Rationale**: A 0% bar would mislead (suggests "low usage out of a real cap"); a 100% bar would falsely imply a cap is being hit. Neither is honest. The progressbar element is only honest when there's a real ratio to display.

**Trade-offs**: The visual treatment is asymmetric (bounded rows have a bar; unlimited rows don't). This is correct — the asymmetry IS the information.

**Implementation**: Early-return branch in `UsageMeter.tsx` (`if (limit === null) { ... }`).

#### Inverted "free-tier footer" capability semantics

**Decision**: In the capability matrix, the row labelled "Free-tier footer (powered by feedbackmonk)" is **enabled** (✓) when `quotas.footer_text === null` (paid tiers), NOT when the footer is present.

**Rationale**: Users read capabilities as "what does my plan give me." On a paid plan, the *user-facing capability* is "no footer" — the absence of the powered-by line. Listing the footer as "enabled on Free" would correctly describe the data but misalign with user mental model ("paid plans don't carry the footer").

**Trade-offs**: The semantics are inverted from the data shape. A reader of the code must trace through `enabled: quotas.footer_text === null` to see this. The line comment in `TierSettings.tsx::capabilities()` explains.

**Implementation**: `capabilities()` function in `TierSettings.tsx`, last entry.

#### `TierSettings.test.tsx` inlines Contract C19 fixture (drift surface)

**Decision**: The test helper `tierStatus(tier, projects, monthlyFeedback)` inlines the four-tier Contract C19 quotas table verbatim rather than importing from a shared fixture file or mocking via a generator.

**Rationale**: This is the **canonical Stage-2-side drift surface** paired with the backend `tier-enforcement-status` Probe B. If Stage 1's `tier_quotas()` rebases, this test FAILS — making contract drift loud. A shared fixture file would also work, but inlining is more obvious to a reader assessing what the test is actually asserting.

**Trade-offs**: Drift between Stage 1 and Stage 2 is detected at vitest time, not earlier. Acceptable: the contract is frozen at the handoff brief; mid-arc rebases require both sides to update together by definition.

**Implementation**: `TierSettings.test.tsx::tierStatus()` helper. Comment at the function header documents the role.

#### `findByRole({name: /Plan & usage/i})` is NOT a useful await target

**Decision**: Tests await a data-bound element (the tier badge text), not the static `<h1>Plan & usage</h1>`.

**Rationale**: The h1 is rendered before any async data loads, so `findByRole` resolves immediately on first paint and tells you nothing about whether the query has settled. One test had to be fixed during develop/test/fix to follow this rule.

**Implementation**: All `findBy*` queries in `TierSettings.test.tsx` target tier-badge text or section content, never the static h1.
