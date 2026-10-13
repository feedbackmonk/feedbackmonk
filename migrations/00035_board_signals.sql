-- 00035_board_signals.sql -- a signed-in viewer's private board signals:
-- "not needed" marks (FR-FBR-48/49, DEC-FBR-20) and their personal priority
-- list (FR-FBR-50..54, DEC-FBR-18/19). Contract C43 (public) / C44 (owner).
--
-- Both tables key a person by `user_sub`, the verified JWT `sub` (board
-- session or bearer token). Anonymous viewers never write here: an anonymous
-- identity is an IP-and-cookie hash, which DEC-FBR-20 rules out for both
-- signals.
--
-- Hard invariants:
--   1. Moderation gate (FR-FBR-61): every write resolves its target through an
--      approved-only, ranking-pool query BEFORE the insert, in the same
--      transaction (repository `board_signals`). Not a CHECK here, because
--      `moderation_status` lives on `feedback`.
--   2. Ranking pool (FR-FBR-51): approved, kind `feature`, status not in
--      (shipped, wontfix, duplicate). An entry whose item later leaves the pool
--      is excluded from every aggregate by the read queries, and dropped from
--      the user's list the next time they load it.
--   3. Exclusivity (FR-FBR-48): a user's upvote (`feedback_board_votes`,
--      voter_id = sub) and their not-needed mark on the same item never
--      coexist. Each write clears the other in its own transaction.
--   4. Privacy (FR-FBR-49/56): neither table is read by any public surface
--      except for the caller's OWN rows, and the owner sees only aggregates.
--   5. Erasure (FR-FBR-60): ON DELETE CASCADE on feedback_id (erasing an item
--      removes its marks and list entries), and `erase_all_for_end_user`
--      deletes by user_sub (erasing a person removes their marks and list).

CREATE TABLE feedback_not_needed (
    tenant_id   UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    feedback_id UUID NOT NULL REFERENCES feedback(id) ON DELETE CASCADE,
    user_sub    TEXT NOT NULL CHECK (char_length(user_sub) BETWEEN 1 AND 512),
    marked_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- At most one mark per (item, user) (FR-FBR-48).
    PRIMARY KEY (feedback_id, user_sub)
);
-- Owner aggregate (count per item) uses the PK prefix; erasure and the
-- per-viewer state read go by (project_id, user_sub).
CREATE INDEX feedback_not_needed_project_sub_idx ON feedback_not_needed (project_id, user_sub);

CREATE TABLE board_priority_entries (
    tenant_id   UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_sub    TEXT NOT NULL CHECK (char_length(user_sub) BETWEEN 1 AND 512),
    feedback_id UUID NOT NULL REFERENCES feedback(id) ON DELETE CASCADE,
    -- 0-based rank in the user's list; 0 is their top priority.
    position    INTEGER NOT NULL CHECK (position >= 0),
    saved_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- An item appears at most once in a user's list...
    PRIMARY KEY (project_id, user_sub, feedback_id),
    -- ...and each position holds one item. The list is always saved whole
    -- (delete-then-insert in one transaction, FR-FBR-50), so this never needs
    -- to be deferrable.
    UNIQUE (project_id, user_sub, position)
);
-- Aggregation reads every list touching a pool item.
CREATE INDEX board_priority_entries_feedback_idx ON board_priority_entries (feedback_id);
