# Execution Plan
**Source**: /0-uldf-ldis-plan
**Generated**: 2026-06-02T12:15:00
**Task**: feedbackmonk — GitCellar customer-#1 enablement: deploy + close parity gaps 1–4
**Strategy**: STAGED (sequential deploy track ∥ PODS build track → gated cutover)
**Intake Source**: docs/planning/intakes/20260602T120000-ready-feedbackmonk-as-gitcellar-backend.md

---

═══════════════════════════════════════════════════════════════
       LDIS EXECUTION PLAN
═══════════════════════════════════════════════════════════════

Task: Ready feedbackmonk as GitCellar's feedback backend + close parity gaps 1–4.
Strategy: **STAGED**, with the build stage executed as **PODS** (one worker per gap).

───────────────────────────────────────────────────────────────
STRATEGY RATIONALE
───────────────────────────────────────────────────────────────

Two tracks with different shapes, run concurrently:

- **Track A — Deploy + Provisioning** (sequential, ops/user-driven). Unblocks GitCellar's
  Phase 1 (anonymous website feedback) immediately with ZERO feedbackmonk feature work. Cannot
  be fully automated by an agent — needs Railway credentials + DNS access. Agent role here is
  prep (Railway config, provisioning script, runbook), not execution.
- **Track B — Gap-closing build** (PODS). 4 independent subsystems, code-verified missing,
  with ready-made boundaries (the parity checklist). Per global Task-Tool Escalation Rule
  (4+ file-modifying workstreams) and the intake Net collaboration score ~13, this is PODS.

Collaboration Value Assessment (Track B): Specialization 4/5, Quality 4/5 (no-regression
contract), Discovery 3/5, Speed 4/5 = 15/20. Friction: Boundary Clarity 5/5, Coupling 4/5
= 9/10. **Net = 15 − 4.5 = 10.5 → PARALLEL (PODS).**

Cutover (GitCellar retires its internal backend) is **gated** on all 4 gaps closing — tracked
by the `feedback-parity-status` oracle. GitCellar owns that cutover; feedbackmonk owns the gate
inputs.

───────────────────────────────────────────────────────────────
CONTEXT BUDGET ASSESSMENT
───────────────────────────────────────────────────────────────

Each gap is a bounded subsystem; all fit comfortably in one ~1M worker (<30% each incl. spec +
sibling summaries + interface contracts + reasoning reserve). Gap 1 (attachments) is the only
one near the line because it spans Rust backend + widget + storage — split into two workers
(B1 backend, B2 widget) to keep each well under budget and respect the 30KB widget cap as an
isolated concern. All others single-worker. **Pass.**

───────────────────────────────────────────────────────────────
EXECUTION OVERVIEW — STAGES
───────────────────────────────────────────────────────────────

**Stage 0 — Deploy + Provision (Track A, starts immediately, ~days, ops-gated)**
  0.1 Decide host + ownership (DECISION — see Risks/Decisions). Default: Railway, feedbackmonk-operated.
  0.2 Agent prep: Railway service config, `$PORT`/bind wiring, provisioning script
      (signup→verify→create project→register key), deploy runbook addendum to SELFHOST.md.
  0.3 User/ops: create Railway project, managed Postgres, set env (🔒 secrets), point DNS
      (`api.` / `app.` / `cdn.feedbackmonk.com`), run migrate one-shot.
  0.4 Provision GitCellar tenant + project + Ed25519 key; **fill real `project_id` into
      `docs/integrations/gitcellar-adoption.md` and flip it ACTIVE.**
  0.5 GitCellar embeds anonymous widget on gitcellar.com (GitCellar-side; Phase 1 done).
  → Fix the stale `embed_snippet` bug (discovery §7) as a tiny inline change here.

**Stage 1 — Oracle pre-build (before PODS spawn, ~hours)**
  1.1 Build `feedback-parity-status` Verification/state oracle. Consumed by every phase
      boundary and by GitCellar's cutover gate. Build-first per parallel oracle rule.

**Stage 2 — Gap-closing (Track B, PODS, parallel)**
  Workers B1–B5 (see Component Breakdown). Order-of-merge cheapest-first to bank parity early:
  Gap 4 → Gap 3 → Gap 2 → Gap 1 (1 is largest; B1+B2 run throughout).

**Stage 3 — Converge + parity verification (sequential)**
  3.1 `/0-uldf-pods-converge`; full `/0-uldf-finalize` quality gate; all oracles GREEN.
  3.2 `feedback-parity-status` reports 4/4 closed → signal GitCellar the cutover gate is OPEN.
  3.3 GitCellar runs Phase 3 (Desktop migrate + retire internal backend). feedbackmonk supports.

───────────────────────────────────────────────────────────────
COMPONENT BREAKDOWN (PODS workers, Stage 2)
───────────────────────────────────────────────────────────────

| Worker | Gap | Scope | Migration | Key files |
|---|---|---|---|---|
| **B-DELTA** | #4 my-feedback read API | 2 JWT-`sub`-scoped read routes (`GET …/me/feedback` list; `GET …/me/feedback/:fb/thread` → status + PUBLIC replies only). No schema change. Reuse `feedbackmonk_jwt::verify_with_leeway` + repo. | none | `handlers/me_feedback.rs` (new), `router.rs`, repository read methods |
| **B-CHARLIE** | #3 admin full-text search | `tsvector` generated column + GIN index; `GET /api/v1/admin/feedback/search?q=` (AdminSession-gated, tenant-scoped); admin UI debounced search box. | 00011 (or next free) | migration, `admin_feedback.rs`, `admin-ui/.../feedback` |
| **B-BRAVO** | #2 crash correlation | `crash_event_id TEXT` on `feedback` (nullable); accept on auth-mode submit; correlation worker polling/receiving Glitchtip; Desktop crash-link-banner contract. | 00010 | migration, `handlers/feedback.rs`, new worker module, integration-contract addendum |
| **B-ALPHA-1** | #1 attachments (backend) | `attachments` table (FK feedback, ≤4/row, ≤5MB, image MIME allowlist); S3-compatible storage (`FEEDBACKMONK_S3_*`, already namespaced in SELFHOST_ENV §Out-of-Scope); multipart upload route; **service/console-log capture routed through `feedbackmonk-tracing` 20-pattern PII scrubber**. | 00009 | migration, new handler, storage module, `feedbackmonk-tracing` reuse |
| **B-ALPHA-2** | #1 attachments (widget) | Screenshot attach (≤4), **canvas redaction tool**, console-log capture, service-log capture; upload via B-ALPHA-1 contract. **Must respect 30KB bundle cap** — redaction UI likely lazy-loaded/code-split. | none | `widget/src/*`, widget tests |

B-ALPHA-1 ↔ B-ALPHA-2 meet at the **upload contract** (see Interface Contracts).

───────────────────────────────────────────────────────────────
ORACLE PRE-BUILD PLAN
───────────────────────────────────────────────────────────────

| Oracle | Question | Consumer(s) | Timing | Status |
|---|---|---|---|---|
| `feedback-parity-status` | Which of gaps 1–4 are closed; is the GitCellar cutover gate open? | all workers + both repos' phase boundaries | build before spawn (Stage 1) | not yet built |

**Rationale**: multi-phase cross-repo effort where every boundary re-asks "are we at parity?"
One oracle consulted by N workers + GitCellar inverts N× redundant checklist re-derivation.
Reads FR-FBR statuses + a parity-marker file; freshness contracted to those. Gracefully absent
(humans can read the checklist).

**Existing oracles that guard this work (no build needed)**: `multi-tenant-isolation-check`
(guards B-DELTA's new query path + all new migrations), `pii-scrub-audit` (guards B-ALPHA-1's
log capture — must route through the canonical scrubber), `widget-bundle-size` (guards
B-ALPHA-2 against the 30KB cap).

**Deferrals**: none beyond the above — single-pass items are absent here.

───────────────────────────────────────────────────────────────
TESTABILITY GATE FINDINGS (Probandurgy — flagged items)
───────────────────────────────────────────────────────────────

Scored 1–5 (5 worst): Q1 iter-cost, Q2 fidelity-risk, Q3 critical-path, Q4 scaffolding-leverage, Q5 drift.

- **Gap #1 attachments — composite 16, FLAGGED.** Q1=5 (storage+multipart+image+redaction =
  slow manual verify), Q2=4 (a verifier that doesn't prove PII is actually scrubbed from
  captured logs misses the load-bearing privacy failure — DEC-FBR-02/FR-FBR-10), Q3=3, Q4=5,
  Q5=4. **Recommendation**: pre-build a **fixture corpus** of service/console logs containing
  known PII (mirroring `pii-scrub-audit`'s pattern set) + sample images; assert post-capture
  bytes contain none of the known PII tokens. Drift detection: the fixture's PII token list is
  hashed against `feedbackmonk-tracing` CANONICAL_PATTERNS (same discipline as pii-scrub Probe B).
- **Gap #4 my-feedback read API — Q2=5, FLAGGED on fidelity alone (composite only 9).** The
  endpoint's whole risk is **isolation**: it must return ONLY the caller's own `sub` and ONLY
  PUBLIC replies (never internal triage notes, never another user's feedback). A verifier that
  doesn't test cross-`sub` and public/internal isolation misses the only failure that matters —
  this is the same class as the Q24 promote invariant. **Recommendation**: byte-for-byte
  isolation test set (own-sub-only, public-replies-only, cross-tenant 404) authored as a frozen
  fixture before B-DELTA implements; `multi-tenant-isolation-check` provides the second leg.
- Gap #2 (composite 11) and Gap #3 (composite 8): not flagged. Gap #2's correlation worker
  benefits from a **mock Glitchtip fixture** (recommended, not gating) so the worker is testable
  without a live Glitchtip.

───────────────────────────────────────────────────────────────
RIPPLE ANALYSIS
───────────────────────────────────────────────────────────────

- **Migrations 00009–00011**: three workers add migrations → **collision risk**. Frozen numbering
  in Interface Contracts (B-ALPHA-1=00009, B-BRAVO=00010, B-CHARLIE=00011). `.sqlx/` offline
  cache must be regenerated at converge. `multi-tenant-isolation-check` runs on every schema change.
- **`router.rs`**: B-DELTA + B-CHARLIE both add routes → both edit `router.rs`/handler `routes()`.
  Low-conflict (additive) but a known shared file — converge resolves.
- **Widget (B-ALPHA-2)**: bundle size is the binding constraint. Redaction tooling can blow the
  30KB cap → `widget-bundle-size` Probe A will fail loud. Mitigation: code-split/lazy-load the
  redaction canvas; treat the cap as a hard design input, not a post-hoc check.
- **`feedbackmonk-tracing`**: B-ALPHA-1 reuses the scrubber; must NOT create a second
  subscriber/scrub path (pii-scrub-audit Probe A AST forbids it). Reuse the existing chokepoint.
- **Integration contract**: B-DELTA freezes gap-#4 endpoint paths (currently "proposed" in §6);
  B-BRAVO adds the crash-link-banner contract; Stage 0.4 fills `project_id`. All edits to
  `docs/integrations/gitcellar-adoption.md` — coordinate (it's GitCellar's source of truth).

───────────────────────────────────────────────────────────────
INTERFACE CONTRACTS (cross-worker, freeze before spawn)
───────────────────────────────────────────────────────────────

1. **Migration numbering** — B-ALPHA-1=`00009_attachments.sql`, B-BRAVO=`00010_feedback_crash_event.sql`,
   B-CHARLIE=`00011_feedback_fts.sql`. No worker picks its own number. `.sqlx` regenerated at converge only.
2. **Attachment upload contract (B-ALPHA-1 ↔ B-ALPHA-2)**: `POST /api/v1/projects/:id/feedback/:fb/attachments`
   multipart; field `files[]` (≤4); per-file ≤5MB; MIME ∈ {image/png,image/jpeg,image/webp}; returns
   `{ attachment_id, url }[]`. Logs captured as a separate text part `service_log` / `console_log`,
   scrubbed server-side before persist. (B1 and B2 ratify exact shape in channel before coding.)
3. **Gap-#4 endpoints (B-DELTA, freezes §6)**: `GET /api/v1/projects/:id/me/feedback` (Bearer, paginated,
   own-sub only) + `GET /api/v1/projects/:id/me/feedback/:fb/thread` (status + public replies only).
4. **crash_event_id (B-BRAVO)**: nullable `TEXT` on `feedback`; set in auth-mode submit from a new
   optional request field `crash_event_id` (NOT smuggled in `external_metadata`); correlation worker
   reads it. Desktop crash-link-banner shape documented in the integration contract addendum.

───────────────────────────────────────────────────────────────
COORDINATION REQUIREMENTS
───────────────────────────────────────────────────────────────

- LD (this session, post-spawn) is pure coordination — never implements (global PODS rule 4).
- Shared files (`router.rs`, `migrations/`, `docs/integrations/gitcellar-adoption.md`): touch-log
  + channel sync before edit; converge resolves.
- Sync points: (a) after Stage 1 oracle build; (b) when each gap's frozen fixture lands (B-DELTA
  isolation set, B-ALPHA PII corpus) before that worker implements; (c) at converge.
- Convergence criteria: all 5 workers report DONE; all oracles GREEN; `cargo test` + admin-ui
  Vitest + widget Playwright/axe GREEN; `feedback-parity-status` = 4/4; integration contract ACTIVE.

───────────────────────────────────────────────────────────────
DEFERRED DECISIONS
───────────────────────────────────────────────────────────────

- **D-Stage0 — RESOLVED (2026-06-02)**: GitCellar self-hosts feedbackmonk on its existing Railway,
  **reusing GitCellar's existing Postgres** (new database; feedbackmonk is multi-tenant on one PG).
  Lowest incremental cost. `feedbackmonk.com` SaaS deferred. Runbook: `docs/operations/RAILWAY_GITCELLAR.md`.
- Attachment storage backend specifics (S3 provider) — decide at B-ALPHA-1 Task Zero; env names
  already reserved (`FEEDBACKMONK_S3_*`).
- Glitchtip correlation mode (push webhook vs pull poll) — B-BRAVO Task Zero; mock fixture either way.

───────────────────────────────────────────────────────────────
RISKS & MITIGATIONS
───────────────────────────────────────────────────────────────

| Risk | Mitigation |
|---|---|
| Widget redaction blows 30KB cap | Lazy-load/code-split redaction; `widget-bundle-size` fails loud; design input not afterthought |
| PII leak via captured logs (privacy-critical) | Route through canonical scrubber (reuse only); pre-built PII corpus fixture; pii-scrub-audit Probe A |
| Gap-#4 cross-user/internal-reply leak | Frozen isolation fixture before impl; multi-tenant-isolation-check second leg |
| Migration number collision | Frozen numbering contract above |
| Deploy blocked on credentials/DNS | Track A is ops-gated by design; Track B proceeds independently; agent preps config so ops step is mechanical |
| `.sqlx` offline-cache drift across parallel migrations | Regenerate once at converge, not per-worker |

───────────────────────────────────────────────────────────────
EXECUTION COMMANDS
───────────────────────────────────────────────────────────────

1. Stage 1 oracle: build `.claude/oracles/feedback-parity-status/`.
2. Stage 2 build: `/0-uldf-pods-parallelize --from-ldis-plan=docs/planning/plans/20260602T121500-gitcellar-customer-1-enablement.md`
   → `/0-uldf-pods-spawn-collaborator --all` (workers B-DELTA, B-CHARLIE, B-BRAVO, B-ALPHA-1, B-ALPHA-2).
3. Monitor: `/0-uldf-pods-collab-sync`. Converge: `/0-uldf-pods-converge`.
4. Track A (parallel, ops-gated): prep Railway config + provisioning script; user runs deploy; fill `project_id`.

═══════════════════════════════════════════════════════════════
