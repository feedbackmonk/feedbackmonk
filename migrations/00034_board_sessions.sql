-- 00034_board_sessions.sql -- the hosted board's short signed-in session
-- (FR-FBR-44, DEC-FBR-21, Contract C43).
--
-- The board receives a customer-signed end-user JWT exactly once (URL fragment
-- -> POST exchange). The exchange verifies it with the existing verifier and
-- mints a board session: an opaque random token in an HttpOnly, Secure,
-- host-only cookie scoped to one project. This table is the server half.
--
--   token_hash -- sha256 hex of the cookie token. The raw token is never
--                 stored, so a read of this table cannot be replayed as a
--                 session (mirrors password_resets, 00024). Hashing lives in
--                 the API layer; the repository stores an opaque digest.
--   user_sub   -- the verified JWT `sub`, and NOTHING else from the token
--                 (FR-FBR-44): no email, no name, no claims blob.
--   expires_at -- at most 8 hours after created_at; never extended. A fresh
--                 handoff mints a new row (DEC-FBR-21: no refreshable session).
--
-- Erasure (FR-FBR-60): `user_sub` is an end-user identity column, so
-- `erase_all_for_end_user` deletes the sub's sessions in its transaction
-- (feedback-erasure-completeness Probe E). Expired rows are dead weight only;
-- lookups refuse them by `expires_at > now()`, and the purge is opportunistic.

CREATE TABLE board_sessions (
    token_hash  TEXT PRIMARY KEY CHECK (char_length(token_hash) = 64),
    tenant_id   UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_sub    TEXT NOT NULL CHECK (char_length(user_sub) BETWEEN 1 AND 512),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at  TIMESTAMPTZ NOT NULL,
    -- DEC-FBR-21: 8 hours is the ceiling, enforced in the schema as well as in
    -- the repository so no code path can mint a longer-lived bearer.
    CHECK (expires_at > created_at AND expires_at <= created_at + interval '8 hours')
);

-- Erasure + per-user lookups: (project_id, user_sub).
CREATE INDEX board_sessions_project_sub_idx ON board_sessions (project_id, user_sub);
-- Expiry sweep.
CREATE INDEX board_sessions_expires_idx ON board_sessions (expires_at);
