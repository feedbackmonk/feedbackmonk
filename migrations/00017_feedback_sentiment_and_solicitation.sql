-- 00017_feedback_sentiment_and_solicitation.sql
--
-- Two capabilities requested by GitCellar (customer #1) for its in-app feedback
-- solicitation prompt. Designed NATIVELY for feedbackmonk's multi-tenant /
-- multi-product model, not GitCellar-specifically.
--
--   Capability 1 (FR-FBR-28) — first-class sentiment on a submission, and
--     sentiment-only submissions (body becomes optional when sentiment present).
--   Capability 2 (FR-FBR-29) — durable per-user solicitation/suppression state,
--     keyed by the end-user's stable JWT `sub` (per project) so it survives a
--     client reinstall.
--
-- Lineage:
--   docs/integrations/gitcellar-adoption.md §8 (capability negotiation)
--   docs/specs/SPECIFICATION.md FR-FBR-28 / FR-FBR-29
--   docs/specs/DECISIONS.md DEC-FBR-IMPL-23 (sentiment storage),
--                           DEC-FBR-IMPL-24 (solicitation as a first-class feature)
--   DEC-FBR-03 (sole query path through feedbackmonk-repository)
--
-- Tenant isolation (DEC-FBR-03): adds NO new raw-SQL query path outside the
-- repository crate. `feedback_solicitations` carries both `tenant_id` and
-- `project_id`; every read/write in `SqlxSolicitationRepo` filters on both.
--
-- Idempotency: standard sqlx migrator semantics — runs exactly once.

-- ============================================================================
-- Capability 1: sentiment + optional body
-- ============================================================================

-- The original `feedback.body` was `TEXT NOT NULL CHECK (length 1..16384)`
-- (migration 00001, constraint `feedback_body_check`). A sentiment-only
-- submission has no body, so `body` becomes nullable. When present it keeps the
-- 1..16384 length bound, so `body IS NULL` (and ONLY null) means "no body" —
-- the repository maps NULL -> "" for the domain `Feedback.body: String`.
ALTER TABLE feedback ALTER COLUMN body DROP NOT NULL;
ALTER TABLE feedback DROP CONSTRAINT IF EXISTS feedback_body_check;
ALTER TABLE feedback ADD CONSTRAINT feedback_body_len_check
    CHECK (body IS NULL OR (char_length(body) BETWEEN 1 AND 16384));

-- First-class 3-point sentiment. Nullable; CHECK mirrors `feedback_kind_check`.
ALTER TABLE feedback ADD COLUMN sentiment TEXT
    CHECK (sentiment IS NULL OR sentiment IN ('negative', 'neutral', 'positive'));

-- A submission must carry at least a body OR a sentiment. This is the structural
-- guarantee that backs the API-layer "body or sentiment required" 400.
ALTER TABLE feedback ADD CONSTRAINT feedback_body_or_sentiment_check
    CHECK (body IS NOT NULL OR sentiment IS NOT NULL);

-- Index for the admin "satisfaction trend over time" aggregation
-- (GET /api/v1/admin/feedback/sentiment-trend): scoped scans bucketed by time,
-- filtered to rows that carry a sentiment.
CREATE INDEX feedback_project_sentiment_idx
    ON feedback (project_id, accepted_at)
    WHERE sentiment IS NOT NULL;

-- Note: `body_tsv` (migration 00011) is `GENERATED ALWAYS AS
-- (to_tsvector('english', body)) STORED`. With `body` now nullable,
-- `to_tsvector('english', NULL)` yields NULL — the column was already nullable
-- and the GIN index simply never matches a sentiment-only (body-less) row.
-- No change required; full-text search correctly ignores body-less rows.

-- ============================================================================
-- Capability 2: durable per-user solicitation state
-- ============================================================================

-- One row per (project, end-user JWT sub). The state machine + transition
-- legality live in `feedbackmonk-core::solicitation`; this table is pure
-- storage. The frequency-cap (cooldown) POLICY is computed at the API layer
-- from `prompted_at`, so the cap can be tuned without a schema change.
CREATE TABLE feedback_solicitations (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id     UUID NOT NULL REFERENCES tenants(id)  ON DELETE CASCADE,
    project_id    UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    -- The end-user's stable JWT `sub`. NOT an anon token: solicitation state is
    -- JWT-only (a durable record requires a stable identity; DEC-FBR-04).
    end_user_sub  TEXT NOT NULL,
    status        TEXT NOT NULL DEFAULT 'eligible'
                  CHECK (status IN ('eligible', 'prompted', 'dismissed', 'gave_feedback', 'opted_out')),
    -- How many times this sub has been prompted (lifetime). Drives nothing in
    -- the DB; surfaced to the consumer for its own telemetry/cap logic.
    prompt_count  INTEGER NOT NULL DEFAULT 0 CHECK (prompt_count >= 0),
    -- Timestamp of the most recent `prompted` event. NULL = never prompted.
    -- The cooldown/eligibility computation keys off this.
    prompted_at   TIMESTAMPTZ,
    last_event_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- One record per (project, sub). The repository upserts on this key.
    UNIQUE (project_id, end_user_sub)
);

CREATE INDEX feedback_solicitations_tenant_idx
    ON feedback_solicitations (tenant_id);
