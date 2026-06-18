-- 00013_feedback_clusters.sql -- feedbackmonk P5a (Agentic Feedback Resolution
-- Loop), Contract C23 part 1: the analyst data model.
--
-- Adds the three "analyst" tables + one column that turn the feedback inbox
-- into a clustered, swept, recommendation-bearing corpus:
--
--   feedback_clusters  -- canonical grouping of near-duplicate feedback
--   analysis_sweeps    -- provenance for each deep sweep (FR-FBR-20)
--   recommendations    -- the analyst's proposed action for a cluster
--   feedback.cluster_id -- first-class nullable "current cluster" pointer
--
-- DECISION POINT D1 (RESOLVED in the P5a plan): `feedback.cluster_id` is a
-- FIRST-CLASS NULLABLE FK COLUMN, not a join table. A feedback belongs to at
-- most one cluster at a time; re-clustering is audited via
-- `feedback_clusters.updated_at` + sweep provenance, not historised in a
-- membership table. Idiomatic per the `crash_event_id` precedent (00010).
--
-- Every domain table carries denormalized `tenant_id` + `project_id` (NOT
-- NULL) so the tenant-scoped repository layer (DEC-FBR-03) can filter by both
-- without a join. Enums are TEXT + CHECK (the `FeedbackKind` / status-workflow
-- precedent) -- no Postgres native enums, keeping migrations cheap and matching
-- existing convention. The CHECK value sets MUST match the `.as_db_str()`
-- output of the `feedbackmonk-core` enums byte-for-byte:
--   kind        -> FeedbackKind   ('bug','feature','question','other')
--   action_type -> ActionType     ('bug_fix','feature_implementation','enhancement','investigation','no_action')
--
-- Lineage:
--   FR-FBR-19 (clustering) / FR-FBR-20 (analysis sweep)
--   Contract C23 (P5a plan §Frozen Contracts)
--   DEC-FBR-03 (sole query path) / DEC-FBR-12 (open-core boundary, deferred)
--   D1 (cluster_id is a column, not a join table)
--
-- Idempotency: standard sqlx migrator semantics -- runs exactly once.

-- feedback_clusters ----------------------------------------------------------
-- A canonical grouping of near-duplicate feedback ("same thing said many
-- ways"). `priority_rationale` is load-bearing for explainability: the owner
-- must see WHY a cluster was prioritised. `merged_into_id` supports owner merge
-- (the merged cluster's status becomes 'merged' and points at the survivor).
CREATE TABLE feedback_clusters (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    label TEXT NOT NULL CHECK (length(label) BETWEEN 1 AND 512),
    summary TEXT,
    kind TEXT NOT NULL DEFAULT 'other'
        CHECK (kind IN ('bug','feature','question','other')),
    priority TEXT NOT NULL DEFAULT 'none'
        CHECK (priority IN ('high','medium','low','none')),
    priority_rationale TEXT,
    status TEXT NOT NULL DEFAULT 'open'
        CHECK (status IN ('open','actioned','dismissed','merged')),
    -- Self-referential survivor pointer for merges. ON DELETE SET NULL so
    -- deleting a survivor doesn't cascade-delete the clusters merged into it.
    merged_into_id UUID REFERENCES feedback_clusters(id) ON DELETE SET NULL,
    member_count INTEGER NOT NULL DEFAULT 0 CHECK (member_count >= 0),
    last_swept_at TIMESTAMPTZ,
    created_by TEXT NOT NULL DEFAULT 'agent'
        CHECK (created_by IN ('agent','admin')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- A cluster's merge survivor cannot be itself.
    CONSTRAINT feedback_clusters_no_self_merge
        CHECK (merged_into_id IS NULL OR merged_into_id <> id)
);
-- Owner cluster-list view: open clusters for a project, highest priority first.
CREATE INDEX feedback_clusters_project_status_idx
    ON feedback_clusters (project_id, status);
CREATE INDEX feedback_clusters_tenant_idx
    ON feedback_clusters (tenant_id);

-- analysis_sweeps ------------------------------------------------------------
-- Provenance for each deep sweep (FR-FBR-20). `digest_summary` powers the
-- "what changed since last time" review. P5a stores the sweep record +
-- orchestration; the deep recommendation generation runs customer-side (the
-- runner, P5b) and writes back through the ingestion API.
CREATE TABLE analysis_sweeps (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    triggered_by TEXT NOT NULL
        CHECK (triggered_by IN ('schedule','on_demand')),
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ,
    status TEXT NOT NULL DEFAULT 'running'
        CHECK (status IN ('running','completed','failed')),
    clusters_touched INTEGER NOT NULL DEFAULT 0 CHECK (clusters_touched >= 0),
    recommendations_emitted INTEGER NOT NULL DEFAULT 0
        CHECK (recommendations_emitted >= 0),
    runner_id TEXT,
    agent_version TEXT,
    digest_summary TEXT
);
-- Digest view: most-recent sweeps for a project first.
CREATE INDEX analysis_sweeps_project_started_idx
    ON analysis_sweeps (project_id, started_at DESC);

-- recommendations ------------------------------------------------------------
-- The analyst's recommended action for an actionable cluster. `source_refs` =
-- the code files/lines/docs the analyst inspected (grounding evidence) -- it is
-- a list of REFERENCES, never a dump of file contents (exfiltration defense,
-- C24 case f). 1:N from cluster (history); superseded recs are retained.
CREATE TABLE recommendations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    cluster_id UUID NOT NULL REFERENCES feedback_clusters(id) ON DELETE CASCADE,
    -- Provenance sweep. ON DELETE SET NULL: a recommendation outlives the sweep
    -- record that produced it.
    sweep_id UUID REFERENCES analysis_sweeps(id) ON DELETE SET NULL,
    action_type TEXT NOT NULL
        CHECK (action_type IN ('bug_fix','feature_implementation','enhancement','investigation','no_action')),
    title TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 512),
    body TEXT NOT NULL,
    rationale TEXT,
    source_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    confidence DOUBLE PRECISION NOT NULL DEFAULT 0
        CHECK (confidence >= 0 AND confidence <= 1),
    status TEXT NOT NULL DEFAULT 'proposed'
        CHECK (status IN ('proposed','approved','tweaked_approved','rejected','superseded')),
    generated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- Cluster detail view: a cluster's recommendations, newest first.
CREATE INDEX recommendations_cluster_idx
    ON recommendations (cluster_id, generated_at DESC);
CREATE INDEX recommendations_project_status_idx
    ON recommendations (project_id, status);

-- feedback.cluster_id --------------------------------------------------------
-- First-class "current cluster" pointer (decision point D1). Nullable: a
-- feedback row may be unclustered (pre-cluster, or a singleton). ON DELETE SET
-- NULL so deleting a cluster un-clusters its members rather than deleting the
-- feedback. Additive + nullable => non-breaking for every existing
-- INSERT/SELECT on `feedback`.
ALTER TABLE feedback
    ADD COLUMN cluster_id UUID REFERENCES feedback_clusters(id) ON DELETE SET NULL;

-- Cluster-membership lookup ("which feedback is in this cluster?"). Partial
-- index keeps it tiny -- only the clustered minority is indexed.
CREATE INDEX feedback_cluster_id_idx
    ON feedback (cluster_id)
    WHERE cluster_id IS NOT NULL;
