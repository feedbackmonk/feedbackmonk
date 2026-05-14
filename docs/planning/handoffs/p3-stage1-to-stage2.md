# P3 Stage 1 → Stage 2 — Contract Freeze

**Stage 1 commit**: see `git log --grep='P3-S1'` after orchestrator finalize.
**Stage 1 worker**: `session-20260514-102233-006` (orchestrated worker).
**Stage 2 trigger**: admin UI tier-settings page + cap-aware error rendering.
**Frozen contracts**: **C17** (tier-cap predicate trait), **C18**
(`TierCapExceeded` HTTP error body), **C19** (`tier_quotas()` static config).

**These contracts are FROZEN.** Stage 2 consumes them verbatim. Changes
require a `DEC-FBR-*` decision entry + Stage 1 worker re-engagement.

---

## Contract C17 — Tier-cap predicate (frozen verbatim)

The repository-layer trait Stage 2's admin UI consumes via the
`GET /api/v1/admin/tier` endpoint and via the structured HTTP error
shape (Contract C18) returned on 402/409 responses.

```rust
// crates/feedbackmonk-repository/src/tier_quota.rs

pub trait TierQuotaRepo: Send + Sync {
    /// Predicate: would creating a NEW unit of `resource` under `scope`
    /// be allowed by the tenant's current tier?
    async fn check_tier_quota(
        &self,
        scope: &TenantScope,
        resource: ResourceKind,
    ) -> Result<QuotaStatus, RepoError>;

    /// Aggregate read for the admin tier-status endpoint. No side effects.
    async fn get_tier_status(
        &self,
        scope: &TenantScope,
    ) -> Result<TierStatus, RepoError>;
}

// crates/feedbackmonk-core/src/tier.rs

pub enum ResourceKind {
    Project,                  // SELECT COUNT(*) FROM projects WHERE tenant_id = ?
    FeedbackInRollingMonth,   // SELECT COUNT(*) FROM feedback WHERE tenant_id = ?
                              //                   AND accepted_at > now() - interval '30 days'
}

// crates/feedbackmonk-repository/src/tier_quota.rs

pub struct QuotaStatus {
    pub tier: Tier,
    pub resource: ResourceKind,
    pub current: i64,
    pub limit: Option<i64>,   // None = unlimited (Pro/SelfHost projects; SelfHost feedback)
    pub allowed: bool,        // false → handler returns ApiError::TierCapExceeded
}

pub struct TierStatus {
    pub tier: Tier,
    pub quotas: TierQuotas,   // static — from tier_quotas() for this tier
    pub usage: TierUsage,
}

pub struct TierUsage {
    pub projects: i64,
    pub feedback_monthly: i64,
    pub period_start: chrono::DateTime<chrono::Utc>,
}

pub const ROLLING_FEEDBACK_WINDOW_DAYS: i64 = 30;
```

**Wire endpoint** (Stage 2's `fetchTierStatus()` consumes this):

```text
GET /api/v1/admin/tier
Cookie: feedbackmonk_session=...

200 OK
{
  "tier": "free" | "starter" | "pro" | "self_host",
  "quotas": {
    "projects_per_org": 1,        // Option<i64> — null when unlimited
    "monthly_feedback_volume": 50,
    "custom_branding": false,
    "custom_domain": false,
    "eu_residency": false,
    "footer_text": "powered by feedbackmonk" // null when paid tier
  },
  "usage": {
    "projects": 0,
    "feedback_monthly": 0,
    "period_start": "2026-04-14T12:00:00Z"
  }
}

401 if no admin session.
```

---

## Contract C18 — `TierCapExceeded` HTTP error body (frozen verbatim)

Body shape returned by ANY 402 / 409 cap-firing response:

```typescript
// admin-ui/src/shared/types.gen.ts mirror — Stage 2 ports this verbatim
interface TierCapExceededBody {
  error: "tier_cap_exceeded";
  tier: "free" | "starter" | "pro" | "self_host";
  resource: "project" | "feedback_in_rolling_month";
  current: number;
  limit: number;
  upgrade_hint: string;  // e.g., "Upgrade to Starter for 3 projects per org."
}
```

**Status-code mapping**:

| `resource`                       | HTTP status         | Idiomatic meaning           |
| -------------------------------- | ------------------- | --------------------------- |
| `project`                        | 409 Conflict        | State conflict (too many)   |
| `feedback_in_rolling_month`      | 402 Payment Required | Idiomatic paywall semantic |

**Stage 1 emits**: `crates/feedbackmonk-api/src/error.rs`
`ApiError::TierCapExceeded { tier, resource, current, limit, upgrade_hint }`
serialised via `IntoResponse` — the JSON output exactly matches the
TypeScript above. Stage 1 unit tests in `error.rs` assert the byte
shape per resource kind.

**Stage 2 renders**: the `upgrade_hint` string verbatim in the
UpgradePrompt toast — Stage 1 owns the copy choice so Stage 2's UI is
purely a renderer. If Stage 2 wants different copy, file a
`DEC-FBR-*` and update `upgrade_hint_for_*` in
`crates/feedbackmonk-api/src/handlers/projects.rs` +
`handlers/feedback.rs`.

---

## Contract C19 — `tier_quotas()` static config (frozen verbatim)

Source of truth in `crates/feedbackmonk-core/src/tier.rs`:

```rust
pub const fn tier_quotas(tier: Tier) -> TierQuotas {
    match tier {
        Tier::Free     => TierQuotas {
            projects_per_org: Some(1),
            monthly_feedback_volume: Some(50),
            custom_branding: false,
            custom_domain: false,
            eu_residency: false,
            footer_text: Some("powered by feedbackmonk"),
        },
        Tier::Starter  => TierQuotas {
            projects_per_org: Some(3),
            monthly_feedback_volume: Some(500),
            custom_branding: true,
            custom_domain: false,
            eu_residency: false,
            footer_text: None,
        },
        Tier::Pro      => TierQuotas {
            projects_per_org: None,
            monthly_feedback_volume: Some(10000),
            custom_branding: true,
            custom_domain: true,
            eu_residency: true,
            footer_text: None,
        },
        Tier::SelfHost => TierQuotas {
            projects_per_org: None,
            monthly_feedback_volume: None,
            custom_branding: true,
            custom_domain: true,
            eu_residency: true,
            footer_text: None,
        },
    }
}
```

The `tier-enforcement-status` Verification Oracle Probe B asserts this
shape byte-for-byte. Any change requires a `DEC-FBR-*` entry; the
oracle blocks silent drift.

**Display matrix (mirror for `admin-ui/src/pages/settings/TierSettings.tsx`)**:

| Tier      | Projects/org | Monthly feedback | Custom branding | Custom domain | EU residency | Free-tier footer |
| --------- | ------------ | ---------------- | --------------- | ------------- | ------------ | ---------------- |
| Free      | 1            | 50               | ✗               | ✗             | ✗            | ✓                |
| Starter   | 3            | 500              | ✓               | ✗             | ✗            | ✗                |
| Pro       | unlimited    | 10,000           | ✓               | ✓             | ✓            | ✗                |
| SelfHost  | unlimited    | unlimited        | ✓               | ✓             | ✓            | ✗                |

---

## TypeScript starter kit for `admin-ui/src/shared/types.gen.ts`

Stage 2 worker pastes this verbatim into `types.gen.ts`. Drift from
Contract C17/C18/C19 is a Stage 2 implementation bug.

```typescript
// types.gen.ts -- generated from feedbackmonk-core::tier + Contract C18.
// Source of truth: docs/planning/handoffs/p3-stage1-to-stage2.md
// Do NOT hand-edit semantic shape — feedback to Stage 1 via DEC-FBR-*.

export type Tier = "free" | "starter" | "pro" | "self_host";

export type ResourceKind = "project" | "feedback_in_rolling_month";

export interface TierQuotas {
  projects_per_org: number | null;        // null = unlimited
  monthly_feedback_volume: number | null; // null = unlimited
  custom_branding: boolean;
  custom_domain: boolean;
  eu_residency: boolean;
  footer_text: string | null;             // "powered by feedbackmonk" or null
}

export interface TierUsage {
  projects: number;
  feedback_monthly: number;
  period_start: string;                   // ISO-8601
}

export interface TierStatus {
  tier: Tier;
  quotas: TierQuotas;
  usage: TierUsage;
}

export interface TierCapExceededBody {
  error: "tier_cap_exceeded";
  tier: Tier;
  resource: ResourceKind;
  current: number;
  limit: number;
  upgrade_hint: string;
}

// Type-guard for narrow error handling.
export function isTierCapExceeded(body: unknown): body is TierCapExceededBody {
  return (
    typeof body === "object" &&
    body !== null &&
    (body as { error?: unknown }).error === "tier_cap_exceeded"
  );
}
```

---

## Stage 2 ApiClient extension (sketch)

```typescript
// admin-ui/src/shared/ApiClient.ts

export async function fetchTierStatus(): Promise<TierStatus> {
  const resp = await fetch("/api/v1/admin/tier", { credentials: "include" });
  if (!resp.ok) throw new Error(`tier status fetch failed: ${resp.status}`);
  return resp.json();
}

// Error handler refinement — detect TierCapExceededBody from any 402/409.
export async function unwrapMutation<T>(resp: Response): Promise<T> {
  if (resp.ok) return resp.json();
  const body = await resp.json().catch(() => ({}));
  if (isTierCapExceeded(body)) {
    // Hand off to UpgradePrompt toast (Stage 2 component).
    showUpgradePromptToast(body);
  }
  throw new ApiError(resp.status, body);
}
```

---

## Stage 2 deferred decisions (from P3 plan, not Stage 1's responsibility)

| Decision | Default if Stage 2 doesn't ask | Source |
| --- | --- | --- |
| Polar checkout button copy | "Contact support to upgrade" (stub) | DEC-FBR-DEFER-01 |
| Usage-meter color thresholds | green <70%, amber 70–95%, red >95% | P3 plan §Stage 2 |
| `EU residency` capability render | Show ✓/✗ but no action — feature not implemented | P3 plan §Deferred Decisions |
| Custom domain capability render | Same as above | P3 plan §Deferred Decisions |

---

## Stage 1 exit-gate verification (THIS document's preconditions)

Stage 2 worker MAY assume the following are GREEN at handoff:

- `cargo build --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --no-fail-fast` (≥299 tests passing post-P3-S1)
- `cargo sqlx prepare --workspace -- --all-targets` clean
- `multi-tenant-isolation-check` oracle GREEN
- `pii-scrub-audit` oracle GREEN
- `widget-bundle-size` oracle GREEN
- `tier-enforcement-status` oracle GREEN (cold-start vacuous-PASS on
  Probe C; Phase 4 wiring + handler allowlist coverage active on Probes
  A + B; full Probe C smoke crate is Stage 2 / future-worker responsibility).
- `GET /api/v1/admin/tier` returns Contract C17 shape for any admin
  session.
- Free-tier 2nd project create → 409 with Contract C18 body.
- Free-tier 51st feedback in rolling month → 402 with Contract C18 body.
- `GET /api/v1/projects/{id}/widget-config` returns
  `footer_text: "powered by feedbackmonk"` for Free, `footer_text: null`
  for paid tiers.

---

## Lineage

- **FR-FBR-14** — Tier enforcement (caps + footer)
- **FR-FBR-15** — Polar billing (PROPOSED, DEFERRED — DEC-FBR-DEFER-01)
- **DEC-FBR-03** — Pricing tier matrix (Free / Starter / Pro / SelfHost)
- **DEC-FBR-DEFER-01** — Polar billing deferred from P3
- **P3 plan §Interface Contracts** — Contracts C17/C18/C19 source
- `docs/operations/TIER_OVERRIDE.md` — operator tier-flip workflow
- `docs/deferred/polar-integration.md` — Polar contract for future port
- `.claude/oracles/tier-enforcement-status/` — drift defender
