-- 00030_tenant_hosting.sql -- commercial hosting shape (FR-FBR-32 / FR-FBR-33).
--
-- Adds the host->tenant resolution substrate that DEC-FBR-13 assumes and that
-- nothing in the schema provided: today `tenants` carries only `email`, and the
-- only `slug` in the system is `projects.slug`, which is unique PER TENANT, not
-- globally -- so it cannot address a tenant from a hostname.
--
--   tenants.subdomain        -- the globally-unique DNS label in
--                               `{subdomain}.{root_domain}`. NULLABLE: a NULL
--                               means "this tenant has no public subdomain yet",
--                               so EVERY existing row is untouched on deploy and
--                               no host silently starts resolving. Same
--                               conservative posture as 00016's
--                               `public_board_enabled DEFAULT FALSE`.
--
--   tenant_domains           -- customer-controlled hostnames pointed at us by
--                               CNAME. Two kinds, deliberately NOT one:
--
--     kind = 'public'        -- the sellable FR-FBR-33 surface. Serves the
--                               tenant's public board AND their widget/API
--                               endpoint (DEC-FBR-13 sells both under one CNAME
--                               mechanism). Tenant-creatable, tier-gated.
--
--     kind = 'admin_alias'   -- OPERATOR-registered only, never tenant-creatable
--                               and never sold. Answered with a 301 to the
--                               canonical admin host. This is the DEC-FBR-IMPL-27
--                               reconciliation: DEC-FBR-13 pins admin to exactly
--                               one host, DEC-FBR-14 requires
--                               `triage.gitcellar.com` to keep working with no
--                               GitCellar source edit. A redirect satisfies both
--                               -- admin is SERVED on one origin (no per-domain
--                               session, no admin cookie on a customer domain),
--                               and the existing link still lands on triage.
--
--     status = 'pending'     -- claimed, DNS not yet observed pointing at us.
--     status = 'active'      -- traffic/issuance has been seen for this host.
--
-- `domain` is UNIQUE across ALL tenants: two tenants cannot hold the same
-- hostname, which is what makes host->tenant resolution single-valued. Storage
-- is normalized (lowercase, no port, no trailing dot) by
-- `feedbackmonk_core::hosting::normalize_host`; the CHECK below is a
-- belt-and-braces echo of that, not the primary discipline.
--
-- SECURITY NOTE (the reason this migration exists at all): resolving a host to a
-- tenant is only half the mechanism. The other half -- binding every public
-- route to the resolved tenant so tenant A's host cannot reach tenant B's
-- projects -- lives in `feedbackmonk-api::hosting::bind_public_routes` and is
-- guarded by the `host-tenant-binding` Verification Oracle. Schema alone grants
-- no isolation; see DEC-FBR-IMPL-28.
--
-- Lineage:
--   FR-FBR-32 (host-based tenant resolution + tenant subdomains)
--   FR-FBR-33 (custom domain as the tier-gated paid upgrade)
--   DEC-FBR-13 / DEC-FBR-14 (owner decisions, 2026-08-30)
--   DEC-FBR-IMPL-27 (admin-alias 301) / -28 (binding) / -29 (edge owns ACME)
--   DEC-FBR-03 (tenant-scoped repository layer is the sole query path)
--   Plan: docs/planning/plans/20260830T183500-dec-fbr-13-14-tenant-subdomain-hosting.md
--
-- Idempotency: standard sqlx migrator semantics -- runs exactly once.

-- tenants.subdomain -----------------------------------------------------------
ALTER TABLE tenants
    ADD COLUMN subdomain TEXT;

-- Globally unique, but NULLs do not collide (Postgres UNIQUE treats NULLs as
-- distinct), so any number of tenants may have no subdomain.
CREATE UNIQUE INDEX tenants_subdomain_key ON tenants (subdomain);

-- RFC-1123 label shape, enforced at the schema level as defence-in-depth. The
-- authoritative rule (including the reserved-label set, which SQL is the wrong
-- place for) is `feedbackmonk_core::hosting::validate_subdomain_label`.
ALTER TABLE tenants
    ADD CONSTRAINT tenants_subdomain_shape_check
    CHECK (
        subdomain IS NULL
        OR (
            length(subdomain) BETWEEN 3 AND 63
            AND subdomain ~ '^[a-z0-9]([a-z0-9-]*[a-z0-9])?$'
            AND subdomain !~ '^[0-9]+$'
        )
    );

-- tenant_domains --------------------------------------------------------------
CREATE TABLE tenant_domains (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    domain TEXT NOT NULL UNIQUE
        CHECK (
            length(domain) BETWEEN 4 AND 253
            AND domain = lower(domain)
            AND domain NOT LIKE '%:%'      -- no port
            AND domain NOT LIKE '%.'       -- no trailing dot
            AND domain LIKE '%.%'          -- must be a FQDN, not a bare label
        ),
    kind TEXT NOT NULL CHECK (kind IN ('public', 'admin_alias')),
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'active')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    verified_at TIMESTAMPTZ
);

CREATE INDEX tenant_domains_tenant_id_idx ON tenant_domains (tenant_id);
