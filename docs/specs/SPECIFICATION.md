# feedbackmonk — Specification

> **Naming note**: working-named "feedbackmonk" through P0 and most of P1; renamed to **feedbackmonk** on 2026-05-14 per [DEC-FBR-11](DECISIONS.md#dec-fbr-11-working-name-changed-to-feedbackmonk--dec-fbr-09-squat-contingency-enacted). ID prefixes `FR-FBR-*` and `DEC-FBR-*` are stable identifiers and do NOT rename. Some inline references below still read "feedbackmonk" (repository-path strings, decision-quote excerpts) -- they are brand references awaiting the next sweep and do NOT indicate identifier instability.

**Status**: READY FOR PLANNING ✅
**Spec session opened**: 2026-05-13
**Spec session closed**: 2026-05-13 (same session)
**Upstream intake**: [`docs/planning/intakes/20260512T221154-extract-feedback-system-as-standalone-product.md`](../../planning/intakes/20260512T221154-extract-feedback-system-as-standalone-product.md)

---

## What feedbackmonk is

feedbackmonk is a **standalone open-source SaaS user-feedback platform** for indie developers and privacy-conscious teams. Submission widget + status-workflow triage + public roadmap with voting + status emails. Multi-product per tenant.

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
9. ✅ **Product name** — "feedbackmonk" working name; brand pass at P4. [`DEC-FBR-09`](DECISIONS.md#dec-fbr-09-product-name)
10. ✅ **Launch posture** — Three-stage gradient (dogfood → public beta → marketed launch). [`DEC-FBR-10`](DECISIONS.md#dec-fbr-10-launch-posture)

---

## Functional Requirements

Derived from [`DEC-FBR-08`](DECISIONS.md#dec-fbr-08-mvp-scope) MVP scope. P0 (FR-FBR-01/02/03/05/06/18) marked **DONE** as of the P0 close commit (2026-05-13); remaining items are PROPOSED until their phase's `/0-uldf-ldis-plan` round promotes them.

### P0 — Foundation (~2 weeks)

| ID | Requirement | Status | Implementation pointer |
|---|---|---|---|
| FR-FBR-01 | Multi-tenant data model with `tenants` + `projects` + RLS-scoped repositories. All domain rows carry `tenant_id` + `project_id`. Tenant-scoped repository layer is the SOLE query path; raw SQL is a security incident. | **DONE** | `crates/feedbackmonk-repository/` + `crates/feedbackmonk-core/` + `migrations/00001_p0_schema.sql`; reconciled from P0 Stage 1 implementation. Oracle `.claude/oracles/multi-tenant-isolation-check/` enforces. See `docs/planning/handoffs/stage1-to-stage2.md` for the frozen Contract C1 surface. |
| FR-FBR-02 | Customer signup + onboarding: email-verify → create org → create first project → display embed code + signing key registration. | **DONE** | `crates/feedbackmonk-api/src/{auth,email,handlers}/` + `crates/feedbackmonk-repository/src/email_verifications.rs` + `migrations/00002_email_verifications.sql`; signup/verify-email/projects/signing-keys endpoints live; argon2 + HMAC-signed admin session; lettre Mailpit/SMTP mailer. Reconciled from P0 Stage 2 (Worker A); 17 unit + 13 integration tests pass. |
| FR-FBR-03 | Submission API `POST /api/v1/projects/{project_id}/feedback` with JWT-verify or anonymous-mode acceptance. | **DONE** | `crates/feedbackmonk-api/src/handlers/feedback.rs`; Contract C3 response shape; auth-mode dispatch (JWT vs anonymous); 11 handler unit tests. Reconciled from P0 Stage 2 (Worker B). |
| FR-FBR-05 | JWT verification with EdDSA (Ed25519), per-project signing keys, multiple active keys for rotation, 5-min sliding TTL, required claims `sub`/`iat`/`exp`/`aud`. | **DONE** | `crates/feedbackmonk-jwt/` enforces all 6 Contract C2 hard invariants (alg-allowlist EdDSA-only, alg-none + HS256-confusion rejection, wrong-aud, expired, missing-claim, oversize-metadata); JWT fixture corpus 24 named tests (Task Zero, all 8 cases a-h + boundary/leeway/RS256 attack) hermetic-deterministic. Reconciled from P0 Stage 2 (Worker B). |
| FR-FBR-06 | Anonymous submission mode with hashed-IP+cookie dedup, optional email field, per-project rate limits, optional verified-email anti-spam gate. | **DONE** | `crates/feedbackmonk-anon/` AnonGate over governor keyed limiter; BLAKE3 domain-separated hash with `feedbackmonk-anon-v1` prefix; 22-char opaque base64url cookie; 11 tests covering determinism + domain separation + 11th-call 429 boundary. Reconciled from P0 Stage 2 (Worker B). |
| FR-FBR-18 | Health endpoint `/health` returning structured JSON; structured logging; basic error-rate observability. | **DONE** | `crates/feedbackmonk-api/src/handlers/health.rs` + `crates/feedbackmonk-repository/src/health.rs` per Contract C5 (`SqlxHealthCheck` ping, JSON body, 200/503 liveness/readiness split); `tracing` JSON formatter + `tower-http::trace::TraceLayer` + `x-request-id` propagation. E2E P0-exit-gate witness `scripts/e2e-p0-curl.sh` PASS 7/7. Reconciled from P0 Stage 3. |

### P1 — Closes the Loop (~3 weeks)

| ID | Requirement | Status | Implementation pointer |
|---|---|---|---|
| FR-FBR-07 | Admin UI: feedback list view + drawer detail + reply composer with public/internal visibility tabs + status transition controls. | **DONE** | `admin-ui/` React+Vite+TypeScript (port 14204) ships state-machine-aware `StatusControls` rendering `LEGAL_TRANSITIONS[currentStatus]` (Contract C6), drawer detail, plain-text reply composer with public/internal visibility tabs. 13 Vitest unit tests + 1 Playwright + `@axe-core/playwright` a11y smoke. Reconciled from P1 Stage 2 (PODS Worker B). |
| FR-FBR-08 | Status workflow: 6-state machine (`submitted` → `triaged` → `in-progress` → `shipped`/`wontfix`/`duplicate`) with audit history in `feedback_status_history`. | **DONE** | `crates/feedbackmonk-core/src/status.rs` ships `FeedbackStatus` + `legal_transitions_from` (Contract C6 state machine, port from gitcellar). `crates/feedbackmonk-api/src/handlers/admin_feedback.rs::transition_status` enforces all 5 hard invariants (illegal-transition pre-DB-check, duplicate-requires-target, scope-bound duplicate-of, same-txn audit row, no-op rejection) — same-transaction `feedback.status` UPDATE + `feedback_status_history` INSERT via `_in_executor` overloads. Migrations 00003 + 00004 applied. Reconciled from P1 Stage 1 + Stage 2 (PODS Worker A). |
| FR-FBR-09 | Status emails (plain-text): confirmation on submission, on each status change, on admin public reply. FB-1234-style display IDs in subject line. Footer parameterized per tenant brand. | **DONE** | `crates/feedbackmonk-api/src/email/` ships 3 plain-text template renderers (confirmation/status-change/public-reply) brand-parameterised via `EmailTenantBrand` (Contract C10); `LettreEmailNotifier` SMTP chokepoint + `RecordingEmailNotifier` test fixture; FB-id in subject `[{prefix} #{FB-XXX}] {short_subject}`; submitter-visible filter (`is_submitter_visible_transition`) skips re-open/un-merge; insta snapshots × 6. Mailpit integration test PASS. P1 Stage 3 e2e witness `scripts/e2e-p1-curl.sh` polls Mailpit for both status-change + public-reply mails. Reconciled from P1 Stage 2 (PODS Worker A) + Stage 3 witness. |
| FR-FBR-10 | PII scrubber with canonical 20-pattern regex set applied to all server logs. Drift-detection oracle. | **DONE** | `crates/feedbackmonk-tracing/` ships byte-for-byte port of GitCellar's 20-pattern scrubber + WRITE-boundary `MakeWriter` chokepoint (Contract C9); `install_global_subscriber` sole composition seam; `pii-scrub-audit` Verification Oracle (Probe A AST + Probe B SHA-256 hash) GREEN on every P1+ commit; idempotent `scrub(scrub(x)) == scrub(x)` asserted. Reconciled from P1 Stage 1 + Stage 3 e2e closes-the-loop witness. |

### P2 — Customer-Facing (~3 weeks)

| ID | Requirement | Status | Implementation pointer |
|---|---|---|---|
| FR-FBR-04 | Embeddable widget: JS+CSS bundle <30KB, themed via customer-config (logo/color), keyboard-accessible (WCAG AA), CSP-compatible, framework-agnostic. | **DONE** | `widget/` greenfield (vanilla TS+CSS, vite lib mode → ESM `widget.js` + sidecar `widget.css`, terser+CSP-safe). Production build 16,829 B (13,281 JS + 3,548 CSS) = 45% headroom under 30,720 cap. ARIA `dialog`/`aria-modal` + focus-trap + ESC-close + focus-return; native fetch; "powered by feedbackmonk" footer rendered when `brand.footer_text` non-null. Contract C12 widget-config handler (`crates/feedbackmonk-api/src/handlers/widget_config.rs`) + `TenantRepo::get_widget_brand` extension + `WidgetBrand` model. `widget-bundle-size` Verification Oracle (Probe A size cap + Probe B 18-hostname tracker scan + list-hash drift) defends FR-FBR-04 + DEC-FBR-02 as code-level invariants. Playwright + axe-core a11y spec: 0 WCAG 2.1 AA violations. |
| FR-FBR-11 | Public roadmap page at `feedbackmonk.com/{tenant}/{project}/roadmap`: anonymous browse, authenticated vote (per Q4 modes), status-label rendering. | **DONE** | Migrations 00006/00007 + `crates/feedbackmonk-core::roadmap` (RoadmapItem/Status/Vote/VoterMode) + `feedbackmonk-repository::{roadmap_items,roadmap_votes}` (Contracts C13/C14) + `feedbackmonk-api::handlers::roadmap` (8 endpoints: 5 public + 3 admin behind AdminSession; chokepoint re-use of `AnonGate::token_hash` + `feedbackmonk_jwt::verify_with_leeway`). Admin UI: `admin-ui/src/pages/roadmap/{PublicRoadmap,AdminRoadmap}.tsx` + routing in `App.tsx`. Public route `/public/projects/:projectId/roadmap`; admin route `/admin/roadmap` (sole-project resolution via `fetchAdminProjects`, see DEC-PODS-C-02). Frozen contracts: `docs/planning/handoffs/p2-fanout-contracts.md` §C13–C15. |
| FR-FBR-12 | Promote-to-roadmap admin action: admin clicks button on feature-request feedback, creates roadmap item, transitions source feedback to `duplicate`. **Q24 privacy invariant** (no submitter attribution; no FB-ID reference). | DONE | `crates/feedbackmonk-api/src/handlers/promote.rs` (byte-for-byte Q24 ports + same-txn atomic pipeline + AdminSession-gated handler) + `admin-ui/src/pages/roadmap/PromoteButton.tsx` (conditional render on `kind=feature && status≠duplicate`). 16 handler tests GREEN: 6 byte-for-byte Q24/render ports from gitcellar `roadmap_promote.rs` + 4 net-new slug helper tests + 6 sqlx integration tests (happy/idempotent/non-feature/invalid-slug/slug-collision/default-title). Frozen contract: `docs/planning/handoffs/p2-fanout-contracts.md#contract-c16`. |
| FR-FBR-13 | Roadmap voting model (1 vote per `(item, voter_id)`) + top-voted aggregator with 60s in-process cache + `GET /api/v1/projects/{project_id}/roadmap/top-voted?limit=N`. | **DONE** | `feedbackmonk-repository::roadmap_votes` enforces `UNIQUE(item_id, voter_id)` invariant (duplicate → `RepoError::Conflict`, mapped to HTTP 409); cross-tenant `item_id` → `NotFound` via `INSERT…SELECT` scope check; 60-second retraction window enforced inside txn with `FOR UPDATE`. `feedbackmonk-api::roadmap_voting_cache::VotingCache` (`Arc<RwLock<CacheInner>>`, per-project bucketing, lazy warming on cold-start, 60s `tokio::time::interval` refresh tick spawned at boot, tick-failure WARN logs + keeps prior payload). Public endpoint `GET /api/v1/projects/{project_id}/roadmap/top-voted?limit=N` returns cached aggregate + `cached_at` timestamp. 18 net-new tests GREEN (13 sqlx::test repo + 5 cache primitives). Frozen contract: `docs/planning/handoffs/p2-fanout-contracts.md` §C14–C15. |

### P3 — Commercial (~2 weeks)

| ID | Requirement | Status | Implementation pointer |
|---|---|---|---|
| FR-FBR-14 | Tier enforcement: projects-per-org caps + monthly volume caps per tier. Free-tier "powered by feedbackmonk" widget footer (opt-out on paid). | DONE (P3 Stage 1 — backend) | Backend impl: `crates/feedbackmonk-core/src/tier.rs` (`Tier` enum + `tier_quotas()` const fn, Contract C19), `crates/feedbackmonk-repository/src/tier_quota.rs` (`check_tier_quota(scope, resource)` chokepoint, Contract C17), `crates/feedbackmonk-api/src/handlers/{projects,feedback,admin_tier}.rs` (predicate wired pre-INSERT + admin status endpoint), `migrations/00008_tenant_tier_check.sql` (CHECK constraint defense-in-depth). Free-tier footer enforced at `crates/feedbackmonk-repository/src/tenants.rs::get_widget_brand` (tier-aware: `Some("powered by feedbackmonk")` on Free, `None` on paid). Verified by `tier-enforcement-status` Verification Oracle (Probe A handler coverage + Probe B `tier_quotas()` shape vs Contract C19 + Probe C `--full` integration smoke trio). Stage 2 ships admin UI surface. |
| FR-FBR-15 | Billing via Polar integration: Free / $9 Starter / $29 Pro / $79 Self-host. MoR via Polar (same provider as GitCellar). | DEFERRED | Per [`DEC-FBR-DEFER-01`](DECISIONS.md#dec-fbr-defer-01-polar-billing-deferred-from-p3) — deferred from P3; tier promotion is operator-in-the-loop via SQL helper at `docs/operations/TIER_OVERRIDE.md`. Webhook envelope + event→tier mapping + schema migration shape captured in `docs/deferred/polar-integration.md` for future port from `gitcellar-cloud/src/billing/`. |

### P4 — Go-Public (~2 weeks)

| ID | Requirement | Status | Implementation pointer |
|---|---|---|---|
| FR-FBR-16 | Marketing site / landing at feedbackmonk.com: Astro build, hero + pricing + docs + Show HN-ready copy. Open-source per [`DEC-FBR-05`](DECISIONS.md#dec-fbr-05-business-model). | PROPOSED | NEW; pattern from `gitcellar-landing/` |
| FR-FBR-17 | Self-host distribution: `docker compose up` deploys full stack, env-var config, migration runner, backup docs. | PROPOSED | NEW |

---

## Oracles (for implementing agents)

Oracle candidates surfaced during spec. To be authored during P0-P1 or as Task Zero of the first plan task. Decision: `/0-uldf-ldis-plan` finalizes build-vs-defer per candidate.

| Oracle | Question | Strategy | Build Status |
|---|---|---|---|
| `widget-bundle-size` | Current widget JS+CSS bundle size; FR-FBR-04 caps it at 30KB | trigger-invalidate (on build) | **Verification Oracle — build before P2 ships** |
| `multi-tenant-isolation-check` | Verify no cross-tenant data leakage in any query path (FR-FBR-01) | trigger-invalidate (on schema/access-layer change) | **BUILT (P0 Stage 1 Task Zero)** — `.claude/oracles/multi-tenant-isolation-check/` (Python canonical + ps1/sh shims). PASS on every Stage 1 commit. |
| `tier-enforcement-status` | Confirm each pricing-tier cap fires correctly (FR-FBR-14) | trigger-invalidate (handler/tier-config change) + `--full` integration smoke | **BUILT (P3 Stage 1 Task Zero)** — `.claude/oracles/tier-enforcement-status/` (Python canonical + ps1/sh shims). Probe A (AST: every `crates/feedbackmonk-api/src/handlers/*.rs` writer either calls `check_tier_quota` or is allowlisted) + Probe B (Contract C19 shape over `tier_quotas()`) + Probe C (`--full`: `cargo test --test tier_enforcement_smoke`, 3-test smoke trio for cap-firing + footer flip). PASS on every P3+ commit. *(reconciled from implementation)* |
| `pii-scrub-audit` | Drift-detection over canonical PII pattern set (FR-FBR-10) | freshness via pattern-set hash | **BUILT (P1 Stage 1 Task Zero)** — `.claude/oracles/pii-scrub-audit/` (Python canonical + bash shim). Probe A (AST: no tracing-subscriber setup outside `crates/feedbackmonk-tracing/`) + Probe B (SHA-256 hash drift on `CANONICAL_PATTERNS`). PASS on every P1+ commit. *(reconciled from implementation)* |

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
