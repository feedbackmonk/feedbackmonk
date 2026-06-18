-- 00015_runner_tokens.sql -- feedbackmonk P5b, Contract C25: runner-token
-- privilege separation + lifecycle/revocation. THE auth substrate that lets the
-- runner (FR-FBR-24) write work-order transitions WITHOUT being able to mint an
-- end-user identity JWT or author an `approve` event.
--
-- Three additive changes, all honoring DEC-FBR-04 (feedbackmonk holds ONLY
-- public keys; it never mints/holds a private key -- the customer mints runner
-- tokens client-side, this DB only verifies + revokes):
--
--   1. signing_keys.key_class -- discriminates an end-user IDENTITY key from a
--      RUNNER write-token key. End-user JWT verification accepts ONLY
--      'identity'-class keys; runner-token verification requires a 'runner'-class
--      key. Enforced at the key-SELECTION layer
--      (`SigningKeyRepo::list_active_for_class`), leaving the audited
--      `feedbackmonk_jwt::verify` untouched. Existing rows DEFAULT 'identity' so
--      existing end-user auth is byte-for-byte unaffected -- the one
--      non-additive ripple lives in code (which class each caller selects), NOT
--      in data. A runner key cannot verify an end-user JWT and vice-versa, so a
--      stolen runner key is strictly limited to runner-write transitions
--      (C25 structural property + C22 inv. 2: a runner can never author
--      `approve`).
--
--   2. runner_tokens -- the lifecycle REGISTRY (admin visibility). A runner
--      token is a self-verifying short-TTL JWT; registering it here is OPTIONAL
--      bookkeeping so the admin UI can list issued tokens (jti + label +
--      expires_at) and drive revocation. Not consulted by the verify hot path.
--
--   3. runner_token_revocations -- the APPEND-ONLY per-project jti DENYLIST (the
--      load-bearing security table). `verify_runner_token` rejects a token whose
--      `jti` claim is listed here, even before its `exp`. Append-only mirrors the
--      `work_order_events` / `feedback_status_history` ledger discipline: no
--      UPDATE/DELETE repository methods; a revocation is permanent. A jti can be
--      revoked WITHOUT prior registration (revoke-before-register), so the
--      denylist is independent of the registry.
--
-- Both new tables carry `tenant_id` + `project_id` and route through the
-- tenant-scoped repository layer (DEC-FBR-03) -- the `multi-tenant-isolation-check`
-- oracle auto-covers them; no raw SQL outside the repository crate.
--
-- Lineage:
--   FR-FBR-24 (runner host + auth) / FR-FBR-22 (work-order state machine)
--   Contract C25 (P5b plan, FROZEN) / Q14-issuance (customer-mints model)
--   DEC-FBR-04 (feedbackmonk holds only public keys)
--   DEC-FBR-IMPL (P5b: key_class privilege separation; customer-mints issuance)
--
-- Idempotency: standard sqlx migrator semantics -- runs exactly once.

-- 1. key_class privilege separation -------------------------------------------
ALTER TABLE signing_keys
    ADD COLUMN key_class TEXT NOT NULL DEFAULT 'identity'
        CHECK (key_class IN ('identity', 'runner'));

-- The `list_active_for_class` hot path: active keys of a given class for a
-- project (end-user verify selects 'identity'; runner verify selects 'runner').
CREATE INDEX signing_keys_project_class_active_idx
    ON signing_keys (project_id, key_class) WHERE active = TRUE;

-- 2. runner-token lifecycle registry (admin visibility) -----------------------
CREATE TABLE runner_tokens (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    -- The token's `jti` claim (a UUID minted client-side). TEXT (not UUID) to
    -- tolerate any client jti format; per-project uniqueness makes re-register
    -- idempotent (upsert label/expires_at).
    jti TEXT NOT NULL,
    label TEXT NOT NULL CHECK (length(label) BETWEEN 1 AND 100),
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, jti)
);
CREATE INDEX runner_tokens_project_idx ON runner_tokens (project_id, created_at DESC);

-- 3. runner-token revocation denylist (APPEND-ONLY, the security table) --------
CREATE TABLE runner_token_revocations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    -- The revoked token's `jti` claim. Per-project uniqueness makes a
    -- double-revoke idempotent (INSERT ... ON CONFLICT DO NOTHING).
    jti TEXT NOT NULL,
    -- Copied from the registry at revoke time for audit; NULL when the jti was
    -- revoked without prior registration.
    label TEXT,
    revoked_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, jti)
);
-- The verify hot path: "is this (project, jti) revoked?"
CREATE INDEX runner_token_revocations_lookup_idx
    ON runner_token_revocations (project_id, jti);
