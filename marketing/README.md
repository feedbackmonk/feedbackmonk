# marketing/ — feedbackmonk Marketing Site

<!-- Agent Context Header (ULADP) -->
**Synopsis (1-5 lines)**:
Greenfield Astro static site (built at P4 Stage 2 — collab-20260514-170323)
serving `feedbackmonk.com` (PF-REGISTER-01 cleared — org + domain registered; site not yet deployed to the domain). Seven pages per
Contract C23 — home / pricing / docs index + widget + api + self-host /
Show HN draft. Pricing tiers are sourced **at build time** from
`feedbackmonk-core::tier::tier_quotas()` via a Rust→JSON prebuild step
(DEC-FBR-IMPL-05). Self-hosted Inter + JetBrains Mono fonts (no Google
CDN — BRAND.md mandate extending DEC-FBR-02). Zero third-party trackers —
verified by `grep dist/` on every build.

## Purpose & Responsibilities

The public-facing marketing surface for feedbackmonk. Serves:

- **`/`** — hero with three trust signals + product narrative + CTA.
- **`/pricing`** — Free / Starter $9 / Pro $29 / Self-host $79 cards built
  from `src/data/tier_quotas.json` (the prebuild-generated mirror of
  `tier_quotas()`). CTAs are mailto stubs (Polar deferred per DEC-FBR-DEFER-01).
- **`/docs/`** — index linking widget / api / self-host docs.
- **`/docs/widget`** — embed instructions per FR-FBR-04 contract.
- **`/docs/api`** — public API reference (handlers, JWT semantics, error shapes).
- **`/docs/self-host`** — content-mirror snapshot of `docs/operations/SELFHOST.md`.
- **`/blog/show-hn-draft`** — Show HN announcement draft per BRAND.md §Show HN Voice.

Per [`DEC-FBR-05`](../docs/specs/DECISIONS.md), this directory is AGPL-3.0-or-later
just like the rest of the product — no separate proprietary marketing repo.

## File Index

| File / Dir | Role |
|---|---|
| `README.md` | This file (ULADP module README). |
| `package.json` | Astro + Playwright + axe-core devDeps; `prebuild`/`predev` hook the Rust→JSON pricing export; `test:a11y` script for the smoke spec. Pins `@rollup/rollup-win32-x64-msvc` to work around the npm optional-platform-dep bug (same workaround as `admin-ui/`). |
| `astro.config.mjs` | Astro config; `site = https://feedbackmonk.com`; `trailingSlash: 'ignore'`; preview/dev on port 14210 with `strictPort: true` (per DEC-FBR-IMPL-04). |
| `tsconfig.json` | Extends `astro/tsconfigs/strict`. |
| `playwright.config.ts` | A11y smoke harness; webServer spawns `astro preview` on 14210; Windows-aware bin path (mirrors `admin-ui/playwright.config.ts`). |
| `.gitignore` | `node_modules/`, `dist/`, `.astro/`, `test-results/`, **and `src/data/tier_quotas.json`** (the pricing JSON is generated, not source — DEC-FBR-IMPL-05). |
| `scripts/run-export.mjs` | Cross-platform dispatcher invoked by `prebuild` + `predev`. Calls `cargo run -p feedbackmonk-core --example export_tier_quotas` and writes stdout to `src/data/tier_quotas.json`. |
| `scripts/export-tier-quotas.sh` | POSIX shim for the same export (kept as documentation + manual-invocation surface). |
| `scripts/export-tier-quotas.ps1` | Windows PowerShell mirror. |
| `scripts/fetch-fonts.mjs` | One-shot fetch of `InterVariable.woff2` + `JetBrainsMono-Variable.woff2` into `public/fonts/`. Operator runs this once after `npm install` if the fonts aren't already committed. |
| `public/favicon.svg` | Inline-text `fm` favicon (24×24 ink-on-cream); no logo-mark per BRAND.md. |
| `public/fonts/InterVariable.woff2` | Self-hosted Inter Variable (SIL OFL 1.1; ~352 KB; safe to redistribute). |
| `public/fonts/JetBrainsMono-Variable.woff2` | Self-hosted JetBrains Mono Variable (Apache-2.0; ~114 KB). |
| `src/styles/global.css` | The 6 brand tokens as `:root` CSS custom properties (BRAND.md §Color Palette) + 3 additive properties (`--brand-radius`, `--brand-shadow`, `--brand-content-max`); `@font-face` declarations for the two self-hosted families; typography clamps from BRAND.md; reset + skip-link + reduced-motion overrides. |
| `src/layouts/BaseLayout.astro` | `<html lang="en">` + viewport + per-page title/description/canonical/OG meta + theme-color; preloads Inter; mounts `<Nav>` + `<main id="main" class="container">` + `<Footer>`. |
| `src/components/Nav.astro` | Header with text-only `feedbackmonk` wordmark + Pricing / Docs nav. `aria-current="page"` on the active link. |
| `src/components/Footer.astro` | Carries the FR-FBR-14 brand-promise string verbatim (`powered by feedbackmonk`) + footer nav + AGPL repo link + email CTA. |
| `src/components/PricingCard.astro` | Consumes one `TierEntry` from `src/data/tier_quotas.json`. Renders name + price + caps + capability checkmarks + CTA (mailto stub for paid tiers; `/docs/self-host` link for Self-host). |
| `src/components/TrustSignal.astro` | Hero's three trust signals (EU+US hosting, AGPL, zero trackers). |
| `src/data/tier_quotas.json` | **gitignored** — regenerated on every `npm run prebuild`. Hand-edits are a DEC-FBR-IMPL-05 violation. |
| `src/pages/index.astro` | Hero + trust signals + 4-paragraph product narrative + final CTA. |
| `src/pages/pricing.astro` | Renders all four `TierEntry`s in canonical order (Free → Starter → Pro → Self-host); Pro is `highlighted`. |
| `src/pages/docs/index.astro` | 3-card index. |
| `src/pages/docs/widget.astro` | Quickstart + auth modes + programmatic mount + widget-config shape + invariants (size, trackers, CSP, a11y). |
| `src/pages/docs/api.astro` | Auth + widget endpoints + roadmap endpoints + admin endpoints + error shape + health endpoints. |
| `src/pages/docs/self-host.astro` | Content-mirror snapshot of `docs/operations/SELFHOST.md` (P4 Stage 2 freeze). Links to canonical full runbook for depth. |
| `src/pages/blog/show-hn-draft.astro` | Draft post per BRAND.md §Show HN Voice. Unlisted — not linked from nav. |
| `tests/a11y.spec.ts` | 11 Playwright + axe-core assertions: 7-page WCAG 2.1 A+AA smoke + skip-link + tier rendering + FR-FBR-14 footer + brand attribution. |

## Public API & Usage

### One-shot setup (operator)

```bash
cd marketing
npm install                # ~149 packages
npm run fetch-fonts        # ~466 KB into public/fonts/ (idempotent)
```

### Build

```bash
npm run prebuild           # generates src/data/tier_quotas.json from tier_quotas()
npm run build              # astro build → dist/
```

`npm run build` runs `prebuild` automatically; the explicit invocation is
useful for diagnosing the JSON shape on its own.

### Dev / preview

```bash
npm run dev                # astro dev on 127.0.0.1:14210 (strictPort)
npm run preview            # astro preview from dist/ on the same port
```

Port **14210** is reserved in `~/.claude/MACHINE_CONFIG.md` Dev Port
Registry (logged via `channels/messages.md` for LD to register at
convergence). `strictPort: true` prevents silent collision per
DEC-FBR-IMPL-04.

### Tests

```bash
npm run test:a11y          # Playwright + axe-core, 11 assertions, ~12s
```

Webserver auto-spawns `astro preview`; runs against the built static
output. Set `CI=1` to fail on `test.only`.

### Pricing-export shim (manual invocation)

```bash
./scripts/export-tier-quotas.sh           # POSIX
pwsh ./scripts/export-tier-quotas.ps1     # Windows
# Output: marketing/src/data/tier_quotas.json (~836 bytes)
```

## Constraints & Business Rules

### Hard invariants (Verification-Oracle defended or build-step gated)

1. **No third-party trackers in built artifacts.** DEC-FBR-02 brand promise.
   Exit-gate `grep` on `dist/` against `googletagmanager|google-analytics|
   segment.io|mixpanel.com|intercom.io|hotjar.com|fullstory.com|amplitude.com|
   plausible.io|cloudflareinsights.com|fonts.googleapis.com|fonts.gstatic.com`
   MUST return zero matches. (Confirmed at Stage 2 close: zero matches.)
2. **Pricing parity.** `/pricing` reads `src/data/tier_quotas.json` only —
   hand-editing it is a DEC-FBR-IMPL-05 violation. The file is gitignored;
   it regenerates on every `npm run prebuild`. The `tier-enforcement-status`
   Verification Oracle Probe B asserts `tier_quotas()`'s shape against
   Contract C19, so the JSON is canonical by transitivity.
3. **Self-hosted fonts only.** No `fonts.googleapis.com` or any CDN.
   `@font-face` `src` URLs point at `/fonts/*.woff2` paths served from
   `public/fonts/`. (Verified at Stage 2 close.)
4. **AGPL footer + repo link on every page.** `Footer.astro` is mounted
   by `BaseLayout.astro` and rendered on every page.
5. **FR-FBR-14 brand-promise footer string verbatim.** `Footer.astro`
   carries the literal `powered by feedbackmonk` with a link to
   `feedbackmonk.com`. PricingCard renders the same string verbatim
   from `tier_quotas.json` for the Free tier card; tests assert both.
6. **Polar deferred.** `/pricing` CTAs are `mailto:` stubs per DEC-FBR-DEFER-01.
   Adding a Polar checkout button is a halt (no LD ratification path for it
   in this phase).
7. **A11y zero violations.** Playwright + axe-core WCAG 2.1 A+AA on every
   page. Failures fail the build.

### Soft invariants (style / tone)

- BRAND.md §Voice & Tone govern copy: direct, evidenced, no marketing-speak.
  The ban list (`leverage`, `synergize`, `robust`, `world-class`,
  `cutting-edge`, `paradigm`, `holistic`, `ecosystem`, `journey`, `empower`)
  was scrubbed during Stage 2 authoring.
- No exclamation points in body copy. No emoji-as-decoration.
- Dark mode is deferred to v1.1 — body is hardcoded `color-scheme: light`.

## Relationships & Dependencies

- **Brand kit consumer** (Contract C20, read-only): `docs/brand/BRAND.md`.
  Six color tokens defined as `:root` CSS custom properties in
  `src/styles/global.css`. Three additive tokens (`--brand-radius`,
  `--brand-shadow`, `--brand-content-max`) per GUIDE.md §8 pre-authorization
  (no new hue).
- **Pricing SSOT consumer** (Contract C19, read-only): the export binary at
  `crates/feedbackmonk-core/examples/export_tier_quotas.rs` reads
  `feedbackmonk_core::tier::tier_quotas()` and emits JSON. The four-variant
  shape matches the function's return type byte-for-byte (no rename, no
  restructure — the `tier` discriminator is the only added field, taken
  verbatim from `Tier::as_db_str`).
- **Self-host docs consumer** (read-only): `/docs/self-host` is a snapshot
  of `docs/operations/SELFHOST.md` (Worker B, Frozen at the Stage 2 freeze
  ping `[B → A] SELFHOST.md frozen at HEAD ... 17:35Z`). Refresh when the
  canonical runbook changes substantively.
- **Env catalog consumer** (Contract C21, read-only): the `/docs/self-host`
  page links to `docs/operations/SELFHOST_ENV.md` rather than duplicating
  the full env table (avoids drift).
- **Widget docs sourcing**: `/docs/widget` content is grounded in
  `widget/README.md` and the FR-FBR-04 contract.
- **No GitCellar dependency**: per DEC-FBR-07, this directory is greenfield.
  No port pattern, no extraction.

## Decision Log

- **2026-05-14** — Pricing-export binary location: **A1** (`crates/feedbackmonk-core/examples/export_tier_quotas.rs`) over A2 (new crate). Rationale: `feedbackmonk-core` already depends on `serde` + `serde_json`; an example binary is the lightest weight that satisfies DEC-FBR-IMPL-05. No new workspace member needed. Invocation: `cargo run --quiet -p feedbackmonk-core --example export_tier_quotas`.
- **2026-05-14** — Pricing-export JSON shape: `Vec<{ tier: <discriminator>, ...TierQuotas via serde flatten }>`. Self-describing (every consumer can round-trip), flat (no nested `quotas:` wrapper), field-shape matches `TierQuotas` verbatim with the `tier` discriminator added (no rename/restructure of existing fields per task spec).
- **2026-05-14** — Cross-platform prebuild: a Node dispatcher (`scripts/run-export.mjs`) is the canonical entry point; the `.sh` / `.ps1` shims are retained for manual invocation. Node is already present (Astro requires it), so dispatching via Node avoids `&&` / `||` shell incompatibility between PowerShell and bash.
- **2026-05-14** — Astro dev/preview port: **14210**. Chosen from the 14200-range per the project's Dev Port Registry; deconflicted from admin-ui (14204), widget e2e (14205), widget preview (14206), and api (14304). `strictPort: true` in `astro.config.mjs` and `playwright.config.ts`.
- **2026-05-14** — `/docs/self-host` strategy: **content-mirror snapshot** rather than MDX-import. Rationale: the marketing-side page is a "is this for me + happy path" introduction, not a verbatim operator runbook. The canonical operator-side version stays in `docs/operations/SELFHOST.md` (terminal-readable, refreshed post-launch from real incidents); the marketing page links there for backup/restore depth + full troubleshooting matrix. The snapshot date is logged at the top of `src/pages/docs/self-host.astro` so future refreshes have a known starting state.
- **2026-05-14** — Self-hosted fonts: **Inter Variable + JetBrains Mono Variable** as single `.woff2` files (~466 KB combined). Committed to `public/fonts/` rather than fetched at runtime per BRAND.md mandate. `scripts/fetch-fonts.mjs` exists for re-fetch if the files are ever pruned.
- **2026-05-14** — Brand-token additive widening: 3 new tokens (`--brand-radius: 0.375rem`, `--brand-shadow`, `--brand-content-max: 68ch`). All structural / spatial — no new color hue. Pre-authorized per GUIDE.md §8 (CSS custom properties beyond the 6 BRAND.md tokens, additive, harmonize with palette).
- **2026-05-14** — Show HN draft word count: ~280 (slightly over BRAND.md's 150-250 guidance). Maintainer's revision pass will tighten. Frontmatter is `draft: true`; no nav link.
- **2026-05-14** — `@rollup/rollup-win32-x64-msvc` pinned in devDeps as a workaround for the npm optional-platform-dep bug (npm/cli#4828). Same workaround `admin-ui/` uses; pins the same major version (`^4.60.3`).

## Lineage

- **FR-FBR-16** — Astro marketing site at feedbackmonk.com
- **Contract C20** — Brand kit (`docs/brand/BRAND.md`)
- **Contract C21** — Self-host env catalog (`docs/operations/SELFHOST_ENV.md`)
- **Contract C22** — Pricing SSOT mechanism (build-step Rust→JSON, DEC-FBR-IMPL-05)
- **Contract C23** — URL routing scheme (the seven pages)
- **Contract C19** — `tier_quotas()` shape (consumed by the export binary)
- **DEC-FBR-02** — No third-party trackers, brand promise
- **DEC-FBR-05** — AGPL-3.0-or-later; no proprietary marketing repo
- **DEC-FBR-07** — Greenfield (no GitCellar port pattern)
- **DEC-FBR-IMPL-05** — Pricing SSOT = build-time Rust→JSON
- **DEC-FBR-IMPL-04** — Dev Port Registry + `strictPort: true`
- **DEC-FBR-DEFER-01** — Polar billing deferred (CTAs are mailto stubs)
- **PODS session** — `collab-20260514-170323` (CLAUDE-A worker)
