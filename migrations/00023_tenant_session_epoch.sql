-- 00023_tenant_session_epoch.sql -- per-tenant session-revocation epoch (P1-1)
--
-- Session revocation kill-switch for the sole, no-fallback admin account
-- (security scrutiny P1-1). The signed admin-session cookie folds this epoch
-- into its HMAC payload (tenant_id ‖ issued_at ‖ session_epoch). On every
-- request the `AdminSession` extractor compares the cookie's epoch against the
-- tenant's CURRENT `session_epoch` and rejects (401) on mismatch.
--
-- Bumping a tenant's `session_epoch` (logout, password-reset confirm) therefore
-- invalidates EVERY outstanding cookie for that tenant WITHOUT rotating the
-- global `FEEDBACKMONK_SESSION_SECRET` (which would log out all tenants).
--
-- Correct for the single-admin-per-tenant model: "revoke my sessions" ==
-- "revoke this tenant's sessions".
--
-- Deploy note: because the cookie payload shape changes (a third HMAC-covered
-- field), cookies minted before this deploy no longer verify and admins
-- re-login once. This is a one-time, expected effect.
--
-- Lineage:
--   Security scrutiny 2026-07-01, finding P1-1 (no session revocation).
--   DEC-FBR-04 (JWT is the ONLY end-user identity; this is the ADMIN session).
--
-- Idempotency: standard sqlx migrator semantics — runs exactly once.

ALTER TABLE tenants
    ADD COLUMN session_epoch INTEGER NOT NULL DEFAULT 0;
