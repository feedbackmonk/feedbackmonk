-- 00027: append-only audit log for operator (ops-token) mutations.
--
-- Scrutiny P1-12: the operator god-key (`PATCH /api/v1/ops/tenants/{id}` — tier
-- flips + widget-brand/footer overrides across the WHOLE customer base) wrote NO
-- audit record, so a tier change or badge-strip was unattributable and
-- unforensic ("who changed this tenant's tier, when?"). This append-only log
-- records every successful ops mutation, scoped to the TARGET tenant. (The
-- shared-secret ops token carries no operator identity — that limitation is
-- documented; this log still captures the action + before/after detail, which is
-- the load-bearing forensic record.)
CREATE TABLE ops_audit_log (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id  UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    action     TEXT NOT NULL,
    detail     JSONB NOT NULL DEFAULT '{}'::jsonb,
    at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Newest-first lookups per tenant (the operator forensic query).
CREATE INDEX ops_audit_log_tenant_idx ON ops_audit_log (tenant_id, at DESC);
