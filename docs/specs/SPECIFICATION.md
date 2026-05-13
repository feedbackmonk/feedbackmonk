# Feedbackr — Specification

**Status**: READY FOR PLANNING ✅
**Spec session opened**: 2026-05-13
**Spec session closed**: 2026-05-13 (same session)
**Upstream intake**: [`docs/planning/intakes/20260512T221154-extract-feedback-system-as-standalone-product.md`](../../planning/intakes/20260512T221154-extract-feedback-system-as-standalone-product.md)

---

## What Feedbackr is

Feedbackr is a **standalone open-source SaaS user-feedback platform** for indie developers and privacy-conscious teams. Submission widget + status-workflow triage + public roadmap with voting + status emails. Multi-product per tenant.

**One-line pitch**: "Privacy-first product feedback. Hear your users without spying on them."
**Elevator pitch ("X for Y")**: "Plausible Analytics for product feedback."

This spec is for the standalone product — separate from `docs/specs/feedback-system/` which documents GitCellar's internal feedback module (the reference implementation).

## Foundational triad — COMPLETE ✅

1. ✅ **Target user persona** — Persona A (indie/solo founders) primary + Persona D (privacy-first) as differentiator. [`DEC-FBR-01`](DECISIONS.md#dec-fbr-01-target-user-persona)
2. ✅ **Market positioning** — "Privacy-first product feedback" / "Plausible Analytics for product feedback." [`DEC-FBR-02`](DECISIONS.md#dec-fbr-02-market-positioning)
3. ✅ **Multi-tenancy architecture** — Shared PostgreSQL, `tenant_id` + `project_id`, multi-product-per-tenant mandatory. [`DEC-FBR-03`](DECISIONS.md#dec-fbr-03-multi-tenancy-architecture)

## Next-tier decisions — COMPLETE ✅

4. ✅ **End-user auth model** — three-mode hybrid (JWT EdDSA primary + anonymous fallback + magic-link optional). [`DEC-FBR-04`](DECISIONS.md#dec-fbr-04-end-user-auth-model)
5. ✅ **Business model** — Open-source self-host (AGPL-3.0-or-later) + Commercial SaaS, same codebase. [`DEC-FBR-05`](DECISIONS.md#dec-fbr-05-business-model)
6. ✅ **Roadmap backend** — Native PostgreSQL data model + UI; drop Forge dependency. [`DEC-FBR-06`](DECISIONS.md#dec-fbr-06-roadmap-backend)
7. ✅ **Repository home** — New public GitHub repo (`E:\Developer\SourceControlled\Apps\Feedbackr` locally). [`DEC-FBR-07`](DECISIONS.md#dec-fbr-07-repository-home)
8. ✅ **MVP scope** — 18 IN-scope items, 5 phases, ~12 weeks FTE. [`DEC-FBR-08`](DECISIONS.md#dec-fbr-08-mvp-scope)
9. ✅ **Product name** — "Feedbackr" working name; brand pass at P4. [`DEC-FBR-09`](DECISIONS.md#dec-fbr-09-product-name)
10. ✅ **Launch posture** — Three-stage gradient (dogfood → public beta → marketed launch). [`DEC-FBR-10`](DECISIONS.md#dec-fbr-10-launch-posture)

---

## Functional Requirements

Derived from [`DEC-FBR-08`](DECISIONS.md#dec-fbr-08-mvp-scope) MVP scope. All 18 are PROPOSED status; status promotion to CONFIRMED happens at `/0-uldf-ldis-plan`.

### P0 — Foundation (~2 weeks)

| ID | Requirement | Status | Implementation pointer |
|---|---|---|---|
| FR-FBR-01 | Multi-tenant data model with `tenants` + `projects` + RLS-scoped repositories. All domain rows carry `tenant_id` + `project_id`. Tenant-scoped repository layer is the SOLE query path; raw SQL is a security incident. | **DONE** | `crates/feedbackr-repository/` + `crates/feedbackr-core/` + `migrations/00001_p0_schema.sql`; reconciled from P0 Stage 1 implementation. Oracle `.claude/oracles/multi-tenant-isolation-check/` enforces. See `docs/planning/handoffs/stage1-to-stage2.md` for the frozen Contract C1 surface. |
| FR-FBR-02 | Customer signup + onboarding: email-verify → create org → create first project → display embed code + signing key registration. | PROPOSED | NEW |
| FR-FBR-03 | Submission API `POST /api/v1/projects/{project_id}/feedback` with JWT-verify or anonymous-mode acceptance. | PROPOSED | Port + extend; ref [`gitcellar-cloud/src/feedback/routes.rs`](../../../gitcellar-cloud/src/feedback/routes.rs) |
| FR-FBR-05 | JWT verification with EdDSA (Ed25519), per-project signing keys, multiple active keys for rotation, 5-min sliding TTL, required claims `sub`/`iat`/`exp`/`aud`. | PROPOSED | NEW; see [`DEC-FBR-04`](DECISIONS.md#dec-fbr-04-end-user-auth-model) |
| FR-FBR-06 | Anonymous submission mode with hashed-IP+cookie dedup, optional email field, per-project rate limits, optional verified-email anti-spam gate. | PROPOSED | NEW |
| FR-FBR-18 | Health endpoint `/health` returning structured JSON; structured logging; basic error-rate observability. | PROPOSED | Port; ref [`gitcellar-cloud/src/main.rs`](../../../gitcellar-cloud/src/main.rs) health endpoint shape |

### P1 — Closes the Loop (~3 weeks)

| ID | Requirement | Status | Implementation pointer |
|---|---|---|---|
| FR-FBR-07 | Admin UI: feedback list view + drawer detail + reply composer with public/internal visibility tabs + status transition controls. | PROPOSED | Port React components; ref [`gitcellar-cloud/admin-ui/`](../../../gitcellar-cloud/admin-ui/) |
| FR-FBR-08 | Status workflow: 6-state machine (`submitted` → `triaged` → `in-progress` → `shipped`/`wontfix`/`duplicate`) with audit history in `feedback_status_history`. | PROPOSED | Port; ref [`gitcellar-cloud/src/feedback/db.rs`](../../../gitcellar-cloud/src/feedback/db.rs) |
| FR-FBR-09 | Status emails (plain-text): confirmation on submission, on each status change, on admin public reply. FB-1234-style display IDs in subject line. Footer parameterized per tenant brand. | PROPOSED | Port + parameterize; ref [`gitcellar-cloud/src/feedback/email_templates.rs`](../../../gitcellar-cloud/src/feedback/email_templates.rs) |
| FR-FBR-10 | PII scrubber with canonical 20-pattern regex set applied to all server logs. Drift-detection oracle. | PROPOSED | Port verbatim; ref [`gitcellar-service/src/feedback_logs/scrubber.rs`](../../../gitcellar-service/src/feedback_logs/scrubber.rs) |

### P2 — Customer-Facing (~3 weeks)

| ID | Requirement | Status | Implementation pointer |
|---|---|---|---|
| FR-FBR-04 | Embeddable widget: JS+CSS bundle <30KB, themed via customer-config (logo/color), keyboard-accessible (WCAG AA), CSP-compatible, framework-agnostic. | PROPOSED | NEW |
| FR-FBR-11 | Public roadmap page at `feedbackr.com/{tenant}/{project}/roadmap`: anonymous browse, authenticated vote (per Q4 modes), status-label rendering. | PROPOSED | Port logic; ref [`gitcellar-cloud/src/feedback/roadmap_routes.rs`](../../../gitcellar-cloud/src/feedback/roadmap_routes.rs) |
| FR-FBR-12 | Promote-to-roadmap admin action: admin clicks button on feature-request feedback, creates roadmap item, transitions source feedback to `duplicate`. **Q24 privacy invariant** (no submitter attribution; no FB-ID reference). | PROPOSED | Port; ref [`gitcellar-cloud/src/feedback/roadmap_promote.rs`](../../../gitcellar-cloud/src/feedback/roadmap_promote.rs) |
| FR-FBR-13 | Roadmap voting model (1 vote per `(item, voter_id)`) + top-voted aggregator with 60s in-process cache + `GET /api/v1/projects/{project_id}/roadmap/top-voted?limit=N`. | PROPOSED | Port; ref [`gitcellar-cloud/src/feedback/roadmap_voting.rs`](../../../gitcellar-cloud/src/feedback/roadmap_voting.rs) |

### P3 — Commercial (~2 weeks)

| ID | Requirement | Status | Implementation pointer |
|---|---|---|---|
| FR-FBR-14 | Tier enforcement: projects-per-org caps + monthly volume caps per tier. Free-tier "powered by Feedbackr" widget footer (opt-out on paid). | PROPOSED | NEW; see [`DEC-FBR-03`](DECISIONS.md#dec-fbr-03-multi-tenancy-architecture) pricing tiers |
| FR-FBR-15 | Billing via Polar integration: Free / $9 Starter / $29 Pro / $79 Self-host. MoR via Polar (same provider as GitCellar). | PROPOSED | Port Polar setup pattern; ref [`gitcellar-cloud/src/billing/`](../../../gitcellar-cloud/src/billing/) |

### P4 — Go-Public (~2 weeks)

| ID | Requirement | Status | Implementation pointer |
|---|---|---|---|
| FR-FBR-16 | Marketing site / landing at feedbackr.com (or final-name.com): Astro build, hero + pricing + docs + Show HN-ready copy. Open-source per [`DEC-FBR-05`](DECISIONS.md#dec-fbr-05-business-model). | PROPOSED | NEW; pattern from `gitcellar-landing/` |
| FR-FBR-17 | Self-host distribution: `docker compose up` deploys full stack, env-var config, migration runner, backup docs. | PROPOSED | NEW |

---

## Oracles (for implementing agents)

Oracle candidates surfaced during spec. To be authored during P0-P1 or as Task Zero of the first plan task. Decision: `/0-uldf-ldis-plan` finalizes build-vs-defer per candidate.

| Oracle | Question | Strategy | Build Status |
|---|---|---|---|
| `widget-bundle-size` | Current widget JS+CSS bundle size; FR-FBR-04 caps it at 30KB | trigger-invalidate (on build) | **Verification Oracle — build before P2 ships** |
| `multi-tenant-isolation-check` | Verify no cross-tenant data leakage in any query path (FR-FBR-01) | trigger-invalidate (on schema/access-layer change) | **BUILT (P0 Stage 1 Task Zero)** — `.claude/oracles/multi-tenant-isolation-check/` (Python canonical + ps1/sh shims). PASS on every Stage 1 commit. |
| `tier-enforcement-status` | Confirm each pricing-tier cap fires correctly (FR-FBR-14) | always-fresh on cap-check | **Verification Oracle — build during P3** |
| `pii-scrub-audit` | Drift-detection over canonical PII pattern set (FR-FBR-10) | freshness via pattern-set hash | Port from GitCellar's existing oracle |

Project-state oracles (not Verification Oracles):

| Oracle | Question | Strategy | Build Status |
|---|---|---|---|
| `feedbackr-tier-quotas` | Current per-org project count + monthly volume vs configured tier limits | trigger-invalidate (on tier change / monthly rollover) | Optional — useful for admin dashboard; defer to v1.1 |

---

## Architecture

See [`ARCHITECTURE.md`](ARCHITECTURE.md).

## Decisions

See [`DECISIONS.md`](DECISIONS.md) (10 decisions, DEC-FBR-01..10, all RESOLVED).

## Open Questions

See [`OPEN_QUESTIONS.md`](OPEN_QUESTIONS.md) (10 of 10 RESOLVED).

---

## Spec session — COMPLETE ✅

**Verdict**: READY FOR `/0-uldf-ldis-plan`.

All 10 critical questions resolved with 10 corresponding decisions recorded. 18 functional requirements (FR-FBR-01..18) span 5 implementation phases. Architecture skeletal but adequate for planning. Oracle candidates surfaced.

**Recommended next step**: `/0-uldf-proceed` (context-budget-aware router will choose topology — likely HANDOFF to a fresh session given context consumed and implementation scope ahead).
