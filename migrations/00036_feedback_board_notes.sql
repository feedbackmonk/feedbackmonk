-- 00036_feedback_board_notes.sql -- the owner's public board note on an item
-- (FR-FBR-57/58, DEC-FBR-22, Contracts C43/C44).
--
-- One plain-text note per feedback item, written by the owner FOR EVERY BOARD
-- VIEWER. It is what the board detail view shows instead of replies: replies
-- (public or internal) are addressed to the submitter and never reach a board
-- surface (C29 invariant 3, FR-FBR-59).
--
-- Deliberately carries no author or submitter column: the board shows the note
-- text and its date, nothing about who wrote it. ON DELETE CASCADE on
-- feedback_id, so erasing the item erases its note (FR-FBR-60). The note is
-- only ever SHOWN for an approved item; the board detail read joins it under the
-- approved-only filter.

CREATE TABLE feedback_board_notes (
    feedback_id UUID PRIMARY KEY REFERENCES feedback(id) ON DELETE CASCADE,
    tenant_id   UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    body        TEXT NOT NULL CHECK (char_length(body) BETWEEN 1 AND 4000),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX feedback_board_notes_project_idx ON feedback_board_notes (project_id);
