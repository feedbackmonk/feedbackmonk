-- 00011_feedback_fts.sql -- GitCellar parity gap #3 (admin full-text search)
--
-- Adds full-text search over feedback bodies so the admin console can search
-- across feedback (not just status-filter + paginate). This closes parity
-- gap #3 from the GitCellar customer-#1 adoption checklist
-- (docs/integrations/gitcellar-adoption.md + the parity-status oracle).
--
-- Design:
--   * A STORED generated `tsvector` column `body_tsv` derived from `body`.
--     Generated-always keeps the vector in lockstep with `body` with zero
--     application-layer maintenance and no UPDATE trigger (mirrors the
--     "no generic UPDATE trigger" convention established in 00006).
--   * `to_tsvector('english', body)` is IMMUTABLE, which is the requirement
--     for use inside a STORED generated column. The feedback row has no
--     `subject` column (see 00001/00003 schema), so the vector covers `body`
--     only — the single free-text field on the row.
--   * A GIN index over `body_tsv` backs the `@@` match. Query-time uses
--     `websearch_to_tsquery('english', $q)` (GUIDE §step 1) so admins get
--     forgiving Google-style query syntax (quoted phrases, `-exclude`, `or`)
--     without the parse-error surface of `to_tsquery`.
--
-- Tenant isolation (DEC-FBR-03): this migration adds NO new query path. The
-- search query lives in `feedbackmonk-repository::FeedbackRepo::search_for_admin`
-- and carries the SAME `WHERE tenant_id = $1 AND project_id = $2` scope clause
-- as every other feedback read. The `multi-tenant-isolation-check` oracle runs
-- against this migration; the column/index are tenant-agnostic storage and the
-- scope filter is enforced at the repository layer (the sole query path).
--
-- Lineage:
--   GitCellar parity gap #3 (admin full-text search)
--   docs/planning/plans/20260602T121500-gitcellar-customer-1-enablement.md
--   DEC-FBR-03 (sole query path through feedbackmonk-repository)
--   PODS session collab-20260602-123000 (frozen migration number: CHARLIE = 00011)
--
-- Idempotency: standard sqlx migrator semantics — runs exactly once
-- (tracked in `_sqlx_migrations`).

-- Generated tsvector over the feedback body. NOT NULL is implicit: body is
-- NOT NULL, and to_tsvector of a non-null text is non-null.
ALTER TABLE feedback
    ADD COLUMN body_tsv tsvector
    GENERATED ALWAYS AS (to_tsvector('english', body)) STORED;

-- GIN index backs the `body_tsv @@ websearch_to_tsquery(...)` match in
-- FeedbackRepo::search_for_admin. The repository scopes every search by
-- (tenant_id, project_id); Postgres combines this GIN index with the existing
-- tenant/project predicates via bitmap-AND.
CREATE INDEX feedback_body_tsv_idx
    ON feedback USING GIN (body_tsv);
