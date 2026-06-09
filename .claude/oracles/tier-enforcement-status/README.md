# tier-enforcement-status

**Kind**: Verification Oracle (Probandurgy — P3 Task Zero leg 2 of three-leg defense).

**Question**: Does every domain-write handler under
`crates/feedbackmonk-api/src/handlers/` either consult `check_tier_quota()`
before its first write OR appear in the allowlist? Does `tier_quotas()` in
`crates/feedbackmonk-core/src/tier.rs` return the Contract C19 canonical
shape per `Tier` variant? With `--full`: do the end-to-end cap-firing
smoke tests pass?

## Synopsis

Verification Oracle (P3 Task Zero) defending FR-FBR-14 tier enforcement: every domain-write handler under `crates/feedbackmonk-api/src/handlers/` either consults `check_tier_quota()` before its first write or is allowlisted, and `tier_quotas()` in `feedbackmonk-core::tier` returns the Contract C19 canonical shape per `Tier`. `--full` runs the end-to-end cap-firing smoke trio. Re-run after touching tier logic or adding a write handler.

## Probes

### Probe A — Handler tier-cap coverage (AST scan)

Walks every `pub async fn` in `crates/feedbackmonk-api/src/handlers/*.rs`
(skipping `mod.rs`). For each function whose body contains a domain-write
pattern (`.create(`, `.update*(`, `.submit_authenticated(`,
`.submit_anonymous(`, `.append_in_executor(`, `.cast(`, `.retract(`,
`.register(`, `.deactivate(`, `.set_status(`, `.redeem(`, `.mark_verified(`,
`.promote(`), the probe requires EITHER:

- A `check_tier_quota(` call that **precedes** the first write call, OR
- An entry in `allowlist.toml` with a documented rationale.

Same defensive pattern as `multi-tenant-isolation-check` Probe A. The
allowlist is the drift surface — every new exemption requires a rationale
comment that survives code review.

### Probe B — `tier_quotas()` config shape (static)

Reads `crates/feedbackmonk-core/src/tier.rs`. For each
`Tier::<Variant> => TierQuotas { ... }` arm, asserts the canonical token
set per **Contract C19** is present:

| Variant   | projects_per_org | monthly_feedback_volume | custom_branding | custom_domain | eu_residency | footer_text                          |
| --------- | ---------------- | ----------------------- | --------------- | ------------- | ------------ | ------------------------------------ |
| Free      | Some(1)          | Some(50)                | false           | false         | false        | Some("powered by feedbackmonk")      |
| Starter   | Some(3)          | Some(500)               | true            | false         | false        | None                                 |
| Pro       | None             | Some(10000)             | true            | true          | true         | None                                 |
| SelfHost  | None             | None                    | true            | true          | true         | None                                 |

Defends against accidental edits like setting Free to unlimited or
flipping the free-tier footer off. Token check is whitespace-insensitive
so rustfmt cosmetic changes don't churn.

> **`tier_quotas().footer_text` is the tier DEFAULT, not the final value**
> (DEC-FBR-IMPL-11). The per-tenant `footer_text_override` column (migration
> 00012) is resolved as a layer ABOVE this default in
> `SqlxTenantRepo::get_widget_brand` — `tier_quotas()` itself is deliberately
> unchanged, so this Probe B assertion (and the FR-FBR-14 default it pins) holds
> exactly as before. The override is admin-ops-only (it cannot be set by a
> tenant's own session), so external Free tenants still cannot strip the badge.
> The override behavior is verified by Probe C scenario 4 (below), not here.

### Probe C — Integration smoke (gated behind `--full`)

Invokes `cargo test --test tier_enforcement_smoke -p feedbackmonk-api`.
The smoke crate (Phase 4 deliverable) drives the actual HTTP path:

1. Free-tier tenant creates 2nd project → 409 + structured
   `tier_cap_exceeded` body.
2. Free-tier tenant submits 51st feedback in 30-day window → 402 + same
   body shape.
3. `GET /api/v1/projects/{id}/widget-config` for Free tenant returns
   `footer_text: Some("powered by feedbackmonk")`; for Pro/SelfHost
   returns `None`.
4. **(DEC-FBR-IMPL-11)** Footer/tier decoupling: a Free tenant with NO
   override still returns the badge (FR-FBR-14 default), and a Free tenant
   whose `footer_text_override = ""` returns `footer_text: null` (suppressed)
   while its tier — and therefore quotas — stay Free. Proves badge visibility
   is decoupled from tier and that the override supersedes the default.

Probe C is **off by default** so the inner-loop cost stays under 250ms.
CI (and `/0-uldf-finalize` Phase 11) re-run with `--full`.

**Cold-start vacuous-PASS plan**: if the smoke test crate doesn't exist
yet (Phase 4 not landed), Probe C reports vacuous PASS by detecting the
cargo "no test target named" error. This lets the oracle ship in Task
Zero before the wiring lands.

## Three-leg defense (per P3 plan § Testability Gate)

| Leg | Mechanism | File / location |
|---|---|---|
| 1. Type-system chokepoint | `Tier` enum + `TierQuotas` struct in `feedbackmonk-core/src/tier.rs`; `check_tier_quota(scope, ResourceKind) -> Result<QuotaStatus>` predicate in `feedbackmonk-repository/src/tier_quota.rs`. Exhaustive match in `ApiError::TierCapExceeded` mapping. | `crates/feedbackmonk-core/src/tier.rs`, `crates/feedbackmonk-repository/src/tier_quota.rs` |
| 2. AST / artifact oracle (this file) | Probe A (handler coverage) + Probe B (config shape) + Probe C (integration smoke, `--full`) | `.claude/oracles/tier-enforcement-status/` |
| 3. Integration tests | `sqlx::test` fixtures in `tier_enforcement_smoke.rs` exercise the cap-firing HTTP path end-to-end | `crates/feedbackmonk-api/tests/tier_enforcement_smoke.rs` |

## Invocation

```bash
# Unix / Git Bash / WSL — inner-loop fast path (A + B only):
bash .claude/oracles/tier-enforcement-status/oracle.sh

# Full loop (adds Probe C integration smoke):
bash .claude/oracles/tier-enforcement-status/oracle.sh --full

# Windows (PowerShell):
pwsh .claude/oracles/tier-enforcement-status/oracle.ps1
pwsh .claude/oracles/tier-enforcement-status/oracle.ps1 --full

# Direct Python (cross-platform):
python .claude/oracles/tier-enforcement-status/oracle.py
python .claude/oracles/tier-enforcement-status/oracle.py --full
```

Exit `0` on PASS, `1` on FAIL, `2` on environment failure (Python not found).

## Output schema

```
PASS tier-enforcement-status
  Probe A (handler tier-cap coverage): clean (crates/feedbackmonk-api/src/handlers)
  Probe B (tier_quotas() shape): clean (Contract C19 invariants hold)
  Probe C (integration smoke): cargo test --test tier_enforcement_smoke: GREEN
```

or

```
FAIL tier-enforcement-status (<N> probe(s) failed)

Probe A failures (handler missing tier-cap check):
  crates/feedbackmonk-api/src/handlers/projects.rs:42  projects::create  performs a domain write without check_tier_quota (add the check or allowlist with rationale)
  Remediation: add `state.tier_quotas.check_tier_quota(&scope, ResourceKind::*).await?` at the top of the handler BEFORE any data write, OR allowlist it in .claude/oracles/tier-enforcement-status/allowlist.toml with a documented rationale.

Probe B failures (tier_quotas() shape drift from Contract C19):
  tier.rs  Tier::Free arm missing canonical token `monthly_feedback_volume: Some(50)` (Contract C19 drift)
  Remediation: restore the canonical TierQuotas literal per docs/planning/plans/20260514T134816-feedbackmonk-p3-commercial-gate.md § Contract C19. Changing tier-cap defaults requires a spec-level decision (DEC-FBR-* entry).
```

Cold-start (Task Zero, before Phase 4 wiring lands):

```
PASS tier-enforcement-status
  Probe A (handler tier-cap coverage): clean (every handler with writes either consults check_tier_quota OR is allowlisted with rationale)
  Probe B (tier_quotas() shape): vacuous PASS — crates/feedbackmonk-core/src/tier.rs does not exist yet (pre-build)
  Probe C (integration smoke): skipped (pass --full to run integration smoke)
```

## Editing the allowlist

The `allowlist.toml` file gates Probe A. **Adding a handler is a
reviewable surface** — every entry requires a `rationale` line that a
reviewer can audit. Entries are tightly scoped to:

1. Pre-tier boundary handlers (signup, verify-email) — no tier exists yet.
2. Operational writes that don't produce a NEW chargeable resource (admin
   transitions, replies, signing-key registration, roadmap mutations).

Adding `tier_quotas()` flag changes (e.g. raising the Free monthly cap)
requires a `DEC-FBR-*` entry; the oracle's Probe B blocks silent drift.

## Why integration smoke is gated behind `--full`

Per P3 plan § Strategy Rationale: keeping the inner-loop fast (Probe A + B
only, <250ms) means agents can re-run after each edit without paying
fixture cost. The `--full` gate is the CI/finalize bound where the cost
is amortized.

## Lineage

- **FR-FBR-14** — Tier enforcement (caps + footer)
- **DEC-FBR-03** — Pricing tier matrix (Free / Starter / Pro / SelfHost)
- **P3 plan §Oracle Pre-Build Plan** — Probe A + Probe B + Probe C-gated
- **P3 plan §Testability Gate** — composite 16/25 → scaffolding pairing
- **Three-leg defense pattern** — type-system + oracle + integration

## Decision log

- **File-naming**: `oracle.{py,sh,ps1}` matches the existing oracle
  conventions in `widget-bundle-size`, `multi-tenant-isolation-check`,
  `pii-scrub-audit`. The brief said `manifest.toml` — kept as a TOML
  mirror; `manifest.json` is authoritative at runtime.
- **Write-pattern set**: conservative — `.create(`, `.submit_*`,
  `.update_*`, etc. Over-flagging is safe (covered by allowlist with
  rationale); under-flagging silently misses a write path.
- **Probe B token check is whitespace-insensitive**: rustfmt rewrites
  whitespace inside struct literals; the canonical-token comparison
  should not churn on cosmetic edits.
- **Probe C gated behind `--full`**: keeps cold-start vacuous-PASS and
  inner-loop fast (<250ms). `/0-uldf-finalize` Phase 11 + CI gate run
  with `--full`.
- **Cold-start vacuous-PASS for Probe C** when the test crate doesn't
  exist (matched on `no test target named tier_enforcement_smoke`):
  load-bearing for Task Zero — oracle lands BEFORE the smoke test
  crate, then re-evaluates as Phase 4 wiring + Phase 7 smoke crate
  land in subsequent commits.
