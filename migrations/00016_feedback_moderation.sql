-- 00016_feedback_moderation.sql -- Public Feedback Board + Moderation Gate,
-- Stage 0 freeze (Contracts C28/C29).
--
-- Introduces a NEW, ORTHOGONAL public-visibility axis on feedback — distinct
-- from the triage `feedback.status` machine (00003), which tracks workflow
-- progress (submitted/triaged/in-progress/...), NOT public exposure. Overloading
-- triage status as a visibility flag was explicitly rejected in planning; this
-- migration keeps the two axes separate.
--
--   feedback.moderation_status        -- pending | approved | rejected. NEW rows
--                                        default 'pending'. ONLY 'approved' is
--                                        ever returned by a public-board read
--                                        (the trust boundary, C29).
--   feedback_moderation_events        -- APPEND-ONLY audit trail + the ledger the
--                                        `public-board-moderation-gate` oracle
--                                        reads: no feedback may be publicly
--                                        visible without a prior owner-authored
--                                        'approve' event (C28 inv. 1, FR-FBR-25a
--                                        sibling — the moderation trust boundary).
--   projects.public_board_enabled     -- per-project board master switch. DEFAULT
--                                        FALSE so NO existing project silently
--                                        exposes feedback publicly on deploy.
--   projects.board_requires_moderation-- when TRUE (default), only 'approved'
--                                        rows reach the board. Reserved for a
--                                        future "auto-approve" relaxation; v1
--                                        always requires moderation.
--
-- The state machine mirrors the proven work-order pattern (00014) and the
-- feedback status-workflow pattern (00003): the legal transition table lives in
-- `feedbackmonk-core::moderation::legal_moderation_transitions_from`, illegal
-- transitions are rejected pre-DB-check, and every status change writes BOTH the
-- `feedback.moderation_status` UPDATE AND a `feedback_moderation_events` row in
-- the same DB transaction. The CHECK constraints are belt-and-braces; the
-- discipline lives in Worker A's moderation handler.
--
-- The `moderation_status` / `to_status` / `from_status` CHECK value set MUST
-- match `ModerationStatus::as_db_str()` byte-for-byte (lowercase):
--   pending | approved | rejected
--
-- `actor_id` is bare TEXT without an FK — mirrors `feedback_status_history`
-- (00003) and `work_order_events` (00014): there is no `tenant_users` table yet,
-- and the acting admin is identified by the tenant UUID (string form). A later
-- migration may add a deferred FK.
--
-- APPEND-ONLY: `feedback_moderation_events` has NO UPDATE/DELETE repository
-- methods (enforced at the repository layer, like `feedback_status_history` and
-- `work_order_events`). The ledger is the immutable record the gate oracle trusts.
--
-- Lineage:
--   FR-FBR-12 (public roadmap) sibling — public board is the new public surface
--   FR-FBR-25a (approval-as-security-boundary) — moderation gate is the same
--     class of trust boundary applied to public-board visibility
--   DEC-FBR-02 (no-trackers brand promise) / Q24 (public-surface privacy) — the
--     board never exposes submitter PII (enforced in C29 + Probe B)
--   Plan: docs/planning/plans/20260619T001105-public-feedback-board-moderation-gate.md
--   GitCellar peer reference: feedback status-workflow pattern (00003)
--
-- Idempotency: standard sqlx migrator semantics -- runs exactly once.

-- feedback.moderation_status -------------------------------------------------
-- Adding a NOT NULL column with a DEFAULT backfills every existing row to
-- 'pending' in one statement. Combined with public_board_enabled=FALSE, no
-- existing feedback becomes publicly visible: owners curate from the moderation
-- queue when they opt a project into the board.
ALTER TABLE feedback
    ADD COLUMN moderation_status TEXT NOT NULL DEFAULT 'pending'
        CHECK (moderation_status IN ('pending', 'approved', 'rejected'));

-- Board read path: approved rows for a project, newest-first. Also serves the
-- admin moderation-queue read (filter on 'pending').
CREATE INDEX feedback_project_moderation_idx
    ON feedback (project_id, moderation_status, accepted_at DESC);

-- feedback_moderation_events -------------------------------------------------
-- APPEND-ONLY. `from_status` is NULL for the genesis event (NULL -> 'pending'
-- at submit, if recorded). `to_status` is always a valid ModerationStatus.
-- `event_type` records WHICH moderation action fired; the oracle queries it for
-- the owner-authored 'approve' event.
CREATE TABLE feedback_moderation_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    feedback_id UUID NOT NULL REFERENCES feedback(id) ON DELETE CASCADE,
    from_status TEXT
        CHECK (from_status IS NULL OR from_status IN ('pending', 'approved', 'rejected')),
    to_status TEXT NOT NULL
        CHECK (to_status IN ('pending', 'approved', 'rejected')),
    -- to Approved => 'approve'; to Rejected => 'reject'; to Pending => 'reset'.
    event_type TEXT NOT NULL
        CHECK (event_type IN ('create', 'approve', 'reject', 'reset')),
    -- Only the authenticated owner moderates in v1; 'system' reserved for a
    -- future auto-approve relaxation. No submitter ever authors a row here.
    actor TEXT NOT NULL
        CHECK (actor IN ('admin', 'system')),
    -- Acting admin identity: tenant UUID (string form). Bare TEXT, no FK
    -- (mirrors feedback_status_history.transitioned_by / work_order_events.actor_id).
    actor_id TEXT,
    reason_note TEXT,
    detail JSONB,
    at TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- Ledger replay for one feedback row, oldest-first (state-machine order).
CREATE INDEX feedback_moderation_events_feedback_idx
    ON feedback_moderation_events (feedback_id, at ASC);
-- Moderation-gate oracle query path: "is there an owner-authored 'approve'
-- event for this feedback?" -- the partial index makes the lookup tiny.
CREATE INDEX feedback_moderation_events_approve_idx
    ON feedback_moderation_events (feedback_id)
    WHERE event_type = 'approve';
CREATE INDEX feedback_moderation_events_tenant_idx
    ON feedback_moderation_events (tenant_id);

-- projects: per-project board settings ---------------------------------------
-- DEFAULT FALSE: opting a project into the public board is a deliberate owner
-- action, never a side effect of deploying this migration.
ALTER TABLE projects
    ADD COLUMN public_board_enabled BOOLEAN NOT NULL DEFAULT FALSE;
-- DEFAULT TRUE: when the board is on, only moderation-approved rows appear.
-- Reserved for a future auto-approve relaxation; v1 keeps this TRUE.
ALTER TABLE projects
    ADD COLUMN board_requires_moderation BOOLEAN NOT NULL DEFAULT TRUE;
