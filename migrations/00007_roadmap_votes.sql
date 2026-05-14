-- 00007_roadmap_votes.sql -- P2 Customer-Facing (Contract C14)
--
-- Companion to 00006 (roadmap_items). Owns the voting + double-vote-prevention
-- half of the public roadmap surface (FR-FBR-13).
--
-- Lineage:
--   FR-FBR-13 (voting + aggregator)
--   Contract C14 (P2 plan §Interface Contracts)
--   docs/planning/handoffs/p2-fanout-contracts.md §C14
--
-- Hard invariants (assertions in repo + handler tests):
--   1. INSERT with duplicate (item_id, voter_id) returns Err(RepoError::Conflict)
--      from RoadmapVoteRepo::cast -> handler maps to 409 Conflict. NOT silent
--      upsert.
--   2. Anon-mode voter_id is hex(AnonGate::token_hash(ip, cookie, project_id))
--      -- canonical chokepoint, NEVER parallel-implement. Per-project hash
--      domain prevents cross-project replay (token_hash mixes project_id).
--   3. JWT-mode voter_id is the verified `sub` claim from
--      feedbackmonk_jwt::verify_with_leeway (audience checked against
--      project_id at verify time).
--   4. Retraction (DELETE) is permitted within RETRACTION_WINDOW_SECS (60s
--      default) of cast_at; after that the handler returns 403. Window check
--      is in the repo, NOT a DB CHECK, because the window may flex 30-120s
--      per pre-authorized self-mediation widening.
--   5. ON DELETE CASCADE on item_id: deleting a roadmap_item drops its
--      votes too. No orphan vote rows.

CREATE TABLE roadmap_votes (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id   UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    item_id     UUID NOT NULL REFERENCES roadmap_items(id) ON DELETE CASCADE,
    voter_id    TEXT NOT NULL,
    voter_mode  TEXT NOT NULL CHECK (voter_mode IN ('jwt','anon')),
    cast_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Q5 drift defender for the "1 vote per (item, voter)" rule. INSERT
    -- on a duplicate raises 23505 unique_violation -> RepoError::Conflict.
    UNIQUE (item_id, voter_id)
);

-- Aggregator query (60s tick): `SELECT item_id, count(*) FROM roadmap_votes
-- WHERE project_id = $1 GROUP BY item_id`. The (item_id) single-column index
-- supports both the grouping path and the (item_id, voter_id) lookup the
-- retract handler does. The UNIQUE constraint already implies a btree on
-- (item_id, voter_id) so we don't duplicate that.
CREATE INDEX roadmap_votes_item_id_idx ON roadmap_votes (item_id);

-- Per-project aggregator helper: the cache refresh tick will fan out across
-- projects, but a single-query "give me vote counts per item, per project"
-- benefits from this composite when the project is highly active.
CREATE INDEX roadmap_votes_project_cast_idx
    ON roadmap_votes (project_id, cast_at DESC);
