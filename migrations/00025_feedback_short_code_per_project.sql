-- 00025_feedback_short_code_per_project.sql -- shrink the short_code collision
-- space from global to per-project (security scrutiny P1-5).
--
-- ## The defect (P1-5)
--
-- `feedback.short_code` is a 6-char code over a 32-symbol alphabet
-- (32^6 ~= 1.07e9) minted into a GLOBALLY UNIQUE column. As the whole table
-- grows the birthday-bound collision probability on a single INSERT rises
-- (~1% at ~10M total rows) — and the submit path did not catch the unique
-- violation, so a collision surfaced as an HTTP 500. The idempotency retry
-- regenerates a fresh code, so it did not self-heal.
--
-- ## The fix
--
-- Uniqueness only ever needs to hold WITHIN a project: every `short_code`
-- lookup in the repository is already scoped by `(tenant_id, project_id,
-- short_code)`, so no read assumed global uniqueness. Dropping the global
-- UNIQUE and adding `UNIQUE (project_id, short_code)` shrinks the collision
-- space to a single project's row count — astronomically smaller than the
-- global table. The repository ALSO keeps a bounded regenerate-on-conflict
-- retry loop as defense-in-depth (feedback.rs `is_short_code_conflict` matches
-- THIS constraint by name).
--
-- ## No FK / data impact
--
-- Nothing references `feedback(short_code)` (all FKs target `feedback(id)`), so
-- dropping the old constraint is safe. Existing rows already satisfy the
-- stronger-scoped constraint (global-unique ⇒ per-project-unique), so the new
-- UNIQUE index builds without conflict.
--
-- The new constraint NAME (`feedback_project_short_code_key`) is referenced by
-- the repository's typed conflict detector — do not rename without updating it.
--
-- Idempotency: standard sqlx migrator semantics — runs exactly once.

ALTER TABLE feedback
    DROP CONSTRAINT feedback_short_code_key;

ALTER TABLE feedback
    ADD CONSTRAINT feedback_project_short_code_key UNIQUE (project_id, short_code);
