-- 00024_password_resets.sql -- admin password-reset tokens (P1-1)
--
-- Backs the email-token password-reset flow (security scrutiny P1-1: an admin
-- who forgets their password was permanently locked out of their entire
-- feedback corpus). Mirrors `email_verifications` (00002) in shape, with three
-- deliberate differences:
--
--   1. `token_hash` (not the raw token) is the PRIMARY KEY. The wire token is a
--      32-byte random value (base64url); we store ONLY its sha256 hex digest.
--      A read of this table therefore cannot be replayed to reset an account
--      (defense-in-depth, P2-16 sibling). The API layer hashes the presented
--      token and looks it up by digest.
--   2. Short TTL (default 1h, `FEEDBACKMONK_RESET_TOKEN_TTL_HOURS`) — a reset
--      link is far more sensitive than a verify link.
--   3. `used_at` enforces single-use: confirm marks the token used, and the
--      partial-unique guarantee of the PK plus the `used_at IS NULL` predicate
--      in the redeem path make a second confirm a no-op (401).
--
-- Multi-tenant isolation (DEC-FBR-03): `tenant_id` FK CASCADEs on tenant delete
-- (same as email_verifications). The repository layer scopes `mark_used` by
-- `&TenantScope`; `redeem` is the allowlisted pre-auth boundary (the token IS
-- the credential, exactly like `email_verifications.redeem`).
--
-- Lineage:
--   Security scrutiny 2026-07-01, finding P1-1 (no password reset).
--
-- Idempotency: standard sqlx migrator semantics — runs exactly once.

CREATE TABLE password_resets (
    token_hash TEXT PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX password_resets_tenant_idx ON password_resets (tenant_id);
