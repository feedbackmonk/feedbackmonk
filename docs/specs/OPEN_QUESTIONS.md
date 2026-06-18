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

## v1 questions resolved ✅

v1 spec session complete. All 10 critical questions (Q1–Q10) answered, 10 decisions recorded.

---

## P5 — Agentic Feedback Resolution Loop (OPEN — spec session opened 2026-06-18)

New scope beyond v1 (see SPECIFICATION.md § P5, FR-FBR-19..25). These cascade from the open-core boundary decision [`DEC-FBR-12`](DECISIONS.md#dec-fbr-12-open-core-boundary-for-the-agentic-loop-feedbackmonk-autopilot) (architecture DECIDED; packaging DEFERRED). **P5a plan complete 2026-06-18** → Contracts C22–C24 frozen; Q11–Q20 all RESOLVED or RESOLVED-DEFERRED (Q14/Q17 resolved at freeze; Q20 deferred to packaging; D1 resolved). Remaining P5b open items (runner runtime, token issuance, implementer) surface at the P5b plan round.

### Q11 — Reconciliation with DEC-FBR-05's "no private pro-features branch" (was BLOCKING)
**Status**: RESOLVED 2026-06-18 → [`DEC-FBR-12`](DECISIONS.md#dec-fbr-12-open-core-boundary-for-the-agentic-loop-feedbackmonk-autopilot).

Resolution: DEC-FBR-05 relaxed (not sacred). The **build axis** (efficiency/quality/robustness) is separated from the **packaging axis** (open vs proprietary). Architecture DECIDED (work-order API seam); open-vs-proprietary packaging **DEFERRED** behind that seam and built as one coherent system. feedbackmonk stays fully AGPL today; no premature repo split. No longer blocking.

DEC-FBR-05 committed to "single codebase, fully AGPL; no private 'Pro features' branch; no feature gating beyond pricing-tier caps." The proprietary agentic loop appears to collide with that. **Proposed resolution** (DEC-FBR-12): the agents are a *separate commercial product* outside the AGPL repo, integrating only via the public work-order API; feedbackmonk itself stays 100% AGPL with no gated features, and self-hosters get a documented BYO-agent seam. → Confirm this framing, or choose a different reconciliation (e.g. accept a gated feature inside the product, or keep the whole loop open and monetize hosting only).

### Q12 — Analyst: proprietary, but with what reversal trigger?
**Status**: RESOLVED-DEFERRED 2026-06-18 → folds into DEC-FBR-12's deferred packaging axis. Nothing is committed proprietary now, so no reversal trigger is needed: the analyst is built as one coherent system, and whether any of it is later packaged proprietary is decided when external commercialization is on the table. Moot until then.

### Q13 — Runner runtime, distribution, licensing, billing
**Status**: RESOLVED (defaults) 2026-06-18; mechanics deferred to `/0-uldf-ldis-plan`. Cadence default CONFIRMED: analyst sweep **scheduled-by-default** + on-demand "review now"; implementer **approval-event-triggered only**, never automatic by default (autonomy dial FR-FBR-21 opts into more). The concrete runtime mechanism (cron / CI / GitHub Action / CLI) is a build-detail for the P5 plan round. Distribution/licensing/billing folds into the deferred packaging axis (DEC-FBR-12).

### Q14 — Agent authentication shape
**Status**: RESOLVED (seam) 2026-06-18 → frozen in [P5a plan](../../planning/plans/20260618T114524-feedbackmonk-p5a-agentic-resolution-loop.md) Contract C22. The runner authenticates with a **project-scoped, write-scoped runner token** minted from the *same per-project Ed25519 signing-key infrastructure* used for end-user JWTs (DEC-FBR-04) — a new credential **class**, not a separate crypto stack. Scope = runner-authored work-order transitions for one `project_id`; rotation/revocation mirror signing-key deactivation (`POST`/`DELETE /projects/:id/runner-tokens`). **P5a freezes the verification/authz seam** (runner-transition endpoints verify a token of this class, reject everything else, enforced by C22 invariant #2). **Token issuance + the runner that uses it = FR-FBR-24 (P5b).**

### Q15 — Work-order schema + approval state machine
**Status**: RESOLVED (proposed design) 2026-06-18 → SPECIFICATION.md § P5 Design detail. `work_orders` + append-only `work_order_events` ledger; state machine `draft→approved→dispatched→claimed→building→verifying→reported→completed` (+ `failed`/`cancelled`), modelled on FR-FBR-08's legal-transition + same-txn-audit pattern. Five hard invariants, anchored by "no state ≥ dispatched without an owner-authored approved event." Freezes as Contract C22 at plan time.

### Q16 — Data model for clusters, recommendations, work orders
**Status**: RESOLVED (proposed design) 2026-06-18 → SPECIFICATION.md § P5 Design detail. Six new tables (`feedback_clusters`, `recommendations`, `analysis_sweeps`, `work_orders`, `work_order_events`) + new `ActionType` enum + nullable `feedback.cluster_id` column. Reuses existing `FeedbackKind`. All tenant+project scoped via the repository layer. **Decision point D1 RESOLVED 2026-06-18**: `feedback.cluster_id` first-class nullable FK column (NOT a join table) — idiomatic per the `crash_event_id` precedent; a feedback belongs to ≤1 cluster, re-clustering is audited not historised. **FROZEN as Contract C23** in the [P5a plan](../../planning/plans/20260618T114524-feedbackmonk-p5a-agentic-resolution-loop.md).

### Q17 — "Tweak before approve" round-trip
**Status**: RESOLVED 2026-06-18 → frozen in [P5a plan](../../planning/plans/20260618T114524-feedbackmonk-p5a-agentic-resolution-loop.md) Contract C22. **Authoritative overrides** (not appended instructions): owner edits captured in `work_orders.owner_overrides jsonb`, merged over the recommendation's fields at dispatch with **overrides winning** (the recommendation is provenance; the work order is the order). The recommendation row is stamped `tweaked_approved`. Post-report, `request-changes` re-opens `reported → building` carrying a new `owner_overrides` delta in the event `detail`. Rationale: feedback-derived recommendations are untrusted; the owner's edit is the trust signal, so it must be authoritative, not advisory.

### Q18 — Prompt-injection threat model + adversarial fixtures
**Status**: RESOLVED (proposed design) 2026-06-18 → SPECIFICATION.md § P5 Design detail. Five-threat model (instruction injection, poisoned-cluster false consensus, exfiltration, destructive steering, approval-gate bypass) each paired with a specced defense; named adversarial corpus `tests/feedback_injection_corpus.rs` (cases a–h) mirroring the JWT fixture corpus + Q24 byte-for-byte discipline. Freezes as Contract C24 at plan time. Note: most weight lands in P5b (the implementer), but the data-envelope + poisoned-cluster defenses apply to P5a clustering too.

### Q19 — Phase shape + earliest buildable slice
**Status**: RESOLVED 2026-06-18. P5 splits into **P5a** (analyst FR-FBR-19/20 + review surface FR-FBR-21 + work-order API FR-FBR-22 — *recommend-only, no code execution*) then **P5b** (implementer FR-FBR-23 + runner FR-FBR-24). Sequencing, not descoping — everything still gets built. Robustness rationale: don't wire public input to code execution until the analyst is proven and FR-FBR-25 (untrusted-input safety) is built. Ordering feeds `/0-uldf-ldis-plan`.

### Q20 — BYO-agent contract for self-hosters
**Status**: RESOLVED-DEFERRED 2026-06-18 → [P5a plan](../../planning/plans/20260618T114524-feedbackmonk-p5a-agentic-resolution-loop.md) Deferred Decisions. Rides with the deferred packaging axis (DEC-FBR-12). P5a documents the work-order API as a seam (ops doc) but ships **no reference adapter** and makes **no open-vs-proprietary analyst call**; that decision arrives with the runner (FR-FBR-24) when external commercialization is on the table, behind the frozen API seam. The contract is the deliverable now; the BYO-agent reference + open-analyst question is moot until packaging is decided.
