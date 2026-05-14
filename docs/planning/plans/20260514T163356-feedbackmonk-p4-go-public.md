# Execution Plan — feedbackmonk P4 — Go-Public (self-host docker + marketing site)
**Source**: /0-uldf-ldis-plan
**Generated**: 2026-05-14T16:33:56Z
**Task**: feedbackmonk P4 — Go-Public (self-host docker + marketing site)
**Strategy**: STAGED (2 stages: Stage 1 = brand pass + interface-contract freeze, single worker / HERE; Stage 2 = PODS-2 parallel marketing-site + self-host-docker workers; Stage 3 = convergence + e2e witness)
**Intake Source**: `docs/planning/intakes/20260512T221154-extract-feedback-system-as-standalone-product.md` (the arc-level intake; no P4-specific intake artifact exists — P4 scope is fully derived from arc-plan §P4 + FR-FBR-16 + FR-FBR-17 + DEC-FBR-08/09/10)
**Arc plan**: `docs/planning/plans/20260513T185711-feedbackmonk-v1-build-arc.md` §P4

---

## Strategy Rationale

### Why STAGED (not pure PARALLEL, not SEQUENTIAL)

P4 ships two FRs:

- **FR-FBR-16** — Astro marketing site (`feedbackmonk.com`): hero + pricing + docs + Show-HN-ready copy + brand pass (logo / color / font / voice per DEC-FBR-09). AGPL per DEC-FBR-05.
- **FR-FBR-17** — Self-host `docker compose up`: full-stack deployment, env-var config, migration runner, backup docs.

The two surfaces are genuinely independent — different languages (TypeScript/Astro vs YAML/Dockerfile), different files, zero source overlap, separate top-level directories. PARALLEL is the natural fit at the **implementation step**.

But the two surfaces share three small interface points that ARE coupled and benefit from being frozen ONCE before fan-out:

1. **Brand kit** (logo concept / color palette / font / voice guidelines per DEC-FBR-09) — Worker A's primary canvas; Worker B references the brand name in `docker-compose.yml` comments + ops docs.
2. **Canonical env-var schema** for self-host (the list of `FEEDBACKMONK_*` env vars + their semantics) — Worker B's authoritative output; Worker A consumes it to render the `/docs/self-host` page.
3. **Pricing strings** — must be byte-identical between the marketing site's `/pricing` page and `feedbackmonk-core::tier::tier_quotas()` (Contract C19, already frozen). Single-source-of-truth question — Stage 1 picks the mechanism (build-time Rust→JSON export vs hand-typed copy + parity oracle).

Resolving those three contracts inline at Stage 2 would force Worker A and Worker B to negotiate via channels mid-flight — exactly the coordination overhead PODS exists to avoid. Resolving them in **Stage 1 (HERE, single worker, ~30-60 min of decisions + scaffold)** lets Stage 2 fan out cleanly.

This mirrors the **P3 precedent** (Stage 1 backend chokepoint frozen, Stage 2 admin UI consumes) and the **P0 precedent** (data model frozen as the contract before parallel feature workers).

### Why not PARALLEL with embedded Task Zero per worker

Two reasons:

- **Brand pass naturally happens once**: logo + color + font + voice are decisions that produce a kit, not per-worker reinventions. Forcing Worker B to also reason about brand voice (just to have docker-compose comments be on-brand) is waste.
- **Pricing parity is a code↔site invariant** that needs a single chosen mechanism. Two workers independently deciding "I'll just hardcode it" produces two unaligned strings.

### Why not SEQUENTIAL across all of P4

Worker A's site content (~100-150k tokens of pages + components + copy) and Worker B's docker stack (~100k tokens of Dockerfile + compose + migration runner + ops docs) are large enough to genuinely benefit from fresh contexts + focused attention. Sequential would also miss the calendar-time savings PODS provides.

### Collaboration Value Assessment

| Factor | Score (1-5) | Notes |
|---|---|---|
| **Specialization** | 4 | Frontend/Astro/copy vs DevOps/Dockerfile/postgres are distinct skill clusters |
| **Quality** | 4 | Marketing-site polish + docker-compose-on-fresh-VM both benefit from focused attention |
| **Discovery** | 3 | Astro patterns and docker-compose patterns are well-trodden; some novelty in brand pass + clean-state smoke harness |
| **Speed** | 4 | Parallel saves ~50% calendar time on the two ~1-week FTE workstreams |
| **Boundary Clarity** | 5 | Two top-level directories (`marketing/` + `deploy/docker/`); zero file overlap |
| **Coupling** | 4 | Three coupling points (brand, env vars, pricing) are tiny and resolved upfront in Stage 1 |

**Value total**: 15/20. **Friction total**: 9/10. **Net**: 10.5. → **PARALLEL strongly recommended at Stage 2**; Stage 1's role is to make Stage 2's parallelism friction-free.

---

## Context Budget Assessment

| Stage / Worker | Estimated budget | Capacity check |
|---|---|---|
| Stage 1 (HERE single-worker) | ~60k tokens: read arc-plan §P4 + DEC-FBR-09 + tier_quotas() + decide brand kit + scaffold Astro & docker directories + write env-var schema + write handoff brief | ✅ Comfortable; fits within current session's remaining budget (already partway through this planning session — may handoff to fresh successor anyway per `/0-uldf-proceed` Phase 3) |
| Stage 2 Worker A (marketing site) | ~150k tokens: scaffold + pages (hero, pricing, features, docs, blog/show-hn-draft) + components + brand assets + Playwright smoke + axe-core a11y check | ✅ Comfortable in fresh 1M-context session |
| Stage 2 Worker B (self-host docker) | ~100k tokens: Dockerfile(s) for `feedbackmonk-api` + admin-ui static-asset serving + docker-compose.yml + migrations runner + Mailpit dev profile + `docs/operations/SELFHOST.md` + clean-state smoke harness | ✅ Comfortable in fresh 1M-context session |
| Stage 3 (convergence) | ~60k tokens: run smoke harness, build site, verify cross-links, assemble single P4-close commit | ✅ Comfortable |

All workers within 85% of effective capacity. No further decomposition required.

**Sibling-summary budget**: each Stage 2 worker needs to know the brand kit + env-var schema + pricing strings (~3-5k tokens total of frozen contracts from Stage 1). Trivial.

---

## Oracle Pre-Build Plan

| Oracle | Question | Consumer(s) | Timing | Status |
|---|---|---|---|---|
| `marketing-pricing-parity` (NEW, OPTIONAL) | Site pricing strings byte-match `feedbackmonk-core::tier::tier_quotas()` | Stage 2 Worker A; ongoing CI | **Stage 1 decides build-vs-defer** (see Decision-1 below); if built, Worker A authors as Task Zero | candidate (decision deferred to Stage 1) |
| `selfhost-compose-smoke` (NEW, OPTIONAL) | `docker compose down -v && docker compose up && curl /health` works on clean state | Stage 2 Worker B; CI gate post-launch | **Stage 1 decides build-vs-defer** (see Decision-2 below); if built, Worker B authors as Task Zero | candidate (decision deferred to Stage 1) |

**Existing four oracles** (`multi-tenant-isolation-check`, `pii-scrub-audit`, `widget-bundle-size`, `tier-enforcement-status`) continue to defend code-side invariants. P4 does not modify any code those oracles cover (no schema changes, no widget changes, no handler changes, no tracing-subscriber changes), so they should remain GREEN throughout.

**Rationale for the two candidates being decisions-not-mandates**:

- **`marketing-pricing-parity`** — Justified if Worker A hardcodes pricing strings in MDX/Astro pages (then drift is possible). If Worker A instead builds with a Rust→JSON export step (`cargo run --bin export-tier-quotas > marketing/src/data/tier_quotas.json`), the oracle is structurally redundant — Astro can't import stale data because the JSON IS the export. **Stage 1 decision**: prefer the build-step approach (`marketing/scripts/export-tier-quotas.{sh,ps1}` runs `cargo run -p feedbackmonk-marketing-export` or similar); if too heavy, fall back to hand-typed pricing + parity oracle.

- **`selfhost-compose-smoke`** — Defends FR-FBR-17 exit gate ("docker compose up works on a fresh VM") as a code-level invariant. Real value if the docker stack will be regularly modified (env-var schema evolution, postgres version bumps, etc.). Build cost: ~150 LOC PowerShell/bash wrapper + a known-good `curl /health` assertion. Iteration cost ~3-5 minutes per full smoke. **Stage 1 decision**: build this oracle — Q1 (iteration cost) for FR-FBR-17 is already 4 in the Testability Gate; halving it is high-leverage.

**Deferrals** (evaluated but not scheduled):

- `marketing-site-link-integrity` — Astro's build step naturally catches broken internal links; external links are out-of-scope for a static-site oracle. Deferred.
- `docker-image-registry-pull` — Tests that pulling the published feedbackmonk image from a registry works. Cannot run until PF-REGISTER-01 clears + first image is pushed to GHCR. Deferred to post-Stage-2 (or v1.1).

---

## Stage Plan

### Stage 1 — Brand pass + interface-contract freeze (HERE single worker, ~60k tokens)

**Topology**: HERE (in-session or HANDOFF to fresh successor — `/0-uldf-proceed` Phase 3 decides based on remaining context budget; given this planning session has consumed ~50-60k tokens already, HANDOFF is mildly preferred).

**Scope**:

1. **Brand pass decisions** (per DEC-FBR-09):
   - Logo concept: text-based wordmark? Mark + wordmark? Color decisions. (NB: visual brand work historically takes founders longer than expected — Stage 1 produces a "good-enough v1 brand kit" that can iterate post-launch. Don't perfection-trap here.)
   - Color palette: primary + accent + neutral. Hex codes. Recorded in `docs/brand/BRAND.md`.
   - Font choice: heading + body. (Recommend Inter + JetBrains Mono — both libre, both Plausible-tier-of-polish, both Google-Fonts-hosted-or-self-hostable.)
   - Voice & tone guidelines: 4-6 bullet points (e.g., "direct, not breathless"; "show, don't sell"; "privacy claims are evidenced, not aspirational"). Establishes Show HN post tone.
   - Output: `docs/brand/BRAND.md` (or `marketing/BRAND.md` if marketing/ is scaffolded in this stage).

2. **Path decisions** (where things live):
   - **Marketing site root**: `marketing/` (top-level, alongside `admin-ui/` and `widget/`). Mirrors gitcellar-landing convention.
   - **Self-host docker root**: `deploy/docker/` (containing `docker-compose.yml`, Dockerfiles, migration runner script, backup-and-restore scripts).
   - **Operations docs**: `docs/operations/SELFHOST.md` (the human-facing run-book that the marketing site `/docs/self-host` links to / iframes / mirrors).

3. **Canonical env-var schema** for self-host (`docs/operations/SELFHOST_ENV.md`):
   - Audit existing env vars across the codebase: scan `crates/feedbackmonk-*/` + `migrations/` + `admin-ui/` for `FEEDBACKMONK_*` / `DATABASE_URL` / SMTP-related vars / signing-key vars / port vars / etc.
   - Produce the canonical list with: name, required vs optional, default, semantics, example value, security-sensitivity flag.
   - This becomes Worker B's input AND Worker A's `/docs/self-host` source-of-truth.

4. **Pricing single-source-of-truth mechanism decision** (the Decision-1 above):
   - Option A (recommended): build-time Rust→JSON export. Add a small binary `crates/feedbackmonk-marketing-export/` (or a `feedbackmonk-core` example binary) that prints `tier_quotas()` as JSON. Worker A's Astro build runs it as a prebuild step. Pricing drift becomes structurally impossible.
   - Option B (fallback): hand-typed pricing + `marketing-pricing-parity` Verification Oracle. Build oracle as Worker A Task Zero.
   - Stage 1 picks A or B and records the choice as DEC-FBR-IMPL-* in DECISIONS.md.

5. **Smoke-harness decision** (Decision-2 above):
   - Build `selfhost-compose-smoke` Verification Oracle (recommended).
   - Define its three probes:
     - Probe A: `deploy/docker/docker-compose.yml` exists and parses (yaml-lint).
     - Probe B: All env vars referenced in compose are documented in `docs/operations/SELFHOST_ENV.md` (cross-reference check).
     - Probe C (`--full`): `docker compose down -v && docker compose up -d && wait-for-healthy && curl /health` — clean-state smoke.

6. **Marketing-site URL routing scheme** (skeleton):
   - `/` — hero + tagline + three trust signals (per DEC-FBR-02 landing-page hero structure)
   - `/pricing` — Free / Starter $9 / Pro $29 / Self-host $79 tier table (sourced from tier_quotas())
   - `/docs/` — index
   - `/docs/widget` — embed instructions
   - `/docs/self-host` — runbook (sourced from `docs/operations/SELFHOST.md`)
   - `/docs/api` — public API reference (link to OpenAPI/Swagger if any, or hand-written)
   - `/blog/show-hn-draft` — Show HN post draft (private/unlisted until launch)
   - `/changelog` (optional v1.1 deferral)

7. **Scaffold Astro & docker directories** (optional in Stage 1 — could defer first-file-creation to Stage 2 workers; recommend scaffolding to keep Stage 2 worker prompts minimal):
   - `cd marketing && npm create astro@latest -- --template minimal --typescript strict --no-git --no-install` (then `npm install` separately).
   - `mkdir -p deploy/docker && touch deploy/docker/{docker-compose.yml,Dockerfile.api,Dockerfile.admin-ui,migrate.sh,backup.sh,README.md}`.

8. **Write Stage 2 handoff brief** to `.claude/handoff/handoff-<ts>-p4-stage2-fanout.md`:
   - Reference Stage 1's frozen contracts.
   - State PF-REGISTER-01 and PF-RENAME-02 gates (commits use `--skip-push` until cleared).
   - Worker A and Worker B can be spawned in parallel as soon as Stage 1 lands.

**Stage 1 exit gate**:

- `docs/brand/BRAND.md` exists with logo concept + colors + fonts + voice.
- `docs/operations/SELFHOST_ENV.md` exists with audited env-var inventory.
- Decision-1 (pricing SSOT) and Decision-2 (smoke oracle) recorded in `docs/specs/DECISIONS.md` as `DEC-FBR-IMPL-05` and `DEC-FBR-IMPL-06` respectively (or next available IMPL number).
- `marketing/` and `deploy/docker/` directories created (empty-ish).
- Stage 2 handoff brief drafted.
- All four existing Verification Oracles GREEN.

**Carry-state to Stage 2**: the four contracts (C20/C21/C22/C23 — see Interface Contracts table below), the two recorded decisions, the scaffolded directories, the handoff brief.

---

### Stage 2 — Parallel fan-out: marketing site + self-host docker (PODS-2)

**Topology**: PODS with 2 workers. Each worker gets fresh ~1M context. Communication via shared files (`channels/`, `status.md`, `touches.json` per PODS convention). Lead-developer-coordination overhead minimal since the surfaces don't overlap.

**Worker A — Marketing site (FR-FBR-16)**

- **Scope**: Astro project under `marketing/`. Hero + pricing + docs + Show HN draft. Brand assets per Stage 1 BRAND.md. Open-source per DEC-FBR-05 (no separate proprietary repo).
- **Task Zero**: either build `marketing-pricing-parity` oracle (if Stage 1 chose Option B) OR wire up the prebuild Rust→JSON export step (if Stage 1 chose Option A).
- **Deliverables**:
  - Astro pages per Stage 1 routing scheme.
  - Hero copy + three trust signals ("EU + US hosting" / "Open-source core" / "Zero third-party trackers" — per DEC-FBR-02).
  - `/pricing` page reading from `tier_quotas()` SSOT.
  - `/docs/self-host` page mirroring `docs/operations/SELFHOST.md` (or importing it as MDX).
  - `/docs/widget` page documenting the FR-FBR-04 embed contract.
  - Show HN post draft at `/blog/show-hn-draft` (unlisted/draft-status; deployed but not linked from nav).
  - Playwright smoke + axe-core a11y sweep (mirror P3's pattern from `admin-ui/tests/`).
  - README.md for the `marketing/` module per ULADP.
- **Exit gate for Worker A**:
  - `cd marketing && npm run build` succeeds with zero errors / warnings.
  - Playwright + axe-core smoke PASS, zero WCAG violations.
  - Pricing strings parity verified (oracle GREEN, or build-step produces identical JSON to `tier_quotas()`).
  - Cross-links to `/docs/self-host` resolve.
  - WORKER-A status flipped CONVERGENCE-READY in PODS channels.

**Worker B — Self-host docker (FR-FBR-17)**

- **Scope**: `deploy/docker/` directory containing the full self-host stack. Plus `docs/operations/SELFHOST.md` runbook.
- **Task Zero**: build `selfhost-compose-smoke` Verification Oracle per Stage 1's Decision-2.
- **Deliverables**:
  - `deploy/docker/docker-compose.yml` — services: `postgres` (5433 host-port or internal-only, configurable), `api` (`feedbackmonk-api`, port 14304 default), `admin-ui` (static-served via the api binary OR a separate nginx — Stage 1 picks; recommend single-binary serving for simplicity), `mailpit` (optional dev profile).
  - `deploy/docker/Dockerfile.api` — multi-stage Rust build (cargo chef pattern for layer cache); minimal final image (distroless or slim-debian).
  - `deploy/docker/Dockerfile.admin-ui` — if separate; otherwise admin-ui assets baked into api image at build time.
  - `deploy/docker/migrate.sh` — runs `sqlx migrate run` against the configured DATABASE_URL; idempotent; runs on `up` via init-container or compose `depends_on: healthy`.
  - `deploy/docker/backup.sh` + `restore.sh` — pg_dump-based runbook scripts, documented in SELFHOST.md.
  - `docs/operations/SELFHOST.md` — human-facing runbook: prerequisites, `docker compose up` quickstart, env-var reference (links to SELFHOST_ENV.md), backup-and-restore, troubleshooting, upgrade procedure.
  - Smoke oracle's Probe C runnable: `docker compose down -v && docker compose up -d && wait-for-healthy && curl /health` returns 200 with JSON.
- **Exit gate for Worker B**:
  - `selfhost-compose-smoke` oracle Probe A + B PASS; Probe C `--full` PASS on the development machine.
  - `docs/operations/SELFHOST.md` cross-references SELFHOST_ENV.md; runbook is followable by someone unfamiliar with the codebase.
  - README.md for the `deploy/docker/` module per ULADP.
  - WORKER-B status flipped CONVERGENCE-READY in PODS channels.

**Stage 2 coordination notes**:

- **Same-branch by default** (per PODS-design): both workers commit-along on `main`. Touched files are disjoint by directory (`marketing/` vs `deploy/docker/` + `docs/operations/SELFHOST.md`), so 3-tier conflict detection should be quiet.
- **Shared edit point**: `docs/specs/SPECIFICATION.md` (status flip on FR-FBR-16 / FR-FBR-17 from PROPOSED → DONE) — handle at convergence, not by either worker mid-flight.
- **Shared edit point**: `docs/specs/DECISIONS.md` (record convergence-time DEC-FBR-IMPL-* additions if any) — handle at convergence.
- **Shared edit point**: `.claude/oracles/INDEX.md` (register new oracles if any built) — each worker adds its own oracle's row; conflict resolved at convergence (append-only file).

---

### Stage 3 — Convergence + integration witness (~60k tokens)

**Topology**: returns-to-lead-developer (the orchestrator that spawned Stage 2). Single-session.

**Convergence work**:

1. Verify all four existing Verification Oracles GREEN on the merged tree.
2. Verify new oracles (if any built) GREEN.
3. Run `cd marketing && npm run build` — site builds.
4. Run `selfhost-compose-smoke --full` — full clean-state smoke PASS.
5. Cross-link integrity: navigate from `/docs/self-host` on the built site to the env-var reference; confirm the path resolves.
6. Run `/0-uldf-finalize` to:
   - Reconcile spec: FR-FBR-16 + FR-FBR-17 → DONE.
   - Record any convergence-time decisions.
   - Update PROJECT_TRAJECTORY.md.
   - Stage and commit (single P4-close commit) with `--skip-push` per handoff-brief constraint (PF-REGISTER-01 not cleared).
7. Update CLAUDE.md "Pending Follow-Ups" section if any new items surfaced.
8. CSI-06 arc-terminus reconciliation: P4 is the final phase of the v1 arc, so this commit should carry `--complete-arc` flag (or the three-signal detection picks it up automatically via final-stage detection).

**Stage 3 exit gate (= DEC-FBR-10 Stage-2-trigger pre-conditions; public push remains GATED on PF-REGISTER-01)**:

- ✅ FR-FBR-16 and FR-FBR-17 status = DONE in SPECIFICATION.md.
- ✅ All Verification Oracles GREEN.
- ✅ Marketing site builds cleanly; Playwright + axe-core a11y PASS.
- ✅ `docker compose up` works on clean-state smoke.
- ✅ Show HN post draft committed (not yet posted).
- ✅ Single P4-close commit landed on `main` (with `--skip-push`).
- ⏸ **Public push BLOCKED** until PF-REGISTER-01 clears (github.com/feedbackmonk org registered + feedbackmonk.com purchased).
- ⏸ **Stage 3 (DEC-FBR-10 marketed launch) BLOCKED** until GitCellar 1.0 ships (per DEC-FBR-10 calendar gate).

**Carry-state to post-P4**:

- The v1 arc is **CONTENT-COMPLETE** at this commit. What remains is purely user action (PF-REGISTER-01, PF-RENAME-02) plus the post-launch Stage 3 marketing-motion calendar gate (GitCellar 1.0).
- The Pending Follow-Ups section in CLAUDE.md should grow to include: "PF-LAUNCH-01: Execute Stage 2 launch sequence (push public, Show HN post, Twitter thread) — gated on PF-REGISTER-01 clearing."

---

## Testability Gate Findings

Per `claude-template/segments/-ldis/plan-phase4-testability-gate.md`. Two FRs scored.

### FR-FBR-17 — Self-host distribution (`docker compose up`)

| Q | Score | Reasoning |
|---|---|---|
| Q1 | Iteration cost | **4** (docker rebuild + compose-up + wait-for-healthy + curl per cycle = 3-5 minutes) |
| Q2 | Fidelity risk | **4** (works-on-my-machine vs works-on-fresh-VM is the canonical gap; local-rebuild caches mask "needs registry pull" bugs; volume leftovers mask "needs init migration" bugs) |
| Q3 | Critical path | 4 (P4 exit gate) |
| Q4 | Scaffolding leverage | yes — `selfhost-compose-smoke` Verification Oracle with `down -v && up && curl` halves iteration cost AND inverts fidelity-risk direction (cleaner state than dev's stale-volume reality) |
| Q5 | Drift detection | clean-state smoke catches stale-images, leftover volumes, missing migrations, env-var doc drift (Probe B) |

**Composite ~14 → flagged.**

**Recommendation**: `selfhost-compose-smoke` oracle is **mandatory before Worker B's main implementation work**. Build as Task Zero. Probe A (yaml-lint) + Probe B (env-var doc cross-reference) run on every change; Probe C (`--full`) runs at Stage 3 convergence and post-launch CI. See Decision-2 above.

### FR-FBR-16 — Marketing site / landing

| Q | Score | Reasoning |
|---|---|---|
| Q1 | Iteration cost | 2 (Astro local preview fast) |
| Q2 | Fidelity risk | 2 (visual + content reviewed by human; the dimensions that auto-verify well — link integrity, a11y, bundle size — Astro+Playwright+axe cover) |
| Q3 | Critical path | 4 (P4 exit gate) |
| Q4 | Scaffolding leverage | yes — pricing-parity scaffolding (Option A build-step or Option B oracle) closes the one drift surface |
| Q5 | Drift detection | pricing-parity covers code↔site drift; Astro build covers internal-link integrity; axe covers a11y; no scaffolding planned for marketing-copy quality (human review only) |

**Composite ~11 — borderline; flagged because Q3=4 makes pricing-parity scaffolding high-leverage even at borderline composite.**

**Recommendation**: pick **Option A (Rust→JSON export build-step) over Option B (parity oracle)** — Option A makes drift **structurally impossible**, which is strictly better than Option B's "drift detected after the fact." See Decision-1 above. If Option A proves too heavy at Stage 1 (e.g., adds 30s to Astro build, or creates a cross-language CI hassle), fall back to Option B.

---

## Ripple Analysis

P4 is **NET-ADDITIVE**: two new top-level directories (`marketing/`, `deploy/docker/`) + new docs (`docs/brand/BRAND.md`, `docs/operations/SELFHOST.md`, `docs/operations/SELFHOST_ENV.md`) + possibly two new oracles (`marketing-pricing-parity`, `selfhost-compose-smoke`). **No existing API contract is modified.** **No existing handler is modified.** **No schema is modified.**

### Step 1 — Modified Interfaces

None. P4 adds; it does not modify.

### Step 2-5 — Consumer impact

| Domain | Touched? | Notes |
|---|---|---|
| Code consumers of `feedbackmonk-*` crates | No | No crate-level API changes. Possibly a NEW small `feedbackmonk-marketing-export` binary if Decision-1 picks Option A; this is additive. |
| Re-export chain | No | No re-exports change. |
| Documentation cascade | Yes (additive) | `docs/operations/SELFHOST.md` is new; `docs/operations/SELFHOST_ENV.md` is new; `docs/brand/BRAND.md` is new. README.md per new module per ULADP. CLAUDE.md updated to surface new directories. PROJECT_TRAJECTORY.md updated by `/0-uldf-finalize`. |
| Test impact | Yes (additive) | New Playwright suite under `marketing/tests/`; new smoke harness under `.claude/oracles/selfhost-compose-smoke/` (if built). Existing test suites untouched. |

### Step 7 — Blast-Radius Rating

🟢 **Low**. Net-additive change with no existing-API modifications. Existing oracles continue to defend code-side invariants; existing test suites are untouched. The only failure mode is "new surface doesn't work" — caught by the new oracles + Playwright + clean-state smoke.

---

## Interface Contracts (Stage 1 → Stage 2)

| Contract | Producer | Consumer(s) | Freeze at end of |
|---|---|---|---|
| **C20** — Brand kit | Stage 1 (you, the planner-executor) authoring `docs/brand/BRAND.md` | Stage 2 Worker A (marketing visuals); Stage 2 Worker B (compose-file header comments, ops-doc tone) | Stage 1 |
| **C21** — Canonical self-host env-var schema | Stage 1 authoring `docs/operations/SELFHOST_ENV.md` (audited from existing `crates/feedbackmonk-*` env consumption) | Stage 2 Worker B (docker-compose env section); Stage 2 Worker A (`/docs/self-host` doc page) | Stage 1 |
| **C22** — Pricing SSOT mechanism | Stage 1's Decision-1: build-step (Option A, recommended) OR hand-typed + parity oracle (Option B) | Stage 2 Worker A | Stage 1 |
| **C23** — Marketing-site URL routing scheme | Stage 1 enumerating the pages in this plan (§Stage 1 step 6) | Stage 2 Worker A | Stage 1 |
| **C24** — Smoke oracle three-probe specification | Stage 1's Decision-2 details (Probe A yaml-lint / Probe B env-var doc cross-ref / Probe C `--full` clean-state smoke) | Stage 2 Worker B (Task Zero) | Stage 1 |

Each Stage 1 sub-step authors the contract in detail (exact env-var list with semantics, exact pricing-export JSON shape, exact URL paths, exact probe commands) before Stage 2 fan-out.

---

## Deferred Decisions

| Decision | Deferred Until | Default if Unresolved | Why Defer |
|---|---|---|---|
| Pricing SSOT mechanism (Option A vs Option B) | Stage 1 Decision-1 | Option A (Rust→JSON build-step) | Both work; Stage 1 picks based on Astro-build integration friction |
| Build `selfhost-compose-smoke` oracle or skip | Stage 1 Decision-2 | Build (recommended) | Q1 reduction is worth ~150 LOC of harness |
| Build `marketing-pricing-parity` oracle | Stage 1 Decision-1 fallback | Skip if Option A taken; build if Option B taken | Conditional on Decision-1 |
| Admin-UI serving topology (single api-binary vs separate nginx) | Stage 2 Worker B | Single api-binary serving (recommended — simpler self-host story) | Affects Dockerfile shape but not env-var schema |
| Marketing-site hosting target (Cloudflare Pages / Vercel / Netlify / static-via-GitHub-Pages) | Post-P4 / pre-launch user action | Cloudflare Pages (Plausible's choice + already in the user's stack) | Hosting decision is post-content; doesn't block P4 implementation |
| Show HN post final copy | User decision at launch | Draft committed; user revises before posting | Show HN tone is highly personal |
| Custom-domain feature for $29+ tier in pricing display | Out per DEC-FBR-08 OUT list | Mentioned in pricing as "coming v1.1" or omitted | OUT scope |
| Polar billing resurrection | Remains DEFERRED per DEC-FBR-DEFER-01 | Pricing "Upgrade" buttons are mailto stubs (mirrors admin-UI pattern from P3 Stage 2) | Per arc constraint; user signal |

---

## Risks and Mitigations

| Risk | Likelihood | Severity | Mitigation |
|---|---|---|---|
| Brand pass turns into perfectionism trap (founder iterates logo for weeks) | Medium | Medium | Stage 1 produces "good-enough v1 brand kit" with explicit "iterate post-launch" framing. Don't gate launch on logo polish. DEC-FBR-09 already framed P4 as brand-pass-with-marketing-site, not separate brand sprint. |
| Docker stack works locally but fails on fresh VM (the canonical fidelity gap) | Medium-High | High | `selfhost-compose-smoke` Verification Oracle's Probe C runs `down -v && up`-style clean-state smoke. Mandatory at Stage 2 Worker B Task Zero. Pre-empts the entire fidelity-gap failure mode. |
| Marketing-site pricing drifts from `tier_quotas()` post-launch | Medium | Medium-High | Decision-1: build-step Rust→JSON export (Option A) makes drift structurally impossible. Fallback: parity oracle. |
| Astro / Vite / Node dependency hell on Windows (npm bug 4828 history) | Medium | Low | Pattern documented from P2 widget work (`docs/test-modifications/` + CONVERGENCE_REPORT). Apply the rollup-win32-x64-msvc manual binary-extraction workaround if it strikes again. |
| Self-host docs incomplete; first OSS user reports "I can't get it to start" | Medium | High | Stage 3 includes follow-the-runbook-cold integration witness (someone unfamiliar with the codebase should be able to follow SELFHOST.md and reach `/health` 200). Trade-off: founder is the only available cold-tester; mitigation imperfect — accept and rely on early-user feedback to harden. |
| PF-REGISTER-01 (org + domain) slips longer than expected | Low-Medium | Low | Doesn't block P4 implementation. All commits use `--skip-push` per handoff brief. The P4-close commit lands locally; pushing waits. |
| Stage 2 worker creates module without ULADP README.md | Low | Low | Both workers' exit gates explicitly require module README per ULADP. `/0-uldf-finalize` Phase 11 catches misses. |
| Marketing-site copy quality below Plausible bar (DEC-FBR-08 surfaced concern #3) | Medium | Medium | Honest framing: founder writes copy; iteration post-Show-HN signal is expected. Don't gate P4 launch on world-class copy. |
| Brand voice in copy clashes with widget "powered by feedbackmonk" footer style | Low | Low | Stage 1 brand voice guidelines include footer-mention consistency note. |
| `selfhost-compose-smoke` Probe C requires docker daemon at CI time | Low | Low | Probe A + B run anywhere (yaml-lint + cross-ref). Probe C is `--full`-gated like `tier-enforcement-status`'s Probe C. CI runs Probe C only on Linux runners with docker; not a Windows CI concern. |
| Existing four oracles regress on a P4 file | Very low | Medium | P4 doesn't touch the files those oracles cover. `/0-uldf-finalize` Phase 11 re-runs all oracles; the regression-window is one commit. |

---

## Execution Commands

The recommended next-step path (autopilot:continuous chain):

1. **Now**: this plan lands on disk; `/0-uldf-proceed` auto-fires (autopilot AUTOCHAIN-03).
2. **Stage 1**: `/0-uldf-proceed` likely picks HANDOFF (this planning session has consumed enough that a fresh context for Stage 1 work is cleaner; Stage 1 scope ~60k tokens is comfortable in a fresh session). Handoff brief points to this plan + Stage 1 scope. Successor performs brand pass + contract freezes + scaffold.
3. **Stage 1 → Stage 2 boundary**: the Stage 1 worker runs `/0-uldf-proceed` again, which routes to `/0-uldf-pods-parallelize` + `/0-uldf-pods-spawn-collaborator --all` (2 workers — A marketing + B docker).
4. **Stage 2 execution**: both workers run in parallel terminals. Lead-developer session monitors via `/0-uldf-pods-collab-sync` (manual or auto-monitor at autopilot).
5. **Stage 2 convergence**: `/0-uldf-pods-converge --finalize` (routes through `/0-uldf-ltads-stop` since LTADS active → which routes through `/0-uldf-finalize` Phase 9 with arc-terminus). Single P4-close commit lands with `--skip-push`.
6. **Post-commit**: user action queue — PF-REGISTER-01 (register org + buy domain), PF-RENAME-02 (rename working dir), then PF-LAUNCH-01 (push public + Show HN).

**DEC-12 gate evaluation at autopilot (`/0-uldf-proceed` Phase 3 will re-check)**:

- G1 (insufficient intake): NO — arc plan §P4 + DEC-FBR-08/09/10 + FR-FBR-16/17 fully delineate scope.
- G2 (new directory with 3+ code files OR new top-level module): YES — `marketing/` and `deploy/docker/` are both new top-level directories. **At autopilot: summary-only, do not halt** (plan-authorized module creation is pre-consented per DEC-12 Halt Principle).
- G3 (API/contract breaking change): NO — P4 is net-additive.
- G4 (delete/rename requirement IDs): NO.
- G5 (security globs intersection): MARGINAL — docker-compose handles secrets-like env vars (DATABASE_URL credentials, signing keys, SMTP creds). Plan-authorized; **at autopilot: summary-only**. Always-on Safety Rails defend against accidentally committing real secrets independently.
- G6 (pending `/0-uldf-ltads-admin decision` gate): NONE.

No halting gate fires at autopilot. Auto-chain proceeds.

---

## Notes for Downstream Consumers

- **`/0-uldf-pods-parallelize` / `/0-uldf-pods-spawn-collaborator`**: this plan's **Stage 2** is the PODS round. Stage 1 first; PODS spawn happens at Stage 1 → Stage 2 boundary, not directly off this plan.
- **`/0-uldf-ltads-start`**: LTADS already ACTIVE on this repo. Continue the existing session; do not start a new one.
- **`/0-uldf-proceed` (now)**: at autopilot:continuous, fires automatically post-plan. Will pick topology for Stage 1 (HERE or HANDOFF).
- **`/0-uldf-finalize` (at Stage 3 convergence)**: this is the P4-close commit. Carries `--complete-arc` semantics — `CSI-06` arc-terminus reconciliation triggers via three-signal detection. The v1 arc closes at this commit.
- **Public push**: REMAINS GATED on PF-REGISTER-01 until cleared. All P4 commits use `--skip-push`.
