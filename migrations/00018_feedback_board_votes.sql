-- 00018_feedback_board_votes.sql -- Public Feedback Board voting (PF-BOARD-VOTING-01)
--
-- Companion to 00016 (feedback moderation + per-project board flags). Owns the
-- voting + double-vote-prevention half of the public feedback BOARD surface —
-- the board sibling of 00007_roadmap_votes.sql (the roadmap voting table).
--
-- A NEW table mirroring `roadmap_votes`, keyed on `feedback_id` (the approved
-- board row) instead of `item_id`. NOT a polymorphic generalization of
-- `roadmap_votes` (DEC-FBR-IMPL-21): keeping the two tables separate leaves the
-- live `roadmap_voting_cache` + roadmap voting path byte-for-byte untouched.
--
-- Lineage:
--   PF-BOARD-VOTING-01 (deferred board-voting follow-up) / DEC-FBR-IMPL-21
--   FR-FBR-26/27 (public feedback board + moderation gate) — votes are a child
--   Plan: docs/planning/plans/20260619T153926-public-board-voting.md (Contract C30)
--   Mirrors migration 00007_roadmap_votes.sql (Contract C14)
--
-- Hard invariants (assertions in repo + handler tests — mirror 00007):
--   1. INSERT with duplicate (feedback_id, voter_id) returns Err(RepoError::Conflict)
--      from BoardVoteRepo::cast -> handler maps to 409 Conflict. NOT silent upsert.
--   2. Anon-mode voter_id is hex(AnonGate::token_hash(ip, cookie, project_id)) --
--      THE canonical chokepoint, NEVER parallel-implemented (00007 inv #2). Board
--      voting REUSES roadmap's resolve_voter (shared voting_common module), so the
--      per-project hash domain that prevents cross-project replay is identical.
--   3. JWT-mode voter_id is the verified `sub` claim from
--      feedbackmonk_jwt::verify_with_leeway (audience checked against project_id).
--   4. Retraction (DELETE) is permitted within RETRACTION_WINDOW_SECS (60s default)
--      of cast_at; after that the handler returns 403. Window check is in the repo,
--      NOT a DB CHECK (the window may flex 30-120s per self-mediation widening).
--   5. ON DELETE CASCADE on feedback_id: deleting a feedback row drops its board
--      votes too. No orphan vote rows.
--   6. Moderation gate (plan D2): the vote handlers resolve the target through an
--      approved-only SQL filter BEFORE any write, so a non-approved / board-disabled
--      feedback row is structurally un-votable. NOT enforced by a CHECK here (the
--      moderation_status lives on `feedback`); enforced in the resolution query +
--      the public-board-moderation-gate oracle Probe B.

CREATE TABLE feedback_board_votes (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id   UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    feedback_id UUID NOT NULL REFERENCES feedback(id) ON DELETE CASCADE,
    voter_id    TEXT NOT NULL,
    voter_mode  TEXT NOT NULL CHECK (voter_mode IN ('jwt','anon')),
    cast_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Q5 drift defender for the "1 vote per (feedback, voter)" rule. INSERT on a
    -- duplicate raises 23505 unique_violation -> RepoError::Conflict (inv #1).
    UNIQUE (feedback_id, voter_id)
);

-- Aggregation query (D1, direct SQL not a cache): `list_public_board` /
-- `get_public_board_item` LEFT JOIN count grouped by feedback_id. The
-- (feedback_id) single-column index supports both the grouping path and the
-- (feedback_id, voter_id) lookup the retract handler does. The UNIQUE constraint
-- already implies a btree on (feedback_id, voter_id) so we don't duplicate that.
CREATE INDEX feedback_board_votes_feedback_id_idx ON feedback_board_votes (feedback_id);

-- Per-project helper (mirror 00007): tenant-scoped reads filter on project_id
-- first; the composite supports a recency-ordered per-project scan.
CREATE INDEX feedback_board_votes_project_cast_idx
    ON feedback_board_votes (project_id, cast_at DESC);
