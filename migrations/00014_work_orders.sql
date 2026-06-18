-- 00014_work_orders.sql -- feedbackmonk P5a, Contract C22/C23 part 2: the
-- work-order API substrate + the approval state machine's append-only ledger.
--
--   work_orders        -- the contract between an APPROVED decision and a
--                         DISPATCHED job (FR-FBR-22). `owner_overrides` carries
--                         the Q17 "tweak before approve" edits.
--   work_order_events  -- APPEND-ONLY audit trail + state-machine ledger. The
--                         substrate the `approval-gate-enforcement` oracle
--                         reads: no row may reach state >= 'dispatched' without
--                         a prior owner-authored 'approved' event (C22 inv. 1,
--                         FR-FBR-25a -- THE security boundary).
--
-- The state machine mirrors FR-FBR-08's proven pattern (00003): the legal
-- transition table lives in `feedbackmonk-core::work_order::legal_transitions_from`,
-- illegal transitions are rejected pre-DB-check, and every state change writes
-- BOTH the `work_orders.state` UPDATE AND a `work_order_events` row in the same
-- DB transaction (C22 inv. 3, the `append_in_executor` pattern). The CHECK
-- constraint on `state` is belt-and-braces; the discipline lives in Worker A's
-- transition handler.
--
-- The `state` / `to_state` / `from_state` CHECK value set MUST match
-- `WorkOrderState::as_db_str()` byte-for-byte (kebab-case):
--   draft | approved | dispatched | claimed | building | verifying
--       | reported | completed | failed | cancelled
--
-- `approved_by` / `actor_id` are bare UUIDs / TEXT without an FK -- mirrors the
-- `feedback_status_history.transitioned_by` precedent (00003): P5a has no
-- `tenant_users` table, and the runner is identified by a token subject (TEXT),
-- not a tenant row. A later migration may add deferred FKs.
--
-- APPEND-ONLY: `work_order_events` has NO UPDATE/DELETE repository methods
-- (enforced at the repository layer, like `feedback_status_history`). The
-- ledger is the immutable record the security oracle trusts.
--
-- Lineage:
--   FR-FBR-22 (work-order API + approval state machine)
--   FR-FBR-25a (approval-as-security-boundary) / FR-FBR-25 (untrusted input)
--   Contract C22 (P5a plan, FROZEN) / Contract C23 (data model)
--   Q14 (runner write-token authz seam) / Q17 (owner_overrides authoritative)
--   GitCellar peer reference: feedback status-workflow pattern (00003)
--
-- Idempotency: standard sqlx migrator semantics -- runs exactly once.

-- work_orders ----------------------------------------------------------------
CREATE TABLE work_orders (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    -- Provenance: the recommendation this order was created from. The
    -- recommendation is provenance; the work order is the order (Q17). CASCADE
    -- so a tenant teardown removes orders alongside their recommendations.
    recommendation_id UUID NOT NULL REFERENCES recommendations(id) ON DELETE CASCADE,
    cluster_id UUID NOT NULL REFERENCES feedback_clusters(id) ON DELETE CASCADE,
    action_type TEXT NOT NULL
        CHECK (action_type IN ('bug_fix','feature_implementation','enhancement','investigation','no_action')),
    title TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 512),
    instructions TEXT NOT NULL,
    -- Q17 authoritative "tweak before approve" edits. Merged over the
    -- recommendation's fields at dispatch with OVERRIDES WINNING.
    owner_overrides JSONB,
    autonomy_rung INTEGER NOT NULL CHECK (autonomy_rung BETWEEN 0 AND 3),
    state TEXT NOT NULL DEFAULT 'draft'
        CHECK (state IN ('draft','approved','dispatched','claimed','building','verifying','reported','completed','failed','cancelled')),
    -- Set when the owner authors the 'approve' event (C22 inv. 1). Bare UUID,
    -- no FK (no tenant_users table yet; mirrors transitioned_by).
    approved_by UUID,
    approved_at TIMESTAMPTZ,
    dispatched_at TIMESTAMPTZ,
    -- The runner subject (token `sub`) that claimed the order. Runner-token
    -- ISSUANCE is P5b; in P5a this is written only by the runner-transition
    -- seam, exercised by tests/a no-op probe.
    claimed_by_runner TEXT,
    result_ref JSONB,
    failure_reason TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- Admin list view: orders for a project filtered by state.
CREATE INDEX work_orders_project_state_idx
    ON work_orders (project_id, state, created_at DESC);
CREATE INDEX work_orders_cluster_idx
    ON work_orders (cluster_id);
CREATE INDEX work_orders_recommendation_idx
    ON work_orders (recommendation_id);
CREATE INDEX work_orders_tenant_idx
    ON work_orders (tenant_id);

-- work_order_events ----------------------------------------------------------
-- APPEND-ONLY. `from_state` is NULL for the genesis event (work-order
-- creation: NULL -> 'draft'). `to_state` is always a valid WorkOrderState.
-- `event_type` records WHICH transition fired; the oracle queries it for the
-- owner-authored 'approve' event. `actor` ties to the C22 authz matrix.
CREATE TABLE work_order_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    work_order_id UUID NOT NULL REFERENCES work_orders(id) ON DELETE CASCADE,
    from_state TEXT
        CHECK (from_state IS NULL OR from_state IN ('draft','approved','dispatched','claimed','building','verifying','reported','completed','failed','cancelled')),
    to_state TEXT NOT NULL
        CHECK (to_state IN ('draft','approved','dispatched','claimed','building','verifying','reported','completed','failed','cancelled')),
    event_type TEXT NOT NULL
        CHECK (event_type IN ('create','approve','dispatch','claim','building','verifying','reported','failed','accept','request-changes','reject','retry','cancel')),
    actor TEXT NOT NULL
        CHECK (actor IN ('admin','runner','system')),
    -- The acting identity: admin UUID (string form) or runner token subject.
    -- Bare TEXT, no FK (mirrors transitioned_by).
    actor_id TEXT,
    detail JSONB,
    at TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- Ledger replay for one work order, oldest-first (state-machine order).
CREATE INDEX work_order_events_order_idx
    ON work_order_events (work_order_id, at ASC);
-- Approval-gate oracle query path: "is there an owner-authored 'approve'
-- event for this order?" -- the partial index makes the lookup tiny.
CREATE INDEX work_order_events_approve_idx
    ON work_order_events (work_order_id)
    WHERE event_type = 'approve';
