-- 00006_roadmap_items.sql -- P2 Customer-Facing (Contract C13)
--
-- Owns the schema half of the public roadmap surface (FR-FBR-11). The
-- companion vote table lands in 00007.
--
-- Lineage:
--   FR-FBR-11 (public roadmap)
--   FR-FBR-12 (promote-to-roadmap; idempotency via origin_feedback_id UNIQUE)
--   Contract C13 (P2 plan §Interface Contracts)
--   DEC-FBR-03 (sole query path through feedbackmonk-repository)
--   docs/planning/handoffs/p2-fanout-contracts.md §C13
--
-- Status state machine (no audit-history table; admin edits freely):
--   considering -> planned -> in-progress -> shipped     (forward path)
--   any         -> wontfix                                (close)
--   wontfix     -> considering                            (re-open via admin edit)
--   shipped     -- terminal
--
-- Two UNIQUE constraints carry load-bearing semantics:
--   - (project_id, slug): URL component is unique per project.
--   - origin_feedback_id: enforces "one roadmap item per source feedback"
--     idempotency for the promote handler (Contract C16 invariant 4).
--
-- `updated_at` policy: feedbackmonk doesn't ship a generic UPDATE trigger.
-- The repository's `update()` method sets `updated_at = now()` explicitly in
-- the UPDATE SQL (mirrors P0/P1 convention of keeping timestamp mutation
-- visible at the call site).
--
-- `created_by` is a UUID without a foreign key, mirroring
-- `feedback_status_history.transitioned_by` (P1 Stage 1 precedent). A
-- `tenant_users` table is deferred; the bare UUID lets the audit identity
-- exist without forcing schema for the user-membership concept this phase.

CREATE TABLE roadmap_items (
    id                 UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id          UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    project_id         UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    slug               TEXT NOT NULL,
    title              TEXT NOT NULL CHECK (char_length(title) BETWEEN 1 AND 200),
    body               TEXT NOT NULL CHECK (char_length(body) BETWEEN 1 AND 16384),
    status             TEXT NOT NULL DEFAULT 'considering'
                         CHECK (status IN ('considering','planned','in-progress','shipped','wontfix')),
    -- Nullable: admin can create roadmap items from scratch (no source feedback).
    -- ON DELETE SET NULL so that hard-deleting a source feedback (not currently
    -- supported in feedbackmonk, but defensive) doesn't cascade-drop the
    -- promoted roadmap item.
    origin_feedback_id UUID REFERENCES feedback(id) ON DELETE SET NULL,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by         UUID NOT NULL,
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, slug),
    UNIQUE (origin_feedback_id)
);

-- Listing index: public + admin list views filter by (tenant_id, project_id)
-- and optionally by status, ordered by created_at DESC.
CREATE INDEX roadmap_items_tenant_project_status_idx
    ON roadmap_items (tenant_id, project_id, status);

-- Idempotent-promote lookup helper: `RoadmapItemRepo::get_existing_promotion`
-- queries by origin_feedback_id within scope. The UNIQUE constraint covers
-- the lookup but having tenant_id alongside lets the planner prove
-- single-row + scope-check in one index path.
CREATE INDEX roadmap_items_origin_feedback_idx
    ON roadmap_items (origin_feedback_id)
    WHERE origin_feedback_id IS NOT NULL;
