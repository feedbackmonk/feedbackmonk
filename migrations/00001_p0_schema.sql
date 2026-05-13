-- 00001_p0_schema.sql -- Feedbackr P0 (Foundation) initial schema
--
-- Authoritative source for column names and constraints. Stage 2 workers and
-- all P1+ code hard-depend on these names; renames require a follow-up
-- migration AND coordinated downstream changes.
--
-- Lineage:
--   FR-FBR-01 (multi-tenant data model)
--   DEC-FBR-03 (repository layer is sole query path)
--   DEC-FBR-04 (JWT sub claim is sole end-user identity)
--   P0 plan section "Sub-task 1" + Contract C1
--
-- gen_random_uuid() requires pgcrypto (or pgcrypto-equivalent on PG13+).

CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- tenants ---------------------------------------------------------------------
CREATE TABLE tenants (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    verified_at TIMESTAMPTZ,
    tier TEXT NOT NULL DEFAULT 'free',  -- forward-looking: P3 tier enforcement
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- projects --------------------------------------------------------------------
CREATE TABLE projects (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    slug TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, slug)
);
CREATE INDEX projects_tenant_id_idx ON projects (tenant_id);

-- signing_keys ---------------------------------------------------------------
-- Ed25519 public keys for JWT verification (FR-FBR-05, Contract C4).
CREATE TABLE signing_keys (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    public_key BYTEA NOT NULL,  -- 32 bytes raw Ed25519
    label TEXT NOT NULL,
    active BOOLEAN NOT NULL DEFAULT TRUE,
    registered_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deactivated_at TIMESTAMPTZ
);
CREATE INDEX signing_keys_project_active_idx ON signing_keys (project_id) WHERE active = TRUE;

-- feedback -------------------------------------------------------------------
-- The heart of the product. Exactly one of (end_user_sub, anon_token_hash)
-- must be set, enforced at the schema level via CHECK constraint.
CREATE TABLE feedback (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    short_code TEXT NOT NULL UNIQUE,  -- public-facing FB-XXXXXX form
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,  -- denormalized for tenant-scoped queries
    end_user_sub TEXT,           -- JWT sub claim (auth mode); NULL for anonymous
    end_user_email TEXT,         -- optional, either source
    end_user_name TEXT,          -- optional, JWT-mode only
    external_metadata JSONB,     -- <= 4KB enforced at app layer; auth-mode only
    anon_token_hash BYTEA,       -- 32 bytes; anonymous-mode only
    body TEXT NOT NULL CHECK (length(body) BETWEEN 1 AND 16384),
    kind TEXT NOT NULL DEFAULT 'other' CHECK (kind IN ('bug','feature','question','other')),
    accepted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((end_user_sub IS NOT NULL) <> (anon_token_hash IS NOT NULL))
);
CREATE INDEX feedback_project_accepted_idx ON feedback (project_id, accepted_at DESC);
CREATE INDEX feedback_tenant_idx ON feedback (tenant_id);

-- anon_submissions -----------------------------------------------------------
-- Per-cookie/project dedup tracking, distinct from feedback rows.
CREATE TABLE anon_submissions (
    anon_token_hash BYTEA NOT NULL,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    first_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_submission_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    submission_count INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (anon_token_hash, project_id)
);

-- rate_limit_counters --------------------------------------------------------
-- In-memory governor is the primary P0 rate-limiter; persisted counters
-- allow restart-recovery hints. Redis-backed limiter deferred to v1.1.
CREATE TABLE rate_limit_counters (
    bucket_key TEXT NOT NULL,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    window_start TIMESTAMPTZ NOT NULL,
    count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (bucket_key, project_id, window_start)
);
