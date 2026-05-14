# marketing/ — feedbackmonk Marketing Site

> **Stage**: P4 Stage 1 SKELETON. Scaffolded with a README only; the Astro project itself is initialized by **P4 Stage 2 Worker A** per the plan at `docs/planning/plans/20260514T163356-feedbackmonk-p4-go-public.md`.

## Purpose & Responsibilities

The marketing site at `feedbackmonk.com` (once PF-REGISTER-01 clears). Astro static-site build. Serves:

- The hero / pricing / docs / blog frontpage
- The public-facing self-host runbook (sourced from `docs/operations/SELFHOST.md`)
- The Show HN draft (`/blog/show-hn-draft`; unlisted until launch)
- The widget embed instructions page (`/docs/widget`)

Per [`DEC-FBR-05`](../docs/specs/DECISIONS.md#dec-fbr-05-business-model), the marketing site is AGPL-3.0-or-later just like the rest of the product; there is no separate proprietary marketing repo.

## File Index

(Stage 1 SKELETON: only this README. Stage 2 Worker A populates the full Astro project.)

| File | Purpose |
|---|---|
| `README.md` | This file. |

## Public API & Usage

**Build** (Stage 2 onward): `npm install && npm run prebuild && npm run build`. The `prebuild` step runs the Rust→JSON pricing export per [`DEC-FBR-IMPL-05`](../docs/specs/DECISIONS.md#dec-fbr-impl-05-p4-marketing-site-pricing-single-source-of-truth--build-time-rustjson-export); the `build` step runs Astro's static site generation.

**Preview**: `npm run preview` after `npm run build`. **Dev server**: `npm run dev` (Astro's HMR; port assigned via Vite — TODO add to Dev Port Registry at Stage 2).

## Constraints & Business Rules

- **Brand kit**: [`docs/brand/BRAND.md`](../docs/brand/BRAND.md) is Contract C20 — colors, fonts, voice, footer attribution. Worker A imports from there; do not invent new tokens here.
- **No third-party trackers** (DEC-FBR-02): no GA, no Mixpanel, no Segment, no Intercom embed, no Hotjar. Privacy-positioning is load-bearing for the target persona.
- **Self-hosted fonts** (per BRAND.md): Inter + JetBrains Mono live under `public/fonts/`; do NOT fetch from Google Fonts CDN.
- **AGPL footer + repo link**: every page footer carries the canonical `powered by feedbackmonk` attribution string and a link to the GitHub repo (once registered).
- **Pricing parity**: `/pricing` page reads from `src/data/tier_quotas.json` (build-time generated from `feedbackmonk-core::tier::tier_quotas()`). Do NOT hand-edit `tier_quotas.json`; do NOT hardcode tier caps in MDX/Astro. Per DEC-FBR-IMPL-05.
- **Polar deferred**: per [`DEC-FBR-DEFER-01`](../docs/specs/DECISIONS.md#dec-fbr-defer-01-polar-billing-deferred-from-p3), the "Get Started" / "Upgrade" CTAs link to a contact-form / mailto stub, NOT to a checkout flow. Same seam as the admin UI's Upgrade button.

## Relationships & Dependencies

- **Brand kit consumer**: imports tokens + voice guidelines from `docs/brand/BRAND.md` (Contract C20).
- **Pricing SSOT consumer**: imports JSON-exported `tier_quotas()` from `crates/feedbackmonk-core/` via build-step shim (DEC-FBR-IMPL-05).
- **Self-host docs consumer**: `/docs/self-host` page sources from `docs/operations/SELFHOST.md` (rendered or imported as MDX). Env-var reference table sources from `docs/operations/SELFHOST_ENV.md` (Contract C21).
- **Widget docs consumer**: `/docs/widget` describes the FR-FBR-04 embed contract — JSON shape, JWT header, anonymous cookie behavior. References `widget/README.md` for the implementation detail.

## Decision Log

- **2026-05-14** — Greenfield Astro project (not ported from gitcellar-landing). Rationale: gitcellar-landing was opportunistic prior art, not architectural reference. P4 builds fresh against the v1 brand kit.
- **2026-05-14** — `marketing/` is the chosen top-level directory name (vs `site/`, `landing/`, `www/`). Rationale: mirrors gitcellar-landing's role in the peer repo; "marketing" is precise (not just a static site — it's the marketing surface).
- **2026-05-14** — Worker A's Task Zero is the Rust→JSON pricing export pipeline + the Astro project init (in that order). Rationale: locking the pricing-parity scaffolding before page authoring means /pricing is always built against the canonical source.
