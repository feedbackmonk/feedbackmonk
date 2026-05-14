# Polar billing integration — DEFERRED

**Status**: deferred from P3 per user direction (DEC-FBR-DEFER-01).
**Trigger to revisit**: when consumer billing becomes a launch-blocker
(currently the founder is dogfooding via the `self_host` tier override).
**Reference implementation to port from**:
[`E:/Developer/SourceControlled/Apps/GitCellar/gitcellar-cloud/src/billing/polar.rs`](https://gitcellar.local/cloud/src/billing/polar.rs)
(read-only reference per DEC-FBR-07; do NOT extract).

---

## Why deferred

P3 plan and intake both included FR-FBR-15 (Polar billing) as a scope
candidate; mid-arc the user clarified: *"we just don't need to set up
billing yet for consumers"* — the founder dogfoods on `self_host` and
paying customers will be onboarded manually via SQL UPDATE (see
[`TIER_OVERRIDE.md`](../operations/TIER_OVERRIDE.md)) until the
consumer-facing upgrade flow matters.

This document captures everything Polar wiring will need so the future
worker can port from GitCellar's billing module without re-deriving the
contract.

---

## Webhook receiver shape

```rust
// crates/feedbackmonk-api/src/handlers/billing/polar.rs (future)

#[derive(Debug, Clone, Deserialize)]
pub struct PolarWebhookEnvelope {
    pub event_id: String,        // unique per delivery; dedupe key
    pub event_type: PolarEventType,
    pub created_at: DateTime<Utc>,
    pub payload: serde_json::Value,
    pub signature: String,       // HMAC-SHA256 over the request body
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolarEventType {
    SubscriptionCreated,
    SubscriptionUpdated,
    SubscriptionCancelled,
    InvoicePaid,
    InvoicePaymentFailed,
}

pub async fn polar_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<StatusCode, ApiError> {
    // 1. Read X-Polar-Signature header
    // 2. HMAC-SHA256 (POLAR_WEBHOOK_SECRET, body) and constant-time compare
    // 3. Deserialise envelope from body
    // 4. Dedupe by event_id (Idempotency-Key style)
    // 5. Dispatch on event_type → tier update + invoice journal write
    Ok(StatusCode::NO_CONTENT)
}
```

Route: `POST /api/billing/polar/webhook` (mounted unauthenticated; the
HMAC signature is the auth).

---

## Schema migration (not yet applied)

```sql
-- migrations/NNNNN_polar_customer.sql -- future
ALTER TABLE tenants
    ADD COLUMN polar_customer_id TEXT UNIQUE,
    ADD COLUMN polar_subscription_id TEXT UNIQUE,
    ADD COLUMN polar_current_period_end TIMESTAMPTZ;

CREATE TABLE polar_events (
    event_id TEXT PRIMARY KEY,         -- Polar's delivery id, for dedupe
    event_type TEXT NOT NULL,
    tenant_id UUID REFERENCES tenants(id) ON DELETE SET NULL,
    payload_json JSONB NOT NULL,
    received_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX polar_events_tenant_idx ON polar_events (tenant_id);
```

**NOT migrated yet** — schema-level changes ship together with the
webhook receiver to avoid orphan columns. P3 Stage 1 leaves the schema
unchanged beyond migration 00008's tier CHECK constraint.

---

## Event → `tenants.tier` mapping

| Polar event                            | DB write                                                                                      |
| -------------------------------------- | --------------------------------------------------------------------------------------------- |
| `subscription.created` (Free trial)    | `UPDATE tenants SET tier = 'free' …`                                                          |
| `subscription.created` (Starter plan)  | `UPDATE tenants SET tier = 'starter', polar_subscription_id = …, polar_current_period_end = …` |
| `subscription.created` (Pro plan)      | `UPDATE tenants SET tier = 'pro', …`                                                          |
| `subscription.updated` (plan change)   | `UPDATE tenants SET tier = '<new>', polar_current_period_end = …`                             |
| `subscription.cancelled`               | `UPDATE tenants SET tier = 'free', polar_subscription_id = NULL`                              |
| `invoice.paid`                         | Idempotency-only (no tier change)                                                             |
| `invoice.payment_failed`               | `UPDATE tenants SET tier = 'free'` after a grace period (TBD; see GitCellar pattern)          |

Plan-id → tier mapping lives in `FEEDBACKMONK_POLAR_PLAN_FREE`,
`_STARTER`, `_PRO` environment variables (load at startup, not hardcoded).

---

## Stage 2 (admin UI) impact

The admin UI's "Upgrade" button is a stub per Stage 2 task brief — it
reads *"Contact support to upgrade"*. When Polar lands:

1. Replace the stub button with a Polar checkout link
   (`https://polar.sh/checkout/<tenant-customer-link>`).
2. Add a "Billing" page that surfaces the next invoice + cancel button.
3. Wire the `TierCapExceeded` 402/409 toast to deep-link to the
   checkout page.

These are NOT in P3 scope.

---

## Port pointers (GitCellar reference)

GitCellar's billing module is the canonical reference for:

- HMAC signature verification (`gitcellar-cloud/src/billing/webhook.rs::verify_signature`)
- Event-id dedupe + replay-window logic (`gitcellar-cloud/src/billing/events.rs`)
- Plan-id → tier mapping config pattern (`gitcellar-cloud/src/billing/plans.rs`)
- Polar SDK error handling + retry semantics (`gitcellar-cloud/src/billing/client.rs`)

Per DEC-FBR-07 GitCellar is **read-only reference, NOT extraction**.
The future worker reads GitCellar's billing module, then writes
feedbackmonk's `billing/` from scratch using the same patterns.

---

## Tests required when Polar lands

- Unit: HMAC signature verification (positive + tampered-body negative).
- Unit: event-id dedupe (replay = no-op).
- Integration (sqlx::test): `subscription.created` → tier flips +
  Probe C `widget-config` footer flips.
- Integration: `subscription.cancelled` → tier returns to Free + caps
  re-fire on next write.
- Integration: cross-tenant safety — webhook for tenant A cannot
  mutate tenant B (tenant lookup is via the Polar customer-id mapping,
  not arbitrary path params).

The `tier-enforcement-status` Verification Oracle Probe C smoke test
extends with a "Polar lifecycle" scenario when this work lands.

---

## Lineage

- **FR-FBR-15** — Polar billing (PROPOSED, deferred from P3 per
  DEC-FBR-DEFER-01)
- **DEC-FBR-DEFER-01** — Polar billing deferred from P3 (this document)
- **DEC-FBR-07** — GitCellar is read-only reference, NOT base for extraction
- **P3 plan §Deferred Decisions** — Polar deferral evaluation
- [`TIER_OVERRIDE.md`](../operations/TIER_OVERRIDE.md) — interim
  operator workflow until Polar lands
