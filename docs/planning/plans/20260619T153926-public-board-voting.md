# Execution Plan
**Source**: /0-uldf-ldis-plan
**Generated**: 2026-06-19T15:39:26
**Task**: feedbackmonk — public board voting (`feedback_board_votes`)
**Strategy**: SEQUENTIAL (single focused session)
**Intake Source**: docs/planning/intakes/20260619T153449-public-board-voting-feedback-board-votes.md

---

═══════════════════════════════════════════════════════════════
       LDIS EXECUTION PLAN
═══════════════════════════════════════════════════════════════

Task: Wire real end-user voting on the public feedback board, replacing the
`vote_count = 0` placeholder shipped in Public Board Stage 1 (`5490600`).
Design pre-decided: DEC-FBR-IMPL-21 / PF-BOARD-VOTING-01.

Strategy: SEQUENTIAL

## Strategy Rationale

One coherent feature: migration → repo → endpoints → vote_count read path →
frontend wiring → tests → oracle extension. The frontend is a thin consumer that
depends on the frozen vote wire shape (Contract C30 below), so a backend↔frontend
split would force a sync point with no real parallelism payoff. Intake's
Collaboration Value Assessment scored Net 6 → SEQUENTIAL/HERE. Total volume is one
migration + ~3 repo methods + one shared-helper extraction + 2 endpoints + a
read-path aggregation + one frontend page + tests — comfortably one context window.

**CTD**: CTD-07 short-circuit → traditional (no tiering, no CTD artifact). Single
domain, fits one context, no genuine 3-way independent parallelism (frontend gated
on backend).

## Context Budget Assessment

Single agent, single session. ~one migration + repo + 2 handlers + frontend page +
tests. Well under 85%. No decomposition needed.

## The one real implementation decision (NOT a mirror — a reuse)

**Share the voter-resolution chokepoint; do NOT re-implement it.** Migration
`00007_roadmap_votes.sql` invariant #2 is explicit: the anon `voter_id =
hex(AnonGate::token_hash(ip, cookie, project_id))` is a **canonical chokepoint,
NEVER parallel-implemented** (per-project hash domain prevents cross-project
replay). The roadmap vote handlers (`roadmap.rs::resolve_voter` /
`resolve_voter_no_rate_limit`, plus the `jwt_error_response` / `rate_limited_response`
/ `already_voted_response` / vote response helpers) currently live private to
`roadmap.rs`.

→ **Extract the voter-resolution + shared vote-response helpers into a common module**
(`handlers/voting_common.rs` or `auth/voter.rs`) consumed by BOTH `roadmap.rs` and
`board.rs`. Board voting reuses `resolve_voter` verbatim — same AnonGate chokepoint,
same JWT verify, same rate-limit + retraction-window semantics. This is the load-
bearing "don't duplicate the security primitive" decision; everything else is
mechanical mirroring of the `roadmap_votes` shape onto `feedback_board_votes`.

## Confirmed deferred-decisions (from intake)

- **D1 — vote_count read path: DIRECT SQL AGGREGATION (confirmed).** `list_public_board`
  / `get_public_board_item` populate `vote_count` via a `LEFT JOIN` count against
  `feedback_board_votes` (grouped by feedback_id), NOT a roadmap-style 60s cache. The
  board has no top-voted ranking requirement; a cache is unjustified complexity and
  DEC-FBR-IMPL-21 says leave `roadmap_voting_cache` untouched. `BoardItem` repo struct
  gains `vote_count: i64`; `board.rs::item_response` reads it instead of
  `VOTE_COUNT_PLACEHOLDER` (delete the constant).

- **D2 — moderation gate EXTENDS to the vote path (confirmed, load-bearing).** A
  vote/retract on a non-approved or board-disabled item MUST 404 identically to the
  read path — otherwise the vote endpoint is an existence-oracle for hidden feedback
  (privacy leak, sibling to C29 / FR-FBR-27). Enforce STRUCTURALLY, not via a handler
  flag: the vote handlers run `open_for_submission` → `ensure_board_enabled` → resolve
  the target via a repo method that hard-filters `moderation_status='approved'`
  (returns `NotFound` for non-approved) BEFORE any vote write. The approved-only
  filter lives in SQL (same posture as `get_public_board_item`), so it cannot be
  reward-hacked by a self-reported column.

- **D3 — oracle Probe B EXTENDS to cover the vote path (confirmed).** Extend
  `public-board-moderation-gate` Probe B to assert the vote/retract handlers route
  through `ensure_board_enabled` + an approved-only resolution (cannot vote on a
  pending/rejected/board-disabled item). Keeps the anti-reward-hacking leg covering
  the FULL public board surface, not just reads.

## Interface Contract — C30 (frozen here; backend ↔ frontend ↔ oracle)

```
POST   /api/v1/projects/{project_id}/board/items/{short_code}/vote
DELETE /api/v1/projects/{project_id}/board/items/{short_code}/vote     (retract)
```
CORS-exposed (added to `board_router`, which is already merged WITH `.layer(cors)`).
Voter resolution reuses the shared chokepoint (JWT `sub` → Jwt mode; else
`AnonGate::token_hash(ip, cookie, project_id)` → Anon mode; anon cookie minted/echoed
via `Set-Cookie` exactly as roadmap does).

Gate (D2): each handler resolves the target by `short_code` through the approved-only
+ board-enabled path FIRST. Non-approved / board-disabled / unknown → **404** (no
existence signal).

Responses (mirror roadmap vote responses):
- POST 200 `{ short_code, voter_mode: "jwt"|"anon", cast_at }`
- POST 409 on duplicate `(feedback_id, voter_id)` (UNIQUE violation → RepoError::Conflict)
- DELETE 200 `{ short_code, retracted_at }` within RETRACTION_WINDOW (60s default)
- DELETE 404 if no vote row; DELETE 403 if past the retraction window
- 401/403 on JWT errors; 429 on anon rate-limit (cast only) — reuse roadmap helpers

Read shape unchanged except `vote_count` now reflects the real aggregate (D1).

## Component Breakdown (sequential order)

1. **Migration `00018_feedback_board_votes.sql`** — mirror `00007_roadmap_votes.sql`,
   keyed on `feedback_id` (FK → `feedback(id)` ON DELETE CASCADE) instead of `item_id`;
   `tenant_id` + `project_id` scoping columns; `voter_id TEXT`, `voter_mode CHECK ('jwt','anon')`,
   `cast_at TIMESTAMPTZ`; `UNIQUE (feedback_id, voter_id)`; index `(feedback_id)` +
   `(project_id, cast_at DESC)`.
2. **Repo (`feedbackmonk-repository`)** — `BoardVoteRepo` (`cast` / `retract` mirroring
   `RoadmapVoteRepo`, same `RepoError::Conflict` + `RetractOutcome` + window semantics);
   add `vote_count` to `BoardItem` + the `LEFT JOIN` count in `list_public_board` /
   `get_public_board_item` (D1); a tenant-scoped "resolve approved board feedback by
   short_code" used by the vote handlers (D2). Regenerate `.sqlx/`.
3. **Shared voter helper** — extract `resolve_voter` / `resolve_voter_no_rate_limit` +
   vote-response helpers out of `roadmap.rs` into a shared module; repoint roadmap to it
   (no behavior change — pure move + re-export). Board consumes it.
4. **Board endpoints (`board.rs`)** — add POST/DELETE vote routes to `board_router`;
   handlers per C30; delete `VOTE_COUNT_PLACEHOLDER`, read real `vote_count`.
5. **Oracle** — extend `public-board-moderation-gate` Probe B (D3) + bump manifest patch.
6. **Tests** — `board_vote.rs` (cast/retract/double-vote-409/window-403/anon+JWT) +
   `board_vote_moderation_gate.rs` (cannot vote on pending/rejected/board-disabled → 404).
7. **Frontend (`admin-ui/`)** — wire `PUBLIC_BOARD_PATHS.vote` stub + `ApiClient`
   castBoardVote / retractBoardVote + `types.gen.ts` vote response types + vote controls
   in `pages/board/PublicBoard.tsx` (mirror the roadmap vote button pattern); a11y stays
   green.

## Oracle Pre-Build Plan

| Oracle | Question | Consumer | Timing | Status |
|---|---|---|---|---|
| `public-board-moderation-gate` (EXISTING, ACTIVE) | Does the vote path also enforce approved-only + board-enabled? (D3) | this feature's exit gate | extend Probe B alongside the backend vote endpoints (step 5) | live, gains scope |

No NEW oracle. Rationale: the vote endpoint is a new surface over the SAME trust
boundary the existing oracle already defends; extending Probe B is cheaper and keeps
one gate authoritative.

## Testability Gate Findings

| Item | Q1 | Q2 | Q3 | Q4 | Q5 | Composite | Flag |
|---|---|---|---|---|---|---|---|
| **Vote path over the moderation boundary** (no vote/retract on non-approved/board-disabled) | 2 | **5** | 4 | 5 | 3 | 19 | **FLAGGED — Q2=5** |
| Vote cast/retract mechanics (409/403/window) | 2 | 2 | 2 | 2 | 2 | 10 | not flagged |

**Q2=5 mitigation (mandatory):** identical to the read-path finding — a handler-side
or self-reported `is_approved` check is the reward-hacking surface. Enforce the gate
STRUCTURALLY (approved-only resolution in SQL, before any write) AND cover it with the
`public-board-moderation-gate` Probe B extension (detection-from-code) + the
`board_vote_moderation_gate.rs` integration test. Q4=5: the existing oracle is the
highest-leverage scaffold and already exists — extending it halves verification cost.

## Ripple Analysis

| Modified interface | Consumers | Impact |
|---|---|---|
| `roadmap.rs` voter helpers → shared module | `roadmap.rs` only (today) | Pure extraction + re-export; roadmap behavior byte-identical. Run roadmap vote tests to prove no regression. |
| `BoardItem` (+`vote_count`) | `board.rs::item_response`, board read SQL | Additive field; read SQL gains a LEFT JOIN. |
| `feedback` table | new child `feedback_board_votes` (FK CASCADE) | No change to `feedback`; deleting feedback drops its votes. |
| `board_router` (+2 routes) | `main.rs::build_app` | New routes inherit the existing `.layer(cors)` — same credentialed CORS posture; `cors-allowlist-enforcement` stays green. |
| `.sqlx/` offline cache | build | Regenerate + commit. |

Blast radius: **🟡 Medium** — additive schema + new endpoints over a load-bearing
privacy/trust boundary, so the oracle Probe B extension + the moderation-gate test are
required gates. The voter-helper extraction is the one refactor touching live roadmap
code — guarded by the existing roadmap vote tests.

## Risks and Mitigations

| Risk | Mitigation |
|---|---|
| Vote path leaks existence of hidden feedback | D2 structural gate (approved-only SQL resolution before write) + oracle Probe B + `board_vote_moderation_gate.rs`; 404 (not 403) on non-approved. |
| Anon-vote chokepoint duplicated → cross-project replay hole | Reuse `AnonGate::token_hash` via the shared module; never re-implement (00007 inv #2). |
| Voter-helper extraction regresses roadmap voting | Pure move + re-export; rerun roadmap vote integration tests as the regression gate. |
| `board_requires_moderation` (inert, DEC-FBR-IMPL-22) confusion | Out of scope; board hard-filters approved unconditionally — voting inherits that. Note in board README. |

## Coordination Requirements

Single session, self-verified exit gate: `cargo build --workspace` + `clippy --tests
-D warnings` clean; `board_vote*` + roadmap vote tests pass; `public-board-moderation-gate`
(Probe A+B+C) + `cors-allowlist-enforcement` + `multi-tenant-isolation-check` GREEN;
admin-ui tsc + vitest + board a11y green; `.sqlx/` regenerated. Then `/0-uldf-finalize`.

## Execution Commands

- **Recommended next**: `/0-uldf-proceed` — at this boundary it will route implementation.
  Given modest scope + 1M context, it will likely keep it HERE (single session) or, if
  context is tighter, HANDOFF to one fresh implementer carrying this plan.
- Per PF-BOARD-VOTING-01: this is a clean feature arc — implement against this plan; do
  NOT graft onto the sentiment/solicitation work.

═══════════════════════════════════════════════════════════════
