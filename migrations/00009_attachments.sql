-- 00009_attachments.sql -- Gap #1 (GitCellar customer-#1 parity): feedback attachments
--
-- Adds the `attachments` table backing the multipart upload endpoint
-- `POST /api/v1/projects/{project_id}/feedback/{feedback_id}/attachments`
-- (GUIDE §6 frozen contract). One feedback row may carry:
--   - up to 4 `image` attachments (screenshots; ≤5 MB each — enforced at the
--     app layer, see crates/feedbackmonk-api/src/handlers/attachments.rs), and
--   - at most one `service_log` and one `console_log` text attachment.
--
-- ## What is persisted
--
-- EVERY attachment (images AND logs) is an object in the configured object
-- store (`crates/feedbackmonk-api/src/storage.rs` — local FS for self-host,
-- S3-compatible for SaaS/MinIO). This table holds only the metadata + the
-- storage key + the resolved URL. Log attachments store the text AFTER it has
-- passed through the canonical `feedbackmonk-tracing` 20-pattern PII scrubber
-- (FR-FBR-10 chokepoint) — raw log text is NEVER persisted. The
-- `attachment_pii_corpus` fixture asserts the stored bytes carry no PII.
--
-- ## Multi-tenant isolation (DEC-FBR-03)
--
-- `tenant_id` + `project_id` are denormalized onto every row (mirroring the
-- `feedback` table) so the repository layer can scope every read/write by
-- `(tenant_id, project_id)` without a join. The `multi-tenant-isolation-check`
-- Verification Oracle runs against this migration + the repository queries.
--
-- `ON DELETE CASCADE` on `feedback_id`: deleting a feedback row deletes its
-- attachments (no orphan rows). The tenant/project FKs also cascade, matching
-- the `feedback` table's cascade chain.
--
-- Lineage:
--   FR-FBR-04 (widget) / GitCellar adoption parity gap #1
--   docs/integrations/gitcellar-adoption.md §6 (upload contract)
--   PODS collab-20260602-123000 GUIDE §6 (frozen migration number 00009)
--   DEC-FBR-03 (repository is sole query path; denormalized scope columns)
--
-- Idempotency: standard sqlx migrator semantics — runs exactly once.

CREATE TABLE attachments (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    feedback_id UUID NOT NULL REFERENCES feedback(id) ON DELETE CASCADE,
    -- Denormalized scope columns (DEC-FBR-03): every query is tenant+project scoped.
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('image', 'service_log', 'console_log')),
    -- Object-store key (path within the bucket / local root). Opaque to the DB.
    storage_key TEXT NOT NULL,
    -- Resolved fetch/public URL returned to the widget in the upload response.
    url TEXT NOT NULL,
    -- MIME type of the stored object: image/png|image/jpeg|image/webp for
    -- images, text/plain for scrubbed log attachments.
    content_type TEXT NOT NULL,
    -- Size in bytes of the STORED object (post-scrub for logs).
    byte_size BIGINT NOT NULL CHECK (byte_size >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Per-feedback listing + the ≤4-image app-layer count check.
CREATE INDEX attachments_feedback_idx ON attachments (feedback_id, created_at);
-- Tenant-scoped sweeps (admin views, retention jobs).
CREATE INDEX attachments_tenant_project_idx ON attachments (tenant_id, project_id);
