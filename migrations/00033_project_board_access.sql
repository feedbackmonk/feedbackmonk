-- 00033_project_board_access.sql -- who may see a project's board (FR-FBR-43,
-- DEC-FBR-21, Contract C44).
--
--   projects.board_access -- `public` (default: today's behaviour, anyone may
--                            read the board and upvote anonymously) or
--                            `signed_in` (every board read and write needs a
--                            signed-in viewer: a verified end-user JWT or a
--                            board session, migration 00034). Only the owner
--                            changes it (admin board-settings PATCH).
--
-- Orthogonal to `public_board_enabled` (00016): a disabled board 404s whatever
-- this says; this only narrows WHO sees an enabled one. The two refusals stay
-- distinguishable on the wire (404 vs the sign-in response, Contract C43).
--
-- NOT NULL DEFAULT 'public' so every existing project keeps exactly its current
-- behaviour. The vocabulary is closed (CHECK), mirrored by
-- `feedbackmonk_core::BoardAccess`.

ALTER TABLE projects
    ADD COLUMN board_access TEXT NOT NULL DEFAULT 'public'
        CHECK (board_access IN ('public', 'signed_in'));

COMMENT ON COLUMN projects.board_access IS
    'FR-FBR-43: public (anyone) | signed_in (verified end-user JWT or board session required for every board read and write). Owner-only setting.';
