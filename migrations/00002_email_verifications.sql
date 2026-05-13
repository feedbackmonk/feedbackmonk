-- 00002_email_verifications.sql -- email verification tokens for FR-FBR-02
--
-- Backs Worker A's signup -> verify-email flow. The token is opaque (a 32-byte
-- random value, base64url-encoded on the wire); we store the on-wire form
-- directly as TEXT PRIMARY KEY so lookup is O(1) by token.
--
-- Lineage: FR-FBR-02 + Stage 2 PODS session decision DEC-PODS-* (see
-- .claude/collaboration/collab-20260513-221600/channels/decisions.md).
--
-- Idempotency: verify-email may be hit twice (double-click); the second hit
-- succeeds within a short replay window. `used_at` is set on first redemption.
-- After the replay window, subsequent redemptions return 410 Gone (handled in
-- the API layer; the repository surface itself only stores the timestamp).

CREATE TABLE email_verifications (
    token TEXT PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX email_verifications_tenant_idx ON email_verifications (tenant_id);
