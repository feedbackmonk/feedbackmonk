# Intake Assessment
**Source**: /0-uldf-ldis-intake
**Generated**: 2026-06-19T15:34:49
**Task**: feedbackmonk — public board voting (`feedback_board_votes`); design pre-decided per DEC-FBR-IMPL-21 / PF-BOARD-VOTING-01

---

═══════════════════════════════════════════════════════════════
       LEAD DEVELOPER INTELLIGENCE ASSESSMENT
═══════════════════════════════════════════════════════════════

TASK: Wire real end-user voting on the public feedback board. Public Board Stage 1
(commit `5490600`) shipped the board read-only: `vote_count` is a hard-coded `0`
placeholder (`board.rs::VOTE_COUNT_PLACEHOLDER`) and the vote controls in
`PublicBoard.tsx` are unwired. This feature makes votes real.

───────────────────────────────────────────────────────────────
PERCEPTION
───────────────────────────────────────────────────────────────

Type: Enhancement → Capability Extension (completes the FR-FBR-26 public board)
Scope: MEDIUM (one migration + repo methods + 2 endpoints + read-path vote_count + frontend wiring + tests + oracle touch)
Risk: MEDIUM — the vote path is a NEW public surface over the moderation trust boundary; it must NOT become a side-channel that confirms the existence of non-approved feedback.

Professional Assessment:
This is the cleanest, lowest-ambiguity item in the backlog: the design is already
ratified (DEC-FBR-IMPL-21), and there is a byte-for-byte reference implementation to
mirror (`roadmap.rs` voting + migration `00007_roadmap_votes.sql`). The only genuine
net-new thinking is how the vote path interacts with the moderation gate.

───────────────────────────────────────────────────────────────
SPECIFICATION ANALYSIS
───────────────────────────────────────────────────────────────

Coverage: 9/10 dimensions specified (design pre-decided)
Gaps: 0 critical, 0 high, 3 assumable/deferrable

Pre-decided (DEC-FBR-IMPL-21 + PF-BOARD-VOTING-01):
• New `feedback_board_votes` table mirroring `roadmap_votes` (NOT a polymorphic
  generalization — keeps the live `roadmap_voting_cache` untouched).
• Endpoints: `POST` / `DELETE /api/v1/projects/{id}/board/items/{short_code}/vote`
  (board items are addressed by `short_code` = FeedbackId, confirmed in `board.rs`).
• Mirror `roadmap.rs` voter resolution: JWT `sub` (Jwt mode) OR
  `AnonGate::token_hash(ip, cookie, project_id)` (Anon mode) — canonical chokepoints,
  NEVER parallel-implemented. UNIQUE (feedback_id, voter_id) → 409 on double-vote.
  Retraction window (60s default, mirror `RETRACTION_WINDOW_SECS`).
• Wire the deferred `PUBLIC_BOARD_PATHS.vote` stub + vote controls in `PublicBoard.tsx`.

───────────────────────────────────────────────────────────────
CALIBRATION
───────────────────────────────────────────────────────────────

Required Spec Level: Standard (capability extension on a live, well-patterned surface)
Current Spec Level: Standard+ (design ratified, reference impl exists)

VERDICT: SUFFICIENT

───────────────────────────────────────────────────────────────
ENGAGEMENT STRATEGY
───────────────────────────────────────────────────────────────

Questions: 0 blocking (design pre-decided). 3 deferrable sub-decisions for plan/Task Zero:

Deferred Decisions:
| ID | Decision | Default if unresolved | Defer until |
|----|----------|----------------------|-------------|
| D1 | vote_count read path: direct SQL aggregation in `list_public_board`/`get_public_board_item` (LEFT JOIN count) vs. a board-specific cache mirroring `roadmap_voting_cache`. | **Direct SQL aggregation** — the board has no top-voted ranking requirement (unlike the roadmap), so a 60s-tick cache is unjustified complexity; DEC-FBR-IMPL-21 explicitly says leave the roadmap cache untouched. | plan / Worker Task Zero |
| D2 | Does the moderation gate extend to the vote path? A vote/retract on a non-approved or board-disabled item must 404 identically to the read path — otherwise the vote endpoint is an existence oracle for hidden feedback (privacy leak, sibling to the C29 / FR-FBR-27 invariant). | **YES — vote handler hard-filters approved + board-enabled before any write**, same posture as the read path. Strong default; near-mandatory. | plan (becomes an oracle probe) |
| D3 | Should `public-board-moderation-gate` Probe B be extended to assert the vote path also enforces approved-only? | **YES if cheap** — the vote endpoint is a new public board surface; the anti-reward-hacking oracle should cover it, not just the read path. | plan |

Assumptions:
| ID | Assumption | Confidence | Rationale |
|----|------------|-----------|-----------|
| S1 | Voting requires NO change to the moderation state machine or the `feedback` table. | 95% | Votes are an orthogonal child table; FR-FBR-26/27 already DONE. |
| S2 | Anon + JWT voter resolution reuses the existing `AnonGate` / JWT chokepoints verbatim. | 95% | Mandated by DEC-FBR-IMPL-21 + migration 00007 invariant #2/#3. |
| R1 | `board_requires_moderation` (DEC-FBR-IMPL-22, currently inert) stays inert — voting does not depend on it. | 85% | Board hard-filters approved unconditionally today; voting inherits that. |

───────────────────────────────────────────────────────────────
ORACLE CANDIDATES (Proactive Oraculurgy)
───────────────────────────────────────────────────────────────

No NEW oracle needed — but an EXISTING oracle gains scope:
• `public-board-moderation-gate` (already ACTIVE): extend Probe B to assert the
  vote/retract handlers enforce approved-only + board-enabled (D2/D3). This keeps the
  anti-reward-hacking leg covering the full public board surface, not just reads.
    Qualification: deterministic ✓ | recurrent ✓ | freshness-contractable ✓ | gracefully-absent ✓
    Suggested timing: alongside the backend vote endpoints (same stage).

───────────────────────────────────────────────────────────────
COLLABORATION ASSESSMENT
───────────────────────────────────────────────────────────────

Scope: MEDIUM (small end of medium)
Subdivisible: YES (backend / frontend) but modest total volume.

Collaboration Value: Specialization 3, Quality 3, Discovery 2, Speed 2 → 10
Friction: Coupling 4 (frontend depends on the frozen vote wire shape), Boundary Clarity 4 → 8
Net: 10 − (8/2) = 6 → STAGED *or* SEQUENTIAL.

Recommendation: **SEQUENTIAL / HERE (single focused session).** The total surface is one
migration + ~3 repo methods + 2 endpoints + read-path count + one frontend page + tests
+ an oracle probe extension — comfortably one context window. PODS ceremony (4-worker
fanout like Stage 1) is not justified at this size; the backend↔frontend split is better
handled as a thin 2-stage HANDOFF (backend freezes the vote wire shape → frontend wires
it) if anything. A short `/0-uldf-ldis-plan` round will confirm the stage shape.

───────────────────────────────────────────────────────────────
RECOMMENDED NEXT STEPS
───────────────────────────────────────────────────────────────

1. `/0-uldf-ldis-plan "feedbackmonk — public board voting"` — a LIGHT plan: freeze the
   vote wire shape (the one cross-boundary contract), confirm D1 (direct aggregation) +
   D2/D3 (gate extends to vote path), pick stage shape (likely HERE or a thin HANDOFF).
2. Implement: migration `00018_feedback_board_votes.sql` → repo vote/retract/count →
   `board.rs` vote endpoints (reuse AnonGate/JWT chokepoints) → replace
   `VOTE_COUNT_PLACEHOLDER` with the real aggregate → wire `PublicBoard.tsx`.
3. Extend `public-board-moderation-gate` Probe B + add `board_vote_*` tests; `/0-uldf-finalize`.

Note: per PF-BOARD-VOTING-01, this is a clean feature arc — it gets its own intake/plan
(this file) and is NOT grafted onto the sentiment/solicitation work.

═══════════════════════════════════════════════════════════════
