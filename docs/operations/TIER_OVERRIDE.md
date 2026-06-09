# Tier override — dogfood + operations runbook

P3 Stage 1 ships the tier model + cap enforcement but **does NOT ship a
self-service upgrade flow** (Polar billing deferred per DEC-FBR-DEFER-01).
Until Polar lands, the operator changes a tenant's tier out-of-band.

> **Preferred path (post-v1, DEC-FBR-IMPL-11): the ops endpoint, not raw SQL.**
> `PATCH /api/v1/ops/tenants/{tenant_id}` (guarded by `FEEDBACKMONK_OPS_TOKEN`)
> sets the tier **and** the per-tenant widget brand override (footer/theme/color/
> logo) through the application layer — no DB shell access required, and it goes
> through the same validated repository path as everything else. Example:
> ```
> curl -X PATCH https://<host>/api/v1/ops/tenants/<tenant_id> \
>   -H "Authorization: Bearer $FEEDBACKMONK_OPS_TOKEN" \
>   -H "content-type: application/json" \
>   -d '{"tier":"self_host"}'
> ```
> The endpoint is disabled (404) unless `FEEDBACKMONK_OPS_TOKEN` is set
> (`docs/operations/SELFHOST_ENV.md`). Branding/footer overrides MUST go through
> this endpoint (admin-ops-only, never tenant-self-serve — preserves FR-FBR-14).
> The raw-SQL recipes below remain valid as a **break-glass fallback** when the
> API is down or the ops token isn't provisioned.

This document is the load-bearing operations runbook for two scenarios:

1. **Dogfood**: founder runs feedbackmonk on their own infrastructure and
   needs to flip their own tenant to `self_host` so caps don't apply.
2. **Manual upgrades**: a tenant pays out-of-band (invoice / Stripe link /
   etc.) and the operator promotes them to the appropriate tier.

---

## Per-tier capability matrix (Contract C19 source of truth)

The authoritative source is
[`crates/feedbackmonk-core/src/tier.rs`](../../crates/feedbackmonk-core/src/tier.rs)
`tier_quotas()`. This table mirrors it verbatim; the
`tier-enforcement-status` Verification Oracle Probe B enforces parity.

| Tier      | Projects/org | Monthly feedback | Custom branding | Custom domain | EU residency | Free-tier footer |
| --------- | ------------ | ---------------- | --------------- | ------------- | ------------ | ---------------- |
| Free      | 1            | 50               | ✗               | ✗             | ✗            | ✓                |
| Starter   | 3            | 500              | ✓               | ✗             | ✗            | ✗                |
| Pro       | unlimited    | 10,000           | ✓               | ✓             | ✓            | ✗                |
| SelfHost  | unlimited    | unlimited        | ✓               | ✓             | ✓            | ✗                |

`Free-tier footer` = the widget renders "powered by feedbackmonk" at the
bottom of the embeddable surface. FR-FBR-14 brand promise — DO NOT relax
without a DEC-FBR-* entry; the `tier-enforcement-status` Probe B will
flag drift.

---

## How to change a tenant's tier

### Prereqs

- `psql` (or `pgcli`) installed.
- Read access to the database. Production credentials live in the
  operator's password manager; dev: `postgres://postgres:dev@localhost:5433/feedbackmonk_dev`.
- The tenant's email or `id` (UUID).

### Dogfood: flip your own tenant to `self_host`

Most useful when the founder runs feedbackmonk on their own gear and
wants no caps when triaging their own product's feedback.

```sql
-- Replace 'founder@example.com' with the actual email.
UPDATE tenants
SET tier = 'self_host', updated_at = now()
WHERE email = 'founder@example.com';
```

Verify:

```sql
SELECT id, email, tier, updated_at FROM tenants WHERE email = 'founder@example.com';
```

### Promote a paying customer to Pro

```sql
UPDATE tenants
SET tier = 'pro', updated_at = now()
WHERE email = '<customer-email>';
```

### Demote a tenant (e.g. after a chargeback)

```sql
UPDATE tenants
SET tier = 'free', updated_at = now()
WHERE email = '<customer-email>';
```

**Project-cap behavior on downgrade**: per P3 plan §Deferred Decisions,
existing over-cap projects are **grandfathered** — the cap-check only
fires on NEW project creation, not on already-existing rows. The tenant
retains visibility / edit / delete of their N projects but cannot
create the (N+1)th until they upgrade again.

---

## Schema invariants

Migration `00008_tenant_tier_check.sql` enforces:

```sql
CHECK (tier IN ('free', 'starter', 'pro', 'self_host'))
```

A malformed `UPDATE … SET tier = 'enterprise'` returns:

```text
ERROR:  new row for relation "tenants" violates check constraint "tenants_tier_check"
```

Defense-in-depth: the Rust `Tier::from_db_str` parser is strict too —
even if the CHECK is bypassed via direct PostgreSQL access, the
repository layer rejects unknown values.

---

## After-the-fact verification

After a tier flip:

1. Operator hits `GET /api/v1/admin/tier` as the affected tenant
   (logged in via the admin UI). The response body's `tier` field MUST
   match the new value.
2. `GET /api/v1/projects/{id}/widget-config` MUST return the appropriate
   `footer_text` (`Some("powered by feedbackmonk")` for `free`, `None`
   for every other tier).

If either smoke is wrong, check:

- The session cookie was issued AFTER the UPDATE (signed-cookie payload
  doesn't carry the tier — but if something cached it, restart the API).
- The CHECK constraint accepts the new value (`\d+ tenants` in psql).

---

## When Polar billing ships (post-P3)

Polar webhook handlers (`docs/deferred/polar-integration.md` defines the
contract) will write the same column. The mapping:

| Polar event                  | `tenants.tier` |
| ---------------------------- | -------------- |
| `subscription.created` (Free plan trial) | `'free'` |
| `subscription.created` (Starter plan) | `'starter'` |
| `subscription.created` (Pro plan) | `'pro'` |
| `subscription.cancelled`     | `'free'`       |
| `subscription.updated`       | new plan       |

The webhook is idempotent + signed; manual SQL overrides will still
work but will be **eventually overwritten** by the next Polar event.
Use environment variable `FEEDBACKMONK_BILLING_OVERRIDE_LOCK=true` to
suppress Polar-side reconciliation when manually managing a tenant.

---

## Lineage

- FR-FBR-14 — tier enforcement
- DEC-FBR-03 — pricing tier matrix
- DEC-FBR-DEFER-01 — Polar billing deferred from P3
- Contract C19 — `tier_quotas()` per-tier shape
- Migration 00008 — `tenants_tier_check` schema invariant
