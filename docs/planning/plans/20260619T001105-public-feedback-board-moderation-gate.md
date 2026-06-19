# Execution Plan
**Source**: /0-uldf-ldis-plan
**Generated**: 2026-06-19T00:11:05
**Task**: feedbackmonk — Public Feedback Board + Moderation Gate (public-visibility approval gate before submissions become public)
**Strategy**: STAGED (Stage 0 freeze → Stage 1 parallel fanout)
**Intake Source**: derived in-session from code investigation (no prior intake; this plan stands as the intake-of-record for the feature)

---

═══════════════════════════════════════════════════════════════
       LDIS EXECUTION PLAN
═══════════════════════════════════════════════════════════════

## Task

Build a **public feedback board** (end-users see and vote on other submissions, Canny-style) **gated by an owner moderation/approval boundary**: no feedback row becomes visible on any public surface until an authenticated owner has approved it. Scope confirmed with user 2026-06-19 (chose "Public board + moderation gate" over foundation-only / board-only).

## Premise correction (load-bearing — this reshaped the feature)

Code investigation established what is **actually** public today, which differs from the initial framing:

- **No public feedback board exists.** Raw feedback is **admin-only** (`admin_feedback.rs`, behind `AdminSession`).
- The only public-facing surfaces are:
  1. **Public roadmap** (`roadmap.rs::roadmap_router`) — already curated; items appear only via the manual admin **promote** action (`promote.rs`) or `admin_create`. Nothing reaches it automatically.
  2. **`/me/feedback`** (`me_feedback.rs`) — JWT-scoped to each submitter's *own* feedback; other users never see it.
- The `feedback.status` enum (`feedbackmonk-core/src/status.rs`) is **triage-only** (submitted/triaged/in-progress/shipped/wontfix/duplicate). It is **NOT** a public-visibility axis and must not be overloaded as one.

**Consequence**: a moderation gate is only meaningful paired with a public surface to guard. This feature therefore builds **both** the public board **and** the gate, and introduces a **new, orthogonal moderation axis** rather than extending the triage status machine.

## Strategy Rationale

**STAGED**, mirroring the established project rhythm (P5a/P5b both ran Stage 0 freeze → Stage 1 fanout):

- **Hard sequencing dependency**: the migration + core moderation state machine + interface contracts + the moderation-gate Verification Oracle skeleton must be **frozen** before parallel workers build against them. Workers making independent decisions on the moderation enum or wire shapes would produce incompatible outputs.
- After the freeze, the work splits cleanly into **four low-coupling domains** (backend endpoints, oracle, admin-UI, public-UI) → parallel fanout with strong boundaries.

**Collaboration Value Assessment** (per `docs/PARALLEL_COLLABORATION_DESIGN.md`):
- Value: Specialization 4 (Rust/React/oracle are distinct skill surfaces), Quality 4 (trust boundary benefits from an independent oracle author + cross-check), Discovery 3, Speed 4 → **15/20**
- Friction: Boundary Clarity 5 (frozen contracts), Coupling 4 (shared only via frozen migration + core enum) → **9/10**
- Net: 15 − (9/2) = **10.5 → PARALLEL strongly recommended for Stage 1.** Stage 0 is single-session (a freeze cannot be parallelized).

## Context Budget Assessment

Each Stage 1 worker fits comfortably under 85%:
- **Worker A (backend)**: feedback repo + 2 handler files + router wiring + integration tests ≈ moderate. Pass.
- **Worker B (oracle)**: one oracle dir (Python canonical + shims) + reads state-machine source. Small. Pass.
- **Worker C (admin-UI)**: moderation queue page + settings + API client. Moderate. Pass.
- **Worker D (public-UI)**: one hosted page mirroring `PublicRoadmap.tsx` + board API client. Small-moderate. Pass.

No component exceeds budget; no deeper decomposition tier needed.

## Oracle Pre-Build Plan

| Oracle | Question | Consumer(s) | Timing | Status |
|---|---|---|---|---|
| `public-board-moderation-gate` | Can any public-board endpoint return a feedback row whose `moderation_status != approved`? (the trust boundary) | Workers A, C, D (exit gate); all future board work | **Stage 0 skeleton; activate probes in Stage 1** | not yet built |

**Rationale**: This is the single highest-leverage scaffold (Testability Gate Flag 1, below). It must be **detection-from-ledger / detection-from-code**, NOT a self-reported `is_public` boolean — exactly mirroring the existing `approval-gate-enforcement` oracle that defends FR-FBR-25a. A green oracle is each backend/UI worker's exit gate, which is why it is built first. Probe shape (mirror `approval-gate-enforcement`):
- **Probe A** (source): the public board repo query (`list_public_board` / `get_public_board_item`) hard-filters `moderation_status = 'approved'` in SQL — not in the handler, not optional.
- **Probe B** (handler authz): no public board handler returns submitter PII (email / anon identity / `end_user_sub` / name).
- **Probe C** (`--full`): runs the board isolation + moderation integration tests for drift detection.

**Deferrals** (evaluated, not scheduled):
- A separate `board-privacy-invariant` oracle: folded into Probe B above rather than a second oracle — same file surface, cheaper as one.

## Execution Overview

```
Stage 0 — FREEZE (single session, ~1 focused unit)
  └─ migration 00016 + core moderation state machine + Contracts C28/C29 + oracle skeleton
            │  (GATE 0: migration applies, core compiles + unit-tests green, contracts frozen)
            ▼
Stage 1 — FANOUT (PODS, 4 workers in parallel against frozen contracts)
  ├─ Worker A: backend — moderation repo + admin queue endpoints + public board read endpoints
  ├─ Worker B: Verification Oracle — activate public-board-moderation-gate probes A/B/C
  ├─ Worker C: admin-UI — moderation queue + per-project board settings
  └─ Worker D: public-UI — hosted public board page + voting wiring
            │  (GATE 1: oracle green, isolation tests pass, board renders only approved rows, a11y pass)
            ▼
  Finalize — spec/decisions reconciliation + privacy-invariant doc + /0-uldf-finalize
```

## Component Breakdown

### Stage 0 — Freeze (single session)
1. **Migration `00016_feedback_moderation.sql`**
   - `feedback.moderation_status TEXT NOT NULL DEFAULT 'pending' CHECK (moderation_status IN ('pending','approved','rejected'))`.
   - Append-only `feedback_moderation_events` audit table (mirror `work_order_events`): `id, tenant_id, project_id, feedback_id FK, from_status, to_status, actor_user_id, reason_note NULL, created_at`. This is the **ledger** the oracle reads — the anti-reward-hacking substrate.
   - Per-project board settings: `projects.public_board_enabled BOOLEAN NOT NULL DEFAULT FALSE` + `projects.board_requires_moderation BOOLEAN NOT NULL DEFAULT TRUE`. **Default OFF** so no existing project silently exposes feedback on deploy (safe long-term default; recorded as a decision).
   - Backfill: existing rows → `moderation_status='pending'` (board is off by default, so nothing leaks; owners curate from the queue when they enable the board).
   - Index for the board read path: `feedback (project_id, moderation_status, accepted_at DESC)`.
2. **Core: `feedbackmonk-core/src/moderation.rs`** — `ModerationStatus` enum (kebab/lowercase DB form), `legal_moderation_transitions_from`, `ModerationError`. Frozen exactly as `status.rs` froze the triage machine in Stage 1 of P1.
3. **Contracts** (write to `docs/planning/handoffs/` for the fanout, as P2/P5 did):
   - **C28 — Moderation state machine + admin queue**: `POST /api/v1/admin/feedback/{id}/moderate {to_status, reason_note}`; same-txn status UPDATE + `feedback_moderation_events` INSERT (mirror C6 invariant #4 in `perform_transition`). Hard invariant: no public exposure without a recorded owner-authored `approved` event.
   - **C29 — Public board read + privacy shape**: `GET /api/v1/projects/{id}/board?limit=&offset=` and `GET /api/v1/projects/{id}/board/items/{short_code}` returning ONLY `moderation_status='approved'` rows via `open_for_submission` scope; wire shape carries `body, kind, status, vote_count, accepted_at` and **NEVER** email / anon identity / `end_user_sub` / name (privacy invariant, sibling to Q24). Voting reuses the existing anon/JWT voter-resolution pattern from `roadmap.rs`.
4. **Oracle skeleton**: `.claude/oracles/public-board-moderation-gate/` scaffolded (manifest + probe stubs), cold-start vacuous-PASS.

### Stage 1 — Fanout (4 parallel workers)
- **Worker A — Backend** (`feedbackmonk-repository` + `feedbackmonk-api`):
  - Repo: `moderate_in_executor` (same-txn status + event append), `list_pending_for_admin`, `list_public_board` / `get_public_board_item` (SQL-level `moderation_status='approved'` filter — Probe A target).
  - Handlers: admin `moderate` + pending-queue list (extend `admin_feedback.rs` or new `moderation.rs`); public `board.rs` router (mirror `roadmap_router` structure + CORS layer wiring in `main.rs::build_app`, matching the public submit/attachments pattern).
  - Board voting: reuse `roadmap_votes` voter-resolution helpers; decide board-vote storage (new `feedback_board_votes` table OR generalize) — **flagged as the heaviest sub-piece; may defer voting to a follow-up if it threatens GATE 1** (board read + moderation is the core; voting is additive).
  - Tests: `board_moderation_gate.rs` (approved-only), `board_privacy_isolation.rs` (no PII — port the shape of `me_feedback_isolation.rs`), moderation state-machine tests.
- **Worker B — Oracle**: activate Probe A (SQL filter present), Probe B (no PII in board wire shapes), Probe C (`--full` runs A's isolation + gate tests). Detection-from-code. Green oracle = GATE 1 anti-reward-hacking leg.
- **Worker C — Admin-UI** (`admin-ui/`): moderation **queue** page (pending list → approve/reject with reason), per-project **board settings** (enable board, require moderation toggles), API client additions. a11y e2e (axe-core, per project mandate).
- **Worker D — Public-UI** (`admin-ui/` public route, mirror `PublicRoadmap.tsx`): hosted public board page at the existing public-route pattern (`/public/projects/:projectId/board`), renders approved rows + vote controls, no-auth. a11y e2e.

## Testability Gate Findings

| Item | Q1 iter | Q2 fidelity | Q3 path | Q4 scaffold | Q5 drift | Composite | Flag |
|---|---|---|---|---|---|---|---|
| **Moderation trust boundary** (no public exposure without approval) | 2 | **5** | 5 | 5 | 3 | 20 | **FLAGGED — Q2=5 + composite>12** |
| **Board privacy** (no submitter PII on public surface) | 2 | 4 | 4 | 4 | 3 | 17 | **FLAGGED — composite>12** |
| Admin queue UI / public board UI | 3 | 2 | 2 | 2 | 2 | 11 | not flagged |

**Trust boundary (Q2=5)**: a self-reported `is_public`/`moderation_status` flag returned by the handler is the reward-hacking surface — a worker could "pass" by trusting a column the same code path can set. **Mitigation (mandatory, not optional)**: the `public-board-moderation-gate` oracle is **detection-from-ledger/code** (asserts the SQL query itself hard-filters `approved`, and that exposure transitions require a recorded owner-authored `approved` event in `feedback_moderation_events`). This is the exact pattern that already defends FR-FBR-25a via `approval-gate-enforcement`. Q4=5: the oracle halves iteration cost and is the highest plan-wide leverage scaffold → built first (Stage 0 skeleton). Q5 drift: Probe C runs the integration tests so the oracle can't pass against drifted behavior.

**Board privacy (composite 17)**: mitigated by a dedicated isolation test (`board_privacy_isolation.rs`) modeled on `me_feedback_isolation.rs` + Probe B. New load-bearing privacy invariant, sibling to Q24 — document as untouchable in the board module README.

## Ripple Analysis

| Modified interface | Consumers traced | Impact |
|---|---|---|
| `feedback` table (+`moderation_status`) | `feedbackmonk-repository` feedback repo, `.sqlx/` offline cache, every feedback query | New column has a DEFAULT → existing INSERTs unaffected; SELECTs must opt into the column. Regenerate `.sqlx/`. |
| `projects` table (+2 board flags) | project repo, admin settings, board read path | DEFAULT FALSE → no behavior change until an owner opts in. |
| `main.rs::build_app` | router composition | New `board_router` merged **with** `.layer(cors)` (public surface — matches submit/attachments; see `cors-allowlist-enforcement`). New admin moderation routes merged **without** CORS. |
| Public submit path (`feedback.rs::submit`) | — | New rows default to `pending`. **No change to submit behavior or response shape** — moderation is downstream of acceptance. |
| `multi-tenant-isolation-check` oracle | board + moderation queries | New queries must be tenant-scoped via repo layer (DEC-FBR-03). Oracle must stay green. |

Blast radius: **🟡 Medium** — additive schema + new endpoints, but touches the load-bearing public-CORS surface and the privacy invariants, so the new oracle + isolation tests are required gates.

## Interface Contracts (frozen in Stage 0, consumed in Stage 1)

- **C28** governs A (impl) ↔ C (admin-UI client). Exact: `POST /api/v1/admin/feedback/{id}/moderate`, body `{ "to_status": "approved"|"rejected", "reason_note": string? }`, 200 `{ feedback_id, from_status, to_status, moderated_at, audit_id }`, 409 on illegal transition (mirror C7 error body).
- **C29** governs A (impl) ↔ D (public-UI client) ↔ B (oracle). Exact board wire shape + the approved-only + no-PII invariants above.

## Coordination Requirements

- Stage 0 GATE 0 (single session self-verifies): migration applies on local PG (`feedbackmonk-pg-dev`, port 5433), `feedbackmonk-core` compiles + moderation unit tests green, contracts written to `docs/planning/handoffs/`, oracle skeleton cold-start PASS.
- Stage 1 sync point GATE 1 (LD verifies at converge): `public-board-moderation-gate` oracle GREEN (A+B), isolation/gate tests pass, admin queue + public board render correct (approved-only), a11y green, `multi-tenant-isolation-check` + `cors-allowlist-enforcement` still green.

## Deferred Decisions

- **Board voting storage** (new `feedback_board_votes` vs generalizing `roadmap_votes`): defer the table-shape decision to Worker A's Task Zero; voting itself may slip to a follow-up if it risks GATE 1 (core gate = approved-only board read).
- **Custom domains / vanity slugs** (`feedback.acme.com`): explicitly OUT of this plan (separate feature surfaced in the same investigation).
- **Auto-approve trust levels** (e.g. auto-approve JWT-authed submitters): OUT — v1 is manual approve only; revisit after the queue ships.

## Risks and Mitigations

| Risk | Mitigation |
|---|---|
| Moderation gate bypassed by a code path that reads feedback without the approved filter | `public-board-moderation-gate` oracle (detection-from-code) + SQL-level filter in the repo, not the handler |
| Submitter PII leaks onto the public board | Privacy invariant in C29 + `board_privacy_isolation.rs` + oracle Probe B; document untouchable in module README |
| Existing project unexpectedly exposes feedback on deploy | `public_board_enabled` DEFAULT FALSE + `moderation_status` DEFAULT 'pending' + backfill to pending |
| Overloading triage `status` as visibility | New orthogonal `moderation_status` axis — explicitly separate enum/column |
| Scope creep via voting | Voting flagged deferrable; core GATE 1 is approved-only board read + moderation queue |

## Execution Commands

- **Recommended next**: `/0-uldf-proceed` — at a phase boundary it will read this plan and route Stage 0 (a freeze → likely HERE or a single orchestrated worker), then Stage 1 (PODS fanout). Let it pick topology from context budget.
- Explicit alternatives: `/0-uldf-ltads-start` (sequential/staged driver) → for Stage 1, `/0-uldf-pods-parallelize --from-ldis-plan=<this file>` then `/0-uldf-pods-spawn-collaborator --all`.

═══════════════════════════════════════════════════════════════
