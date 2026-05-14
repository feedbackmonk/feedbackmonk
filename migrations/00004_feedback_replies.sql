-- 00004_feedback_replies.sql -- P1 Stage 2 (FR-FBR-08 reply + FR-FBR-09 public reply email)
--
-- Adds the `feedback_replies` table backing Contract C7's
-- `POST /api/v1/admin/feedback/{id}/reply` endpoint. Replies have two
-- visibility levels:
--   - 'public'   -> visible to the submitter; triggers a `PublicReplyEmail`
--                   send if the submitter has an email on file (auth-mode
--                   submission, or anonymous-with-optional-email).
--   - 'internal' -> admin-only; never emailed, never surfaced to submitters
--                   in current scope. Reserved for triage notes.
--
-- The body length window mirrors the submission body window from migration
-- 00001 (`feedback.body BETWEEN 1 AND 16384`) so reviewers cannot exceed
-- the same envelope the submitter operates within.
--
-- `author_user_id` is a bare UUID with NO foreign key. P0 has no
-- `tenant_users` table (see migration 00003's commentary); reviewers in
-- P0/P1 are tenants themselves, and `author_user_id` equals the
-- transitioning tenant's id in practice. A future migration introduces
-- `tenant_users` and adds the FK via `ALTER TABLE ... ADD CONSTRAINT ...
-- NOT VALID; VALIDATE CONSTRAINT ...` for online-safe rollout.
--
-- `ON DELETE CASCADE` on `feedback_id`: deleting a feedback row should
-- delete its replies (no orphan rows in the audit/admin views).
--
-- Lineage:
--   FR-FBR-08 (reply on admin feedback detail)
--   FR-FBR-09 (public-reply email)
--   Contract C7 (P1 plan §Interface Contracts)
--   P1 Stage 1->2 handoff doc §Migration numbering (00004 reserved)
--
-- Idempotency: standard sqlx migrator semantics — runs exactly once.

CREATE TABLE feedback_replies (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    feedback_id UUID NOT NULL REFERENCES feedback(id) ON DELETE CASCADE,
    body TEXT NOT NULL CHECK (char_length(body) BETWEEN 1 AND 16384),
    visibility TEXT NOT NULL CHECK (visibility IN ('public', 'internal')),
    -- No FK yet; tenant_users table does not exist in P0/P1 (see migration
    -- 00003 commentary). Stored as bare UUID; future migration adds the FK.
    author_user_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Drawer detail query: list replies for a feedback row, oldest-first
-- (chronological conversation order).
CREATE INDEX feedback_replies_feedback_idx
    ON feedback_replies (feedback_id, created_at);
