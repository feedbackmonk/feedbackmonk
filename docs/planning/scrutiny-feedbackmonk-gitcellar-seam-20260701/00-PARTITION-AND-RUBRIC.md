# Scrutiny: FeedbackMonk + GitCellar seam — Partition Plan & Shared Rubric

**Date**: 2026-07-01 · **Mode**: `/0-uldf-scrutinize --cascade` (root = this doc's author session; children = read-only reviewer agents) · **Autonomy**: autopilot (arc `phase-a-gitcellar-contract-20260701`) — partition emitted, not paused on.
**Trigger**: handoff brief `.claude/handoff/handoff-20260701-scrutinize-feedbackmonk-gitcellar-seam.md`.
**Output**: child reports in this directory; unified report at `SYNTHESIS.md`. Analysis-only — no fixes this session (except <5-line trivia per Work Discovery Protocol).

---

## Phase 0 — Scope Contract

**IN**: the FeedbackMonk repo end-to-end — 7 Rust crates (`feedbackmonk-{api,core,repository,runner,jwt,anon,tracing}`, ~43k lines incl. tests), `widget/` (~1.5k), `admin-ui/` (~10k), `migrations/` (21), `deploy/docker/`, `docs/{specs,integrations,operations}/`, `.claude/oracles/` (the oracle suite is a review SUBJECT); plus the **GitCellar integration seam**: `docs/integrations/gitcellar-adoption.md` (contract SSOT), `GET /api/v1/capabilities`, JWT mint/verify handshake, deploy path, and — **read-only** — the consumer side in `S:\SourceControlled\Apps\GitCellar` (what Desktop/Cloud actually call + the consolidation plan `docs/planning/plans/20260701-feedback-consolidation-onto-feedbackmonk.md`).

**OUT**: GitCellar's internal feedback code as a review subject (being deleted; only its consumption of the seam matters). No modifications to the GitCellar repo. No fix implementation this session.

**Governing artifacts** (authority-bearing, all challengeable per Directive 1): `docs/specs/{SPECIFICATION,DECISIONS,ARCHITECTURE,OPEN_QUESTIONS,DISCOVERIES}.md`, `docs/integrations/gitcellar-adoption.md`, `CLAUDE.md` (project), the frozen Contracts C1–C30, the v1 build-arc plan, the Phase-A plan/intake, `.claude/oracles/INDEX.md`.

**Size self-check**: ~55k+ lines of substance + a peer repo ⇒ exceeds one session's full-standard budget ⇒ cascade engaged (children are read-only reviewer agents, each with its own full context window; root holds rubric, cross-cutting consistency, generative leg, global Phase 4, synthesis).

---

## Phase 1 — Shared Rubric (inherited by every child; challengeable BY ESCALATION ONLY — a child never silently forks it)

### What the scope is FOR

FeedbackMonk is a **privacy-first, open-source (AGPL-3.0) user-feedback platform** — "Plausible Analytics for product feedback" — for indie developers and privacy-conscious teams (Personas A + D). Submission widget + triage workflow + public board/roadmap with voting + status emails + (P5) an agentic feedback-resolution loop, multi-product per tenant, SaaS and self-host from one codebase.

**The review's north star (sole-backend bar)**: GitCellar's Phases B/C will DELETE its internal feedback backend; FeedbackMonk becomes the ONLY place GitCellar's feedback lives, with no fallback. Every finding is weighed against: *"is this good enough to be the sole backend, with no safety net, for a privacy-branded product?"* That raises the bar on durability (backup/restore), availability posture, operational diagnosability, abuse resistance, erasure/export correctness, and contract fidelity — not just code correctness.

**Consumers**: (1) GitCellar Desktop/Cloud as machine consumer #1 over the frozen contract; (2) tenant admins (admin-ui); (3) end-users via widget/board/roadmap (incl. anonymous); (4) self-host operators (`docker compose up`); (5) the customer-side agentic runner; (6) future OSS adopters reading the docs.

### Load-bearing invariants (audit standard AND Phase-4 targets)

1. **Privacy brand promises**: no third-party trackers ever (DEC-FBR-02); the customer-signed JWT is the ONLY end-user identity FeedbackMonk has (DEC-FBR-04); Q24 — promoted roadmap items carry the body verbatim with NO submitter attribution/FB-ID; 20-pattern PII log scrubbing at a single chokepoint (FR-FBR-10); public wire shapes leak no submitter PII (C29); translation egress opt-in, default off (DEC-FBR-IMPL-26); solicitation `opted_out` terminal (DEC-FBR-IMPL-24); Phase-A erasure = bytes + rows, completely (D-A1).
2. **Multi-tenant isolation**: tenant-scoped repository layer is the SOLE query path; raw SQL is a security incident (DEC-FBR-03). Every domain row carries `tenant_id` + `project_id`.
3. **P5 trust boundary**: owner approval is a security boundary between public input and code execution (FR-FBR-25a); feedback content is data, never instructions (25b, single `wrap_untrusted` chokepoint); nothing but conclusions crosses outbound (25c, `sanitize_outbound`); runner-class keys can never author approval or mint identity JWTs.
4. **Seam discipline**: `docs/integrations/gitcellar-adoption.md` is the contract SSOT; changes are additive and advertised via `GET /api/v1/capabilities`; live instance currently v0.2.0 pending the A6 redeploy (contract targets v0.3.0).
5. **Verification honesty**: oracles claim detection-from-code/ledger ("anti-reward-hacking"), never self-reported flags; CI-parity gate (`scripts/ci-local.sh`) green before push.

### What "done/valuable" means

A competent operator can run this as the only home of a product's feedback — with working erasure, export, backup/restore, upgrade, monitoring, rate limiting, and abuse resistance — and GitCellar can delete its fallback without acquiring new risk. The generative leg measures the gap to THAT, not to the spec's own checklist.

### Standing challenge flags (carry into every Phase 4)

- Does the **frozen-contract discipline** ossify the seam just when a sole-backend consumer needs it to evolve fastest?
- Is **JWT-only identity** (DEC-FBR-04) still sufficient when FeedbackMonk is the sole record-holder (erasure requests arriving out-of-band, account recovery, GDPR data-subject flows)?
- Are **12+ verification oracles** all earning their keep, or is part of the suite dead weight / false confidence (e.g., `feedback-parity-status` post-cutover)?
- Does "no trackers / minimal data" tension against the **operational monitoring** a sole backend requires?
- Is the spec's own DONE-ness (every FR marked DONE) masking unserved goals (Directive 9)?

---

## Partition (children, fences, budgets)

| # | Child (slug) | Boundary | Lens priority | Must NOT review (fence) |
|---|---|---|---|---|
| A1 | `a1-public-api` | Public/end-user API: `handlers/{feedback,me_feedback,attachments,board,roadmap,voting_common,solicitation,widget_config,capabilities,health}.rs`, `cors.rs`, `router.rs`, `auth/`, `feedbackmonk-jwt`, `feedbackmonk-anon`, matching tests | AuthZ/authn per route, rate limiting, input validation, enumeration/abuse, idempotency semantics | Repo SQL internals (B); privacy deep-dive (C — flag, don't own); agentic handlers (D); contract-doc drift (F) |
| A2 | `a2-admin-api` | Admin/tenant surface: `handlers/{login,signup,verify_email,admin_feedback,admin_ops,admin_tier,moderation,promote,projects,signing_keys}.rs`, `email/`, session auth, tier enforcement, `crash_correlation.rs` | Session security, admin authz, ops-token surface, email pipeline, tier/quota correctness | Public routes (A1); runner/work-order handlers (D); frontend (E) |
| B | `b-data-layer` | `feedbackmonk-repository`, `feedbackmonk-core`, `migrations/*.sql` | Tenancy scoping in SQL, FK/cascade completeness, constraint honesty, index/perf at sole-backend volume, idempotency table, state machines | Handler-layer authz (A1/A2); oracle quality (H) |
| C | `c-privacy-pipeline` | Privacy end-to-end: `feedbackmonk-tracing`, `translation/`, erasure path (`me_feedback.rs::delete_my_feedback` + `attachments.rs` repo + `storage.rs`), export handler, anon hashing, Q24 promote path, email-content PII, moderation wire shapes | Are the privacy invariants COMPLETE or merely the ones we thought of? Erasure residue hunting (logs, FTS, translations, clusters, emails, backups) | Generic handler correctness (A1/A2); oracle mechanics (H — but verify claims against code) |
| D | `d-agentic-boundary` | P5 loop: `feedbackmonk-runner` (all), `handlers/{work_orders,sweeps,recommendations,clusters,runner_tokens}.rs`, key-class separation, `feedback_injection_corpus.rs`, `RUNNER_PROTOCOL.md` | Is the approval gate as strong as its oracles claim? Injection corpus coverage, runner token lifecycle, `result_ref` egress, dispatch-on-approve | Non-agentic admin surface (A2); oracle INDEX hygiene (H) |
| E | `e-frontends` | `widget/` + `admin-ui/` + `marketing/` (light) | XSS/injection in rendering user content, token/session handling client-side, bundle cap + tracker promise, a11y claims vs reality, board/roadmap public UI | Backend handlers (A1/A2); deploy of frontends (G) |
| F | `f-gitcellar-seam` | `docs/integrations/gitcellar-adoption.md` vs actual routes/`capabilities.rs`; GitCellar repo READ-ONLY (consumer call sites + consolidation plan); JWT TTL/re-mint ergonomics; reply-state polling; idempotency scope; severity; export; delete; A6 version skew | Contract-vs-consumer drift; what Desktop leans on post-B/C with no fallback; failure modes when FM is down/slow | Implementing anything; reviewing GitCellar internals beyond seam consumption |
| G | `g-ops-deploy` | `deploy/docker/`, `docs/operations/*`, backup/restore scripts, health endpoints, Railway runbook, migration/upgrade story, env catalog C21 | Sole-backend ops: backup/restore actually tested? monitoring/alerting? retention? DR? upgrade path? A6 gate operational risk | App-code correctness (A/B); marketing site content (E) |
| H | `h-oracle-suite` | `.claude/oracles/` project oracles + INDEX.md + `scripts/ci-local.*` + CI workflow; test-suite coherence spot-check | Honesty-of-claims per probe (does it detect what it says?), staleness, dead weight (retire-or-justify per oracle), coverage gaps vs invariants 1–3 | Re-reviewing the invariants themselves (C/D own those) |
| EXT | `ext-field-scan` | External research (web) | Has the field moved? Feedback-platform table stakes 2026; GDPR erasure/export norms; idempotency-key standards; prompt-injection defense state of the art; sole-backend SLO/backup norms for indie SaaS | Repo code (verdicts must be judged against child summaries at root) |

**Root-retained** (not delegated): cross-cutting consistency pass (over child reports), scope-wide generative leg, global Phase 4 (3 fresh critics on mandate families: M1 privacy/identity model; M2 tenancy/data-layer mandates; M3 seam + frozen-contract + sole-backend + ADD proposals), retire-or-justify merge, SYNTHESIS.md.

**Child protocol**: Phases 2–3 + local Phase 4 on cluster-local mandates; findings tagged P0/P1/P2 + liveness (live-surface vs archival) + three-way adjudication (artifact wrong / reality wrong / rule wrong); every finding cites `file:line`; every "this is fine" names what was checked; rubric challenges escalate upward as findings. Reports ≤ ~3,500 words each.
