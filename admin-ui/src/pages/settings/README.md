# `pages/settings/` — Admin settings pages (P3 Stage 2 tier; Stage 2 W-D consolidation)

## Synopsis

The five `/admin/settings/*` pages: **tier** (Plan & usage — current-tier card, UsageMeter, capability matrix, UpgradePrompt CTA; Contract C17, FR-FBR-14), **language** (the tenant's default UI locale; Contract C38, FR-FBR-38), **board** (public-board enable + moderation toggles; migration 00016), **hosting** (tenant subdomain + custom domains; FR-FBR-32/33), and **runner-tokens** (runner key + write-token lifecycle; Contract C25, FR-FBR-24). All five are now fully localized (`useTranslation("admin")`), consolidating what earlier stages left split across file-header docs (this index used to cover only the tier page — see Decision Log).

## Purpose & Responsibilities

- **`TierSettings.tsx`** — read-only consumer of `GET /api/v1/admin/tier`. Current-plan card, accessible usage meters (WCAG 2.1 AA color + text dual-encoding), capability matrix (custom branding, custom domain, EU residency, free-tier footer — inverted semantics, see Decision Log), tier-aware upgrade CTA (mailto stub until Polar lands, DEC-FBR-DEFER-01).
- **`LanguageSettings.tsx`** — the tenant's default admin/email locale. A DEFAULT, not an override: a visitor's own choice and an end-user's submitted locale both win over it. "Browser default" (`null`) is a real, persisted choice.
- **`BoardSettings.tsx`** — per-project `public_board_enabled` toggle + a read-only `board_requires_moderation` explainer (v1 always requires moderation; the flag is reserved for a future auto-approve relaxation).
- **`HostingSettings.tsx`** — the tenant's public address: an explicit edit→save subdomain control (changing it moves the board) and the paid custom-domain claim flow, gated by `UpgradePrompt` when the tier lacks the capability.
- **`RunnerTokens.tsx` / `RunnerTokenCard.tsx` / `RunnerTokensList.tsx`** — the complete "enable a runner" surface: register a `runner`-class signing key, optionally register an issued token for visibility, list + revoke tokens. Structural security property surfaced in copy: a runner token can never author `approved` (C22 inv. 2).

## File Index

| File | Role |
|---|---|
| `TierSettings.tsx` | Page component — orchestrates query + sections (card + meters + capabilities + upgrade) |
| `UsageMeter.tsx` | Reusable accessible progressbar with `unlimited` rendering convention |
| `UpgradePrompt.tsx` | Tier-aware upgrade CTA; renders nothing on Self-host |
| `__tests__/TierSettings.test.tsx` | Vitest suite — 13 tests; **inlined Contract C19 fixture is the Stage-2-side drift surface** paired with `tier-enforcement-status` Probe B |
| `LanguageSettings.tsx` | `/admin/settings/language` — the tenant's default UI locale (FR-FBR-38, Contract C38); applies the saved language immediately |
| `__tests__/LanguageSettings.test.tsx` | Vitest suite — precedence of "Browser default" (null), PUT shape, immediate application, `invalid_locale` rejection |
| `BoardSettings.tsx` | `/admin/settings/board` — public-board enable + moderation-required explainer (Contract C28, migration 00016) |
| `HostingSettings.tsx` | `/admin/settings/hosting` — subdomain edit + custom-domain claim/release (FR-FBR-32/33) |
| `__tests__/HostingSettings.test.tsx` | Vitest suite — subdomain save/dirty-state, claim form gated on `custom_domain_available` |
| `RunnerTokens.tsx` | `/admin/settings/runner-tokens` — page shell: the explainer + both register forms + the list (Contract C25, FR-FBR-24) |
| `RunnerTokenCard.tsx` | One registered token row: lifecycle badge (active/revoked/expired), metadata, revoke action + confirm |
| `RunnerTokensList.tsx` | Fetches + renders the registered-token list; empty state explains registration is optional bookkeeping |
| `__tests__/RunnerTokens.test.tsx` | Vitest suite — key registration, token registration, revoke confirm flow |

E2E a11y coverage: `admin-ui/e2e/tier-settings-a11y.spec.ts`, `hosting-settings-a11y.spec.ts` (Playwright + axe-core, per-scenario, `en-US`+`de-DE`), and `moderation-a11y.spec.ts` covers `BoardSettings.tsx`.

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
- **`shared/types.gen.ts`** — `TierStatus` / `TierQuotas` / `Tier` / `TierCapExceededBody` / `isTierCapExceeded` are all consumed. The tier label itself comes from `i18n/useAdminLabels.ts::tier()` (Stage 2 / W-D, R-1) — `TIER_LABELS` no longer exists on this file.
- **Backend pair**: `crates/feedbackmonk-api/src/handlers/admin_tier.rs` is the server side of Contract C17. The `tier-enforcement-status` Verification Oracle (`.claude/project-oracles/tier-enforcement-status/`) Probe B asserts the canonical four-tier shape from the Rust side; **this module's `TierSettings.test.tsx` fixture asserts the same canonical shape from the React side**. Both must update together if Contract C19 rebases.

## Decision Log

### Stage-2 (W-D) index consolidation

**Decision**: This README now indexes all five settings pages instead of only `TierSettings.tsx`'s trio; the earlier note deferring `BoardSettings.tsx`/`HostingSettings.tsx`/`RunnerTokens.tsx`/`RunnerTokenCard.tsx` to their own file headers is retired.

**Rationale**: ULADP documentation parity — the directory grew to five pages before Stage 2, and a module README that only covers 60% of its files misleads an agent orienting from it. The consolidation was explicitly deferred to Stage 2 / W-D by the earlier note; this is that follow-through, done in the same commit as the localization pass that touches every file here.

**Implementation**: File Index above; every page's own header comment still carries its detailed contract notes (Contract C17/C25/C28/C38, FR-FBR-14/24/32/33/38) — this index is the map, not a duplicate of them.

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
