# feedbackmonk — Open Questions

**Source**: inherited from [`intake assessment`](../../planning/intakes/20260512T221154-extract-feedback-system-as-standalone-product.md).

Resolve the **foundational triad** (Q1-Q3) first. The next-tier questions (Q4-Q10) cascade from those answers.

---

## Foundational triad (resolve first)

### Q1 — Target user persona
**Status**: RESOLVED 2026-05-13 → [`DEC-FBR-01`](DECISIONS.md#dec-fbr-01-target-user-persona)

Resolution: Persona A (indie/solo) primary + Persona D (privacy-first) as differentiator. Plausible Analytics shape, not Canny shape.

### Q2 — Market positioning
**Status**: RESOLVED 2026-05-13 → [`DEC-FBR-02`](DECISIONS.md#dec-fbr-02-market-positioning--plausible-analytics-for-product-feedback--privacy-first-product-feedback)

Resolution: "Privacy-first product feedback" / "Plausible Analytics for product feedback." Hero, anti-positioning, per-competitor wedge, and landing-page structure recorded.

### Q3 — Multi-tenancy architecture
**Status**: RESOLVED 2026-05-13 → [`DEC-FBR-03`](DECISIONS.md#dec-fbr-03-multi-tenancy-architecture)

Resolution: shared PostgreSQL, `tenant_id` (org) + `project_id` (product), multi-product-per-tenant mandatory. Pricing-tier shape follows naturally.

**Foundational triad complete.**

---

## Next tier (currently active)

### Q4 — Customers' end-user auth model
**Status**: RESOLVED 2026-05-13 → [`DEC-FBR-04`](DECISIONS.md#dec-fbr-04-end-user-auth-model)

Resolution: three-mode hybrid per project (JWT primary + anonymous fallback + magic-link optional). EdDSA Ed25519 JWT signing; 5-min sliding TTL; per-project signing keys.

Customer-signed JWT embed / OAuth-via-customer-provider / magic-link / anonymous-by-default? Likely NOT PassKey-native (that's GitCellar-specific).

### Q5 — Business model
**Status**: RESOLVED 2026-05-13 → [`DEC-FBR-05`](DECISIONS.md#dec-fbr-05-business-model)

Resolution: Open-source self-host (AGPL-3.0-or-later) + Commercial SaaS, same codebase. Revenue ~90-95% from SaaS subscriptions; optional support contracts later.

### Q6 — Roadmap backend
**Status**: RESOLVED 2026-05-13 → [`DEC-FBR-06`](DECISIONS.md#dec-fbr-06-roadmap-backend)

Resolution: Native PostgreSQL data model + UI. Drop Forge dependency entirely. Status-state machine, voting model, and Q24 privacy invariant port from GitCellar; Gitea bridge code dropped.

### Q7 — Repository home
**Status**: RESOLVED 2026-05-13 → [`DEC-FBR-07`](DECISIONS.md#dec-fbr-07-repository-home)

Resolution: New public GitHub repo at `github.com/feedbackmonk/feedbackmonk` (planned; PF-REGISTER-01 pending), local working dir `E:\Developer\SourceControlled\Apps\feedbackmonk` (initially `Apps\Feedbackr`; renamed 2026-05-14 per PF-RENAME-02). Recommendation shifted from intake-time (a) "in-place" to (b) "new repo" because AGPL changed the calculus — visibility is required for the OSS-as-marketing channel that revenue depends on.

### Q8 — Scope of v1 MVP
**Status**: RESOLVED 2026-05-13 → [`DEC-FBR-08`](DECISIONS.md#dec-fbr-08-mvp-scope)

Resolution: 18 IN-scope items across 5 phases (P0 foundation → P1 closes-loop → P2 customer-facing → P3 commercial → P4 go-public). ~12 weeks FTE to public launch. Attachments / Crash Reporting / Forge Bridge / SSO all deferred or ruled out.

### Q9 — Product name / branding
**Status**: RESOLVED 2026-05-13 → [`DEC-FBR-09`](DECISIONS.md#dec-fbr-09-product-name); **AMENDED 2026-05-14** → [`DEC-FBR-11`](DECISIONS.md#dec-fbr-11-working-name-changed-to-feedbackmonk--dec-fbr-09-squat-contingency-enacted)

Original resolution: "Feedbackr" as working name; real branding pass at P4 (pre-launch). Pre-register `github.com/feedbackr` and `.com`/`.app`/`.dev` early.

**Amendment (DEC-FBR-11)**: pre-public-commit availability scan found `github.com/Feedbackr` and `feedbackr.com` taken by a dormant squatter. DEC-FBR-09's squat-contingency clause activated. Working name changed to **feedbackmonk** (both `github.com/feedbackmonk` and `feedbackmonk.com` confirmed open). DEC-FBR-09's scheduling of the FULL brand pass for P4 is unchanged. ID prefixes `DEC-FBR-*` / `FR-FBR-*` are stable and do NOT rename.

### Q10 — Launch posture
**Status**: RESOLVED 2026-05-13 → [`DEC-FBR-10`](DECISIONS.md#dec-fbr-10-launch-posture)

Resolution: three-stage gradient (dogfood alpha → public AGPL beta → marketed launch). Stage 3 coordinates with GitCellar 1.0 ship date.

---

## All questions resolved ✅

Spec session complete. All 10 critical questions answered, 10 decisions recorded.
