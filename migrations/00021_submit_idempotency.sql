-- 00021_submit_idempotency.sql -- submit dedupe on flaky-network retries
--
-- A widget/client retrying `POST .../feedback` after a network failure must
-- not create a duplicate feedback row. Dedupe is keyed by
-- `(project_id, idempotency_key)` (the PRIMARY KEY): the first submit records
-- the created `feedback_id`; a retry with the same key returns that same
-- feedback rather than inserting again.
--
-- ## Why `feedback_id` cascades
--
-- `feedback_id REFERENCES feedback(id) ON DELETE CASCADE`: erasing a feedback
-- row (Phase A A1, right-to-erasure) also removes its idempotency record — a
-- later retry with the same key then creates a FRESH submission rather than
-- returning a dangling (erased) id.
--
-- ## Multi-tenant isolation (DEC-FBR-03)
--
-- `tenant_id` is denormalized alongside `project_id` (mirroring `feedback` /
-- `attachments`) so the repository layer scopes every read/write by
-- `(tenant_id, project_id)` without a join. Both scope FKs cascade, matching
-- the `feedback` table's cascade chain. The `multi-tenant-isolation-check`
-- Verification Oracle scans this migration + the repository queries.
--
-- Lineage:
--   docs/planning/plans/20260701T161200-feedbackmonk-phase-a-gitcellar-contract.md
--     (Stage 0 S0.3 migration reservation; consumed by stream A4)
--   DEC-FBR-03 (repository is sole query path; denormalized scope columns)
--
-- Idempotency: standard sqlx migrator semantics — runs exactly once.

CREATE TABLE submit_idempotency (
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    -- Denormalized scope column (DEC-FBR-03): every query is tenant+project scoped.
    tenant_id  UUID NOT NULL REFERENCES tenants(id)  ON DELETE CASCADE,
    idempotency_key TEXT NOT NULL,
    -- Cascade: erasing a feedback row removes its idempotency record (see above).
    feedback_id UUID NOT NULL REFERENCES feedback(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Dedupe key: one feedback per (project, client-supplied idempotency key).
    PRIMARY KEY (project_id, idempotency_key)
);

-- FK-side lookups: the A1 erasure cascade + per-feedback admin views.
CREATE INDEX submit_idempotency_feedback_idx ON submit_idempotency (feedback_id);
-- Tenant-scoped sweeps (retention jobs, admin views).
CREATE INDEX submit_idempotency_tenant_project_idx ON submit_idempotency (tenant_id, project_id);
