-- 00022_idempotency_identity_scope.sql -- close the cross-user idempotency-key
-- collision (security scrutiny P1-3 + M6/P2-17).
--
-- ## The defect (P1-3)
--
-- Migration 00021 keyed submit dedupe on `(project_id, idempotency_key)` with NO
-- submitter dimension. Two DIFFERENT end-users who happen to choose the same key
-- ("1", "retry", a per-client counter) collided: the second user's feedback row
-- was silently ROLLED BACK and they received the FIRST user's `feedback_id` —
-- cross-user SILENT DATA LOSS plus a `short_code` leak across the trust boundary.
--
-- ## The fix
--
-- Add a `submitter_id` to the primary key so dedupe is scoped per
-- `(project_id, submitter_id, idempotency_key)`. `submitter_id` is the
-- end-user's JWT `sub` (auth mode) or the hex of the anon token hash (anon
-- mode) — the same identity the feedback row is attributed to. Two different
-- submitters reusing a key now each get their own row; a key only dedupes
-- within one submitter's own retries.
--
-- ## M6/P2-17 — content-aware retry + key length bound
--
-- Add `content_hash` (BLAKE3 of a stable serialization of body+sentiment+
-- severity+kind) so the repository can tell a legit retry (identical content →
-- return the original id) from a key REUSED with different content (→ hard 409
-- `IdempotencyKeyReuse`, instead of 00021's silent "return the original"). And
-- bound the previously-unbounded `idempotency_key` TEXT to 1..=255 chars.
--
-- `content_hash` is nullable: rows created before this migration carry NULL, but
-- they also carry the `''` backfill `submitter_id`, so a post-migration submit
-- (always a non-empty submitter_id) never resolves an old row.
--
-- ## Multi-tenant isolation (DEC-FBR-03)
--
-- Unchanged: every query stays scoped by `(tenant_id, project_id)`. The PK
-- constraint NAME (`submit_idempotency_pkey`) is preserved so the repository's
-- typed conflict detector (`is_submit_idempotency_conflict`) keeps matching.
--
-- Idempotency: standard sqlx migrator semantics — runs exactly once.

ALTER TABLE submit_idempotency
    ADD COLUMN submitter_id TEXT NOT NULL DEFAULT '';

ALTER TABLE submit_idempotency
    ADD COLUMN content_hash BYTEA;

ALTER TABLE submit_idempotency
    ADD CONSTRAINT submit_idempotency_key_len
    CHECK (char_length(idempotency_key) BETWEEN 1 AND 255);

ALTER TABLE submit_idempotency
    DROP CONSTRAINT submit_idempotency_pkey;

ALTER TABLE submit_idempotency
    ADD CONSTRAINT submit_idempotency_pkey
    PRIMARY KEY (project_id, submitter_id, idempotency_key);

-- The DEFAULT existed only to backfill the pre-existing rows above; new inserts
-- MUST supply the real submitter identity.
ALTER TABLE submit_idempotency
    ALTER COLUMN submitter_id DROP DEFAULT;
