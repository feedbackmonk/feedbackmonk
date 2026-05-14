-- 00003_feedback_status_history.sql -- P1 Stage 1 (FR-FBR-08 audit history)
--
-- Cohesion decision (DEC-FBR-IMPL-PI-S1-01): this single migration adds BOTH
-- (a) the `feedback.status` column (with CHECK against the six canonical
-- kebab-case status values from Contract C6) AND (b) the
-- `feedback_status_history` audit table. The brief enumerated only the
-- audit table for 00003 and deferred the column to Stage 2 Worker A's
-- transition handler, but the C6-backing repository methods (`list_for_admin`
-- with status_filter, `get_with_history` returning `Feedback { status, .. }`)
-- need the column to exist NOW. Keeping the status-workflow feature
-- cohesive in one migration removes a forced Stage 2 widening and lets
-- Stage 2 Worker A's transition handler land as pure application logic
-- against an already-shaped schema.
--
-- Status workflow state machine (Contract C6):
--   submitted -> triaged -> in-progress -> {shipped, wontfix, duplicate}
-- Every transition handler in Stage 2 Worker A writes BOTH this column
-- and an audit row in the same transaction (Hard Invariant #4).
--
-- Audit-row atomicity (Contract C6 Hard Invariant #4): same-transaction
-- writes to `feedback.status` + this table. This migration supports — but
-- does not enforce — that property; the discipline lives in the handler.
--
-- `transitioned_by` is a UUID without a foreign key. P0 has no
-- `tenant_users` table; Stage 2 Worker A introduces one and may add a
-- deferred FK in a later migration. Storing a bare UUID for now keeps the
-- table forward-compatible without blocking Stage 1 on out-of-scope schema.
--
-- `duplicate_of_feedback_id` references `feedback.id` (the UUID PK, not
-- the public `short_code`). Cascade on delete: ON DELETE SET NULL so that
-- deleting a "duplicate target" feedback row doesn't cascade-delete the
-- audit history of the row that was marked as its duplicate.
--
-- Lineage:
--   FR-FBR-08 (status workflow + audit trail)
--   Contract C6 (P1 plan §Interface Contracts)
--   DEC-FBR-03 (sole query path through feedbackmonk-repository)
--   GitCellar peer reference: gitcellar-cloud/src/feedback/db.rs
--
-- Idempotency: standard sqlx migrator semantics — each migration runs
-- exactly once. Re-running this file is a no-op (sqlx tracks applied
-- migrations in `_sqlx_migrations`).
--
-- DEVIATION FROM BRIEF (documented in development-complete.md): the brief
-- enumerates only the `feedback_status_history` table for migration 00003,
-- but Contract C6 backing methods (FeedbackRepo::list_for_admin's
-- status_filter, get_with_history's status field on Feedback) need a
-- `feedback.status` column to exist. Adding the column here keeps the
-- "status workflow" feature cohesive in one migration and avoids forcing
-- Stage 2 Worker A to widen Stage 1's repository surface (a wash on
-- pre-authorized self-mediation, but with smaller blast radius if Worker A
-- chooses a different column name or constraint).

-- Feedback status column. Default 'submitted' so existing P0 rows retain
-- semantics. Stage 2 Worker A's transition handler writes this column in
-- the same transaction as the audit-row insert (Contract C6 Hard Invariant
-- #4).
ALTER TABLE feedback
    ADD COLUMN status TEXT NOT NULL DEFAULT 'submitted'
    CHECK (status IN ('submitted','triaged','in-progress','shipped','wontfix','duplicate'));

-- Admin list view: `WHERE project_id = $1 AND status = $2` benefits from a
-- composite index covering the status filter + the accepted_at sort.
CREATE INDEX feedback_project_status_idx
    ON feedback (project_id, status, accepted_at DESC);

CREATE TABLE feedback_status_history (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    feedback_id UUID NOT NULL REFERENCES feedback(id) ON DELETE CASCADE,
    from_status TEXT NOT NULL,
    to_status TEXT NOT NULL,
    reason_note TEXT,
    duplicate_of_feedback_id UUID REFERENCES feedback(id) ON DELETE SET NULL,
    transitioned_by UUID NOT NULL,
    transitioned_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Drawer audit-row query: list a feedback's history newest-first.
CREATE INDEX feedback_status_history_feedback_idx
    ON feedback_status_history (feedback_id, transitioned_at DESC);

-- Defensive: a row should never claim itself as its own duplicate target.
-- Stage 2 Worker A's handler enforces this at the application layer
-- (Contract C6 TransitionError::DuplicateSelfReference); the CHECK is
-- belt-and-braces.
ALTER TABLE feedback_status_history
    ADD CONSTRAINT feedback_status_history_no_self_duplicate
    CHECK (duplicate_of_feedback_id IS NULL OR duplicate_of_feedback_id <> feedback_id);
