# Execution Plan — feedbackmonk v1 Build Arc
**Source**: /0-uldf-ldis-plan
**Generated**: 2026-05-13T18:57:11Z
**Task**: Build feedbackmonk v1 — standalone open-source SaaS user-feedback platform, 18 FRs across 5 phases (P0-P4), AGPL-3.0-or-later, hosted at a new public GitHub repo. GitCellar's `gitcellar-cloud/src/feedback/` is the working reference, not a base to extract from.
**Strategy**: STAGED (phase-gated; intra-phase parallelization decided per phase at its own planning gate)
**Intake Source**: `docs/planning/intakes/20260512T221154-extract-feedback-system-as-standalone-product.md`
**Spec Source**: `docs/specs/feedbackr/` (SPECIFICATION.md, DECISIONS.md DEC-FBR-01..10, ARCHITECTURE.md, OPEN_QUESTIONS.md all RESOLVED)

---

## Strategy Rationale

### Why STAGED, not PARALLEL, not SEQUENTIAL

The 18 FRs split into 5 phases with hard dependency gates between them:

- **P0 (Foundation)** — multi-tenant schema, submission API, JWT verify, anonymous mode, health endpoint. Nothing in P1-P4 can ship without P0's data model and API surface.
- **P1 (Closes the Loop)** — admin UI, status workflow, status emails, PII scrubber. Requires P0's data; everything after assumes admins can triage.
- **P2 (Customer-Facing)** — widget, public roadmap, voting, promote. Requires P1 admin to triage submissions and curate roadmap items.
- **P3 (Commercial)** — tier enforcement, Polar billing. Requires P2 widget-shipping signal to justify paywalls.
- **P4 (Go-Public)** — marketing site, self-host docker. Requires P3 commercial gate for "buy" CTA + tier display.

PARALLEL across phases would race the data model against features that depend on it. SEQUENTIAL across all 18 FRs would miss the genuine intra-phase parallelism (P1's admin-UI / emails / PII scrub are independent post-schema; P2's widget / roadmap-backend / promote are independent).

**STAGED** = sequential between phases, parallel within phases where boundaries justify it. The plan finalizes phase **ordering and gates** now; intra-phase parallelization is re-evaluated at each phase's own planning gate (per FR mix and discovered shape).

### Collaboration Value Assessment (whole arc)

| Factor | Score (1-5) | Notes |
|---|---|---|
| **Specialization** | 4 | Distinct skill clusters per phase (DB/auth, UI/email, widget/frontend, billing, marketing). |
| **Quality** | 4 | Multi-tenant isolation, Q24 privacy invariant, billing semantics — each benefits from focused review. |
| **Discovery** | 3 | Reference implementation reduces unknowns, but widget UX and tier-enforcement plumbing are novel. |
| **Speed** | 4 | ~12 weeks FTE → calendar ~6 months given GitCellar overlap; parallelism inside phases meaningfully helps. |
| **Boundary Clarity** | 4 | Spec already partitions cleanly by phase; FR table is the contract. |
| **Coupling** | 3 | Intra-phase coupling is real (admin UI consumes status-workflow types, etc.) but resolvable via interface contracts at each phase plan. |

**Value total**: 15/20. **Friction total**: 7/10. **Net**: 11.5. → PARALLEL strongly recommended at phase scope. But because phases are gated, the macro is **STAGED with parallel branches inside each phase** — encoded here as STAGED + per-phase planning gates.

### Calendar reality

DEC-FBR-08 logs ~12 weeks FTE. DEC-FBR-10 ties feedbackmonk Stage 3 to GitCellar 1.0 ship. GitCellar is in pre-launch hardening (binding calendar constraint). Realistic calendar: **~6 months from this plan's adoption to public Stage 2 beta**, assuming GitCellar consumes ~50% of founder bandwidth through GitCellar 1.0. The plan does NOT attempt to compress this — it explicitly leaves room for context-switching cost.

---

## Context Budget Assessment

This plan is a **multi-session, multi-month arc**, not a single session's queue. Per-phase context budgets are evaluated at each phase's own `/0-uldf-ldis-plan` round, not consolidated here. Cross-phase carry-state is the spec + this plan + per-phase plan artifacts — all durable on disk in the feedbackmonk repo (post-migration, see § Pre-Implementation Migration Gate).

Anti-pattern explicitly avoided: trying to expand all 5 phases into one session's task queue. Plan documents the **arc**; execution sessions plan **the next phase only**.

---

## Pre-Implementation Migration Gate (BEFORE P0)

DEC-FBR-07 mandates spec migration to the feedbackmonk repo before any code begins. **This is the first concrete action** after this plan is adopted:

1. **Pre-registration tasks** (DEC-FBR-09, near-zero cost):
   - Check `github.com/feedbackr` org availability → register if free.
   - Check WHOIS on `feedbackr.com`, `feedbackr.app`, `feedbackr.dev` → register first available.
   - If both squatted: pause and reopen Q9 in spec session before P0.
2. **Create new repo** at `E:\Developer\SourceControlled\Apps\Feedbackr`:
   - `git init`, AGPL-3.0-or-later LICENSE, README.md, `.gitignore`.
   - First commit: empty skeleton.
   - Create `github.com/<org>/feedbackr` remote, push.
3. **Move spec artifacts**:
   - `docs/specs/feedbackr/{SPECIFICATION,DECISIONS,ARCHITECTURE,OPEN_QUESTIONS}.md` → `E:\...\feedbackmonk\docs\specs\`
   - `docs/planning/intakes/20260512T221154-extract-feedback-system-as-standalone-product.md` → `E:\...\feedbackmonk\docs\planning\intakes\`
   - This plan → `E:\...\feedbackmonk\docs\planning\plans\` (re-stamp filename in destination if desired)
4. **Leave breadcrumb in GitCellar**:
   - `docs/specs/feedbackr/README.md` (replacing the moved content) → one-line pointer to the new repo location.
   - Optional: leave intake artifact in place (it documents GitCellar's analysis of the extraction question; historical record).
5. **Run `/0-uldf-setup-project`** in the new feedbackmonk repo to install ULDF framework integration (`.claude/`, hooks, oracles directory).
6. **GitCellar's role flip** (DEC-FBR-07): no source-level changes to GitCellar's `gitcellar-cloud/src/feedback/` during P0-P3. It remains the working reference. Late in P2 / early P3, GitCellar adopts feedbackmonk's widget as customer #1 — a forward-looking integration, NOT an extraction of GitCellar's code.

**Gate condition for P0**: spec moved, repo created, framework installed, breadcrumb left.

---

## Oracle Pre-Build Plan

Four oracle candidates were surfaced in the spec session (SPECIFICATION.md § Oracles). All four are scheduled below; one is deferred to v1.1.

| Oracle | Question | Consumer(s) | Timing | Status |
|---|---|---|---|---|
| `multi-tenant-isolation-check` | Verify no cross-tenant data leakage in any query path (FR-FBR-01) | every P0+ worker writing or reading domain rows | **Task Zero of P0** (before any repository-layer code) | not yet built |
| `pii-scrub-audit` | Drift-detection over canonical PII pattern set (FR-FBR-10) | P1 PII scrubber worker; downstream P4 self-host docs | **Build during P1** (port the GitCellar oracle near-verbatim — pattern set + freshness contract are reusable) | not yet built (port from `gitcellar-service` / `gitcellar-cloud` oracle) |
| `widget-bundle-size` | Current widget JS+CSS bundle size; cap 30KB per FR-FBR-04 | P2 widget worker; CI gate | **Build at start of P2** before widget code lands (trigger-invalidate on build) | not yet built |
| `tier-enforcement-status` | Confirm each pricing-tier cap fires correctly (FR-FBR-14) | P3 tier-enforcement worker; ongoing CI gate | **Build during P3** alongside cap-check code | not yet built |

**Rationale**:
- `multi-tenant-isolation-check` is the highest-leverage oracle — DEC-FBR-03 makes raw SQL a security incident, and every P0+ worker writes through the repository layer. One oracle consulted by N workers across 6 months is the canonical inversion of redundant investigation cost.
- `pii-scrub-audit` is a high-fidelity port from GitCellar's existing oracle; almost zero new authoring cost since the pattern set is canonical. The Verification Oracle catches scrubber drift across SDK ports (Rust core + future JS widget side if applicable).
- `widget-bundle-size` defends FR-FBR-04's 30KB cap as a contract, not an aspiration. Build-time trigger-invalidate makes it cheap.
- `tier-enforcement-status` defends FR-FBR-14's caps and the Free-tier "powered by feedbackmonk" footer rule. Without it, tier drift is silent until a customer notices on their invoice.

**Deferrals** (evaluated but not scheduled):
- `feedback-extraction-coupling-points` (intake's candidate): **not built**. Premise was in-place extraction; DEC-FBR-07 flipped the calculus to "new repo, code-as-reference." No catalog of coupling points needed — feedbackmonk is greenfield with GitCellar as a read-only reference.
- `feedback-competitor-feature-matrix` (intake's candidate): **rejected as oracle**, kept as marketing artifact. Subjective + freshness-unfriendly → handled in P4 landing-site copy.
- `feedbackr-tier-quotas` project-state oracle (SPECIFICATION.md § Oracles): **deferred to v1.1**. Useful for admin dashboard ergonomics; not a v1 blocker.

---

## Phase Plan

Each phase below lists: (1) FRs in scope, (2) intra-phase parallelization recommendation, (3) gates / exit criteria, (4) carry-state for the next phase. **Per-phase `/0-uldf-ldis-plan` rounds finalize execution topology at each gate** (this arc-plan does not pre-commit to PODS/SEQUENTIAL inside each phase).

### P0 — Foundation (~2 weeks FTE)

**FRs**: FR-FBR-01 (multi-tenant data model), FR-FBR-02 (signup/onboarding), FR-FBR-03 (submission API), FR-FBR-05 (JWT EdDSA verify), FR-FBR-06 (anonymous mode), FR-FBR-18 (health/observability).

**Intra-phase ordering** (hard within P0):
1. **Task Zero**: build `multi-tenant-isolation-check` oracle (skeleton + freshness contract) so subsequent code is policed from commit 1.
2. **Sub-task 1**: data model + tenant-scoped repository layer (FR-FBR-01). Establishes the SOLE query path; raw SQL becomes a security incident from this commit forward.
3. **Sub-task 2** (parallel after 1): JWT EdDSA verify (FR-FBR-05) + anonymous mode (FR-FBR-06) + submission API (FR-FBR-03). All three depend on the data model + repository layer; can run as parallel branches.
4. **Sub-task 3** (parallel with 2): customer signup + onboarding (FR-FBR-02). Independent surface; depends on schema only.
5. **Sub-task 4** (after 1-3): health endpoint + structured logging (FR-FBR-18). Port from `gitcellar-cloud/src/main.rs` shape.

**Recommended execution topology** (re-confirm at P0 plan round): SEQUENTIAL for sub-task 1 (data model is the contract); PODS or sub-agent parallelization for sub-tasks 2 + 3 once contract is frozen.

**Exit gate (must hold before P1)**:
- Tenant signup → create first project → POST feedback works end-to-end (curl-able).
- `multi-tenant-isolation-check` oracle green across all query paths.
- `/health` returns structured JSON.
- Repository-layer-only query discipline enforced (lint/CI gate optional but recommended).

**Carry-state to P1**: data model in production-ready shape; repository layer's API surface frozen for admin-UI consumers; oracle catalogued in `.claude/oracles/INDEX.md`.

### P1 — Closes the Loop (~3 weeks FTE)

**FRs**: FR-FBR-07 (admin UI), FR-FBR-08 (status workflow + audit), FR-FBR-09 (status emails), FR-FBR-10 (PII scrubber).

**Intra-phase parallelization** (high opportunity):
- **Worker A**: status workflow state machine + audit history (FR-FBR-08). Port from `gitcellar-cloud/src/feedback/db.rs` patterns. Highest-leverage to do first — admin UI consumes it.
- **Worker B** (parallel with A's contract frozen): admin UI list + drawer + reply composer (FR-FBR-07). React, port from `gitcellar-cloud/admin-ui/`.
- **Worker C** (parallel): status emails (FR-FBR-09). Port from `email_templates.rs`. Parameterize tenant brand; subject becomes `[{tenant.email_subject_prefix} #FB-1234]`.
- **Worker D** (parallel, low coupling): PII scrubber (FR-FBR-10) + `pii-scrub-audit` oracle port (FR-FBR-10's drift-detection contract).

**Recommended execution topology** (re-confirm at P1 plan round): PODS (3-4 workers) — distinct surfaces, clean interface contracts, high specialization value. Each worker's scope fits comfortably in an agent's effective context.

**Interface contracts to author at P1 plan time**:
- Status transition function signature: `transition_status(feedback_id, to_status, reason_note?, duplicate_of?) -> Result<...>` — Worker A defines, Workers B + C consume.
- Email template tenant-brand parameters: subject prefix, footer signature, sender display name.
- PII scrubber call site: `scrub_event(event) -> event` — consumed by Worker C's email-body rendering path AND P4 self-host doc.

**Exit gate (must hold before P2)**:
- Admin can list feedback, click drawer, transition status (with audit row), reply public + internal.
- Submission → confirmation email → status-change email → public-reply email all observed via Mailpit.
- PII scrubber active on all server logs; oracle green.

**Carry-state to P2**: admin UI tenant-brand-parameterized; status workflow frozen for widget-side display strings.

### P2 — Customer-Facing (~3 weeks FTE)

**FRs**: FR-FBR-04 (widget), FR-FBR-11 (public roadmap), FR-FBR-12 (promote + Q24 invariant), FR-FBR-13 (voting + aggregator).

**Intra-phase parallelization**:
- **Worker A**: widget bundle (FR-FBR-04). Greenfield — most novel piece, no port reference (GitCellar had no widget). Heavy frontend work, generous UX iteration budget. Builds `widget-bundle-size` oracle as Task Zero of this worker.
- **Worker B** (parallel): roadmap data model + voting + aggregator (FR-FBR-11 + FR-FBR-13). Port `roadmap_voting.rs` algorithm; native PostgreSQL data store (drop Forge per DEC-FBR-06).
- **Worker C** (after B's contract frozen): promote-to-roadmap action (FR-FBR-12) + **Q24 privacy invariant** byte-for-byte unit test (highest-stakes correctness check; carry over from GitCellar's Phase 5 test).

**Recommended execution topology** (re-confirm at P2 plan round): PODS (3 workers). Widget is the long pole — budget more calendar time for UX iteration.

**Interface contracts to author at P2 plan time**:
- Widget submission API contract: exact JSON shape, JWT header handling, anonymous-mode cookie behavior.
- Widget tenant-config endpoint: how the widget fetches its branding/color/logo at runtime (cached, CSP-friendly).
- Roadmap top-voted endpoint shape: `GET /api/v1/projects/{project_id}/roadmap/top-voted?limit=N` — Worker B defines, public consumers freeze on it.

**Exit gate (must hold before P3)**:
- Widget < 30KB (oracle green), keyboard-accessible (WCAG AA spot-check), CSP-compatible (CSP probe page).
- Public roadmap renders anonymously; vote requires JWT or anon-cookie per project mode.
- Promote action transitions source feedback to `duplicate`; Q24 inline test asserts byte-for-byte.

**Carry-state to P3**: widget shipped → distribution channel exists for tier-enforcement footer; roadmap live → social proof / marketing material exists.

### P3 — Commercial (~2 weeks FTE)

**FRs**: FR-FBR-14 (tier enforcement), FR-FBR-15 (Polar billing).

**Intra-phase parallelization** (low):
- **Worker A**: tier enforcement (FR-FBR-14). Caps + "powered by feedbackmonk" widget footer + opt-out on paid tiers. Builds `tier-enforcement-status` oracle.
- **Worker B** (parallel until webhook-handler design point): Polar integration (FR-FBR-15). Port Polar setup pattern from `gitcellar-cloud/src/billing/`. Webhook handler couples to tier state → final integration step is sequential.

**Recommended execution topology** (re-confirm at P3 plan round): SEQUENTIAL or 2-worker PODS depending on how much Polar setup is already known territory. If recently done for GitCellar, mostly mechanical → sequential preferred.

**Interface contracts to author at P3 plan time**:
- Tier-cap predicate signature: `check_tier_quota(tenant_id, resource_type) -> Result<Quota>` — Worker A defines, every domain write path consumes.
- Polar webhook ↔ tier-state mapping: which Polar subscription events flip which tier flags.

**Exit gate (must hold before P4 — also DEC-FBR-10 Stage 1 dogfood-alpha trigger)**:
- Free vs Starter vs Pro vs Self-host caps all enforced; oracle green.
- Polar webhook → tier flip end-to-end on Polar sandbox.
- GitCellar (or other dogfood target) embeds feedbackmonk widget as customer #1; founder triages own feedback through it.

**Carry-state to P4**: tier matrix live → pricing page in P4 marketing site has real numbers; commercial gate works → "buy" CTA on landing is real, not aspirational.

### P4 — Go-Public (~2 weeks FTE)

**FRs**: FR-FBR-16 (marketing site/landing), FR-FBR-17 (self-host docker).

**Intra-phase parallelization** (clean split):
- **Worker A**: marketing site (FR-FBR-16). Astro build, hero + pricing + docs + Show HN copy. **Open-source per DEC-FBR-05** — landing/marketing/docs are AGPL, NOT a separate proprietary repo. Pattern from `gitcellar-landing/`. Real design work — budget generously per DEC-FBR-08 surfaced concern #3.
- **Worker B**: self-host distribution (FR-FBR-17). `docker compose up` deploys full stack, env-var config, migration runner, backup docs. Real production-readiness work (DEC-FBR-08 surfaced concern #4).

**Recommended execution topology** (re-confirm at P4 plan round): PODS (2 workers, independent surfaces).

**Brand pass at P4** (DEC-FBR-09): real branding pass — logo, color, font, voice — done jointly with landing. If a better name surfaces, rename here (costs are low pre-launch).

**Exit gate (DEC-FBR-10 Stage 2 trigger — public AGPL beta)**:
- `feedbackr.com` (or final-name.com) live with hero / pricing / docs / OSS repo link.
- `docker compose up && curl /health` works on a fresh VM per the self-host docs.
- Show HN post drafted; Twitter/X thread drafted; GitHub repo flipped public.

**Carry-state to Stage 3** (DEC-FBR-10): wait for GitCellar 1.0 ship before paid marketing.

---

## Testability Gate Findings

Per `claude-template/segments/-ldis/plan-phase4-testability-gate.md`. Five questions scored per high-risk FR.

### Flagged items

#### FR-FBR-01 — Multi-tenant data model with RLS-scoped repositories

| Q | Score | Reasoning |
|---|---|---|
| Q1 | Iteration cost | 2 (unit tests cheap once schema set) |
| Q2 | Fidelity risk | **5** (cross-tenant leakage is silent; a passing test does NOT prove isolation under all query paths — the canonical reward-hacking surface for a multi-tenant SaaS) |
| Q3 | Critical path | **5** (every other FR depends on this) |
| Q4 | Scaffolding leverage | yes — `multi-tenant-isolation-check` Verification Oracle materially lowers fidelity risk |
| Q5 | Drift detection | schema-hash + query-path enumeration; oracle re-runs on schema change |

**Composite 12+ AND Q2=5 → flagged.**

**Recommendation**: `multi-tenant-isolation-check` oracle is **mandatory before any data-write code lands**. Build it Task Zero of P0. The oracle's freshness contract is "rebuild whenever schema or repository layer changes" — trigger-invalidate. Drift detection is the schema-hash + repository-method enumeration: any new repository method that doesn't go through tenant-scoped helpers fails the oracle.

#### FR-FBR-15 — Polar billing integration

| Q | Score | Reasoning |
|---|---|---|
| Q1 | Iteration cost | **4** (Polar sandbox roundtrip per iteration; webhook delivery delays) |
| Q2 | Fidelity risk | **4** (webhook race conditions; sandbox-vs-prod environment isolation; replay semantics) |
| Q3 | Critical path | **4** (P3 ships when this works; calendar pressure to P4) |
| Q4 | Scaffolding leverage | yes — Polar webhook fixture replay + sandbox-as-CI environment cuts iteration cost ~3x |
| Q5 | Drift detection | sandbox ↔ prod schema parity check before any prod flip |

**Composite 12+ → flagged.**

**Recommendation**: Build Polar webhook fixture corpus (replayable payloads for `subscription.created`, `subscription.updated`, `subscription.cancelled`, edge cases) before main integration work. Mandatory drift detection: sandbox-vs-prod schema diff in CI; refuse deploy on mismatch. Reference GitCellar's existing Polar setup for patterns (`gitcellar-cloud/src/billing/`).

#### FR-FBR-04 — Embeddable widget (<30KB, themed, accessible)

| Q | Score | Reasoning |
|---|---|---|
| Q1 | Iteration cost | 3 (browser eval per iteration; bundling step) |
| Q2 | Fidelity risk | 3 (visual + a11y verification has known fidelity bounds with Playwright + axe-core) |
| Q3 | Critical path | **4** (P2 ships when this works; widget is the customer-facing artifact) |
| Q4 | Scaffolding leverage | yes — `widget-bundle-size` oracle + Playwright a11y harness halves iteration cycles |
| Q5 | Drift detection | size-budget oracle on every build; a11y harness on every PR |

**Composite 11 — borderline; flagged because Q3 + Q4 combination matches "highest plan-wide leverage" rule (Q1=3 close to 4 with novelty).**

**Recommendation**: Both `widget-bundle-size` oracle AND a Playwright + axe-core a11y harness must exist before main widget code lands. Per DEC-FBR-08 surfaced concern #2, widget is the most novel piece — UX iteration time budgeted generously.

### Items NOT flagged

FR-FBR-02 / 03 / 05 / 06 / 07 / 08 / 09 / 10 / 11 / 12 / 13 / 16 / 17 / 18: each scored composite < 12 with no Q2=5 spike. Most have direct ports from GitCellar's reference implementation (lowering Q1 + Q2) or clear external test surfaces (Mailpit for emails, curl for endpoints, snapshot tests for templates).

**FR-FBR-12 (Q24 privacy invariant)** deserves a note despite not being flagged: it's the highest-stakes single byte-for-byte test in P2. The invariant carries over from GitCellar's `roadmap_promote.rs` — port the inline unit test verbatim. The reason it's not flagged is because Q2 is low (the verification is byte-exact, near-zero fidelity gap) — but the work item is correctness-critical and the test must not be removed under any P2 refactor.

---

## Ripple Analysis

This is a **greenfield build in a NEW repository**. The plan does not modify any code in GitCellar's tree. Ripple analysis on GitCellar is empty by design.

**Forward-looking ripples** (out of scope for this plan, surfaced for future planning):

- **GitCellar embeds feedbackmonk widget as customer #1** (late P2 / early P3). One-shot integration. Impact: GitCellar's Desktop / Cloud surfaces gain a JWT-signing helper + widget embed; existing internal feedback module keeps running. No removal yet.
- **GitCellar migrates off internal feedback module** (post-feedbackmonk v1, separate effort). Impact: export historical feedback from GitCellar's `feedback` tables → admin-import into feedbackmonk → retire `gitcellar-cloud/src/feedback/`. Requires its own plan; out of scope here.

**License ripple**: AGPL-3.0-or-later is **not** a ripple into GitCellar (GitCellar is not AGPL). The widget GitCellar embeds is consumed via HTTP API + script tag — no source-level dependency, no license contamination. DEC-FBR-07 explicitly de-couples the two repos for this reason.

---

## Interface Contracts (Cross-Phase)

Contracts that span phases must be frozen BEFORE the phase that produces them ends:

| Contract | Producer | Consumers | Freeze at end of |
|---|---|---|---|
| Multi-tenant repository API surface | P0 sub-task 1 | every P1+ data-touching surface | P0 |
| Status workflow state machine + transition fn signature | P1 Worker A | P1 Workers B/C, P2 Worker C (promote) | P1 |
| Email template tenant-brand parameters | P1 Worker C | P3 (tier display in footer), P4 (landing-page email samples) | P1 |
| PII scrubber function signature + canonical pattern set | P1 Worker D | P4 self-host docs | P1 |
| Widget submission API JSON shape | P2 Worker A + backend P0 owners | external customers (P4 docs) | P2 |
| Roadmap top-voted endpoint shape | P2 Worker B | public consumers (P4 landing-page top-voted live embed) | P2 |
| Tier-cap predicate signature | P3 Worker A | every domain write path (retroactively wired) | P3 |
| Polar webhook ↔ tier-state mapping | P3 Worker B | P4 self-host docs (env-var setup) | P3 |

Each per-phase plan round MUST author the contracts column in detail (exact signatures, exact JSON schemas) before parallel workers fan out within that phase.

---

## Deferred Decisions

| Decision | Deferred Until | Default if Unresolved | Why Defer |
|---|---|---|---|
| Intra-phase execution topology (PODS vs SEQUENTIAL within each phase) | Per-phase `/0-uldf-ldis-plan` rounds | PODS for P1/P2/P4; SEQUENTIAL or 2-worker for P0/P3 | Each phase's FR mix and discovered shape may change the right call |
| Self-host orchestrator choice (Docker Compose only? Add Helm? K8s manifest?) | P4 planning round | Docker Compose only per FR-FBR-17 | Helm/K8s expand audience but add maintenance burden — let demand drive |
| Custom-domain feature for $29+ tier (FR-FBR-14 footnote) | Post-Stage 2 | Not in v1 (deferred per DEC-FBR-08 OUT list) | Adds DNS/cert ops; not blocking for launch |
| Email digest cadences (port from GitCellar's `digest_worker`) | Post-v1 | Out (DEC-FBR-08 OUT list) | Net new feature beyond v1 MVP |
| Final product name | P4 brand pass (DEC-FBR-09) | "feedbackmonk" | Brand work needs the product to feel real first |
| Real pricing levels (vs current $9/$29/$79 placeholders) | Stage 2 beta market signal | Current placeholders per DEC-FBR-03 | Beta pricing strategy needs real customer conversations |

---

## Risks and Mitigations

| Risk | Likelihood | Severity | Mitigation |
|---|---|---|---|
| Calendar slips beyond ~6 months because GitCellar 1.0 ship slips | High | Medium | Stages 1-2 of DEC-FBR-10 can overlap GitCellar hardening; only Stage 3 marketed launch waits on GitCellar 1.0. P0-P2 are not blocked. |
| Multi-tenant isolation has silent leak | Low (after oracle) | **Critical** | `multi-tenant-isolation-check` oracle as Task Zero of P0. Repository-layer-only query discipline enforced. Raw SQL is a security incident. |
| Widget UX requires more iteration than budgeted | High | Medium | Generous UX budget per DEC-FBR-08 concern #2. Widget is the long pole — if P2 slips 1-2 weeks, accept it; do not compress widget polish. |
| Polar billing webhook edge cases | Medium | High | Pre-built webhook fixture replay corpus; sandbox-vs-prod schema diff in CI. Reference GitCellar's existing Polar integration. |
| Q24 privacy invariant regression in a future P2 refactor | Low | **High** | Byte-for-byte inline unit test ported from GitCellar's `roadmap_promote.rs` — same test name, same assertions. Document as untouchable in module README. |
| AGPL license discourages potential commercial adopters | Low-Medium | Low-Medium | DEC-FBR-05 reverse stress test affirms AGPL is right for this persona. Plausible's revenue data supports the call. If post-Stage-2 signal contradicts, revisit at Stage 3 planning. |
| Marketing site quality bar (Plausible-level) underestimated | Medium | Medium | DEC-FBR-08 surfaced concern #3 logs this. P4 plan round must budget real design work, not "polish the Astro template." |
| Sibling parallel GitCellar sessions (forge-version-tiers + gitea-1.26-cleanup) collide with this work | None | n/a | This plan does NOT modify any GitCellar code. No collision possible. |
| Stale GitCellar LTADS session bleeds into feedbackmonk work | Low | Low | Resolve at adoption: either `/0-uldf-ltads-stop` GitCellar's session or run feedbackmonk work in a fresh successor session in the new repo (the natural shape post-migration gate). |

---

## Execution Commands

The recommended next-step path:

1. **Now**: review this plan with user; accept or revise.
2. **At adoption**: execute the **Pre-Implementation Migration Gate** above:
   - Pre-register `github.com/feedbackr` + domain.
   - `git init` the new feedbackmonk repo, push AGPL skeleton.
   - Move spec + intake + this plan artifacts to `E:\...\feedbackmonk\docs\`.
   - Leave breadcrumb in GitCellar at `docs/specs/feedbackr/README.md`.
   - Run `/0-uldf-setup-project` in the new repo.
3. **In the new repo**: run `/0-uldf-ldis-plan "feedbackmonk P0 — Foundation"` to author the P0-specific plan with finalized intra-phase topology and interface contracts. Then `/0-uldf-proceed` to execution.
4. **Recurring per phase**: at each phase boundary (P0 done → before P1, etc.), repeat the per-phase plan round to finalize the next phase's topology and contracts before fanning out workers.

---

## Notes for Downstream Consumers

- **`/0-uldf-pods-parallelize` / `/0-uldf-pods-spawn-collaborator`**: do NOT consume this arc-plan directly. Each phase will produce its own plan artifact in the feedbackmonk repo's `docs/planning/plans/`; those are the per-PODS-round inputs.
- **`/0-uldf-ltads-start`**: this arc is multi-session, multi-month. LTADS state lives in the feedbackmonk repo (after migration), not here. Do not start an LTADS session against THIS repo's working dir for feedbackmonk work.
- **`/0-uldf-proceed` (now, in GitCellar's working dir)**: legitimate next step is to **dispatch / handoff** the migration gate work. The autonomy is `collaborative` — present this plan and offer `/0-uldf-proceed` for the migration gate.
