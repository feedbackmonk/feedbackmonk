# host-tenant-binding — Verification Oracle

**Status: installed and green.** Run
`python .claude/project-oracles/host-tenant-binding/oracle.py` (add `--full` for
the behavioural leg, which drives `cargo test`).

## Assertion

> When a public HTTP request arrives on a host that resolves to tenant T, no
> public route may return or act on a resource belonging to any tenant other than
> T; and the admin surface is reachable on exactly one host — the canonical admin
> host — and on no tenant subdomain, custom domain, or admin alias.

Frozen at Task Zero, before any implementation existed, per the DEFER-005 brief's
explicit instruction.

## Why it exists

DEC-FBR-13 chose tenant subdomains over path-based multi-tenancy for **one**
decisive reason: origin isolation for the user-generated-content surface. That
reason is bought only by **binding** — the restriction that a request on tenant
A's host cannot reach tenant B's data.

Resolution alone buys nothing, and its absence is **invisible**: a missing guard
renders a perfectly correct-looking page of someone else's feedback on your
origin. Nothing errors. No happy-path test can see it. The subdomain scheme would
ship, look isolated, and not be — which is strictly worse than not shipping it,
because it would be believed.

`multi-tenant-isolation-check` does **not** cover this. That oracle polices the
*repository scope* axis (does every repo method take a `&TenantScope`). Host
binding is a second, orthogonal axis introduced by FR-FBR-32, and a router merged
without the guard passes that oracle completely unchanged.

## Probes

| Probe | What it proves | From |
|---|---|---|
| **A** — guard coverage | every public router in `build_app` is wrapped in `bind_public_routes`, every admin router in `bind_admin_routes` | `main.rs` source |
| **B** — admin exclusivity | `admin_host_binding` maps `HostScope::Tenant` → 404 and `AdminAlias` → 301; never 403 (an existence oracle); `X-Forwarded-Host` gated on the declared trusted proxy | `hosting.rs` source |
| **C** — resolution purity | exactly one host→tenant path, inside the repository crate (DEC-FBR-03); no ad-hoc `Host` reads elsewhere | crate-wide scan |
| **D** — behavioural (`--full`) | `tests/host_tenant_binding.rs` + `tests/domains_repo.rs` | cargo |

Probe A is the anti-treadmill leg, modelled directly on `public-route-ceiling`:
**adding a new public route means adding it to `PUBLIC_ROUTERS` in `oracle.py`
and wrapping it in `build_app`.** That friction is the feature — the recurrent
failure mode for both oracles is a route added later that silently fails to
inherit a floor.

Two routers are deliberately unbound, recorded in `INTENTIONALLY_UNBOUND` with
reasons rather than left to be inferred from absence: `health_router` (an
orchestrator probe may arrive on any hostname, and health carries no tenant data,
so a 404-by-hostname would pull a healthy instance out of rotation for no
security gain) and `capabilities_router` (deployment metadata only).

## Graceful absence

Vacuous-PASS when the host layer is not in the tree. A self-host checkout that
never sets `FEEDBACKMONK_ROOT_DOMAIN` must not go red for declining a SaaS
feature — the binding is inert by design there (DEC-FBR-IMPL-28 part 3), and that
inertness is itself asserted by Probe D's `unconfigured_deployment_is_inert`.

## Adversarial self-test (performed during authoring)

| Sabotage | Result |
|---|---|
| drop `bind_public_routes` from `widget_config_router` | Probe A **FAIL**, names it, exit 1 |
| drop `bind_admin_routes` from `moderation_router` | Probe A **FAIL**, names it |
| flip the cross-tenant refusal from 404 to 403 | Probe B **FAIL** |
| all restored | **PASS** A/B/C |

One implementation detail worth keeping: Probe A walks `.merge(` arguments with
balanced parentheses rather than splitting on a regex. The real arguments nest —
`bind_public_routes(apply_public_rate_limit(board_router(state), prl), hs)` — and
a naive split truncates at the first `)`, which produces a silent **false PASS**.
That is the failure an oracle can least afford.

## Files

| File | Role |
|---|---|
| `oracle.py` | the implementation — the only entry point |
| `manifest.json` | the oracle contract (probes, triggers, self-test) |
| `README.md` | this file |

The install (DEFER-006, 2026-09-01) also appended two entries to
`.claude/project-oracles/multi-tenant-isolation-check/allowlist.toml`; those survived the
deletion below and are still in place.

## History

Installed 2026-09-01 under `.claude/oracles/`. Deleted by `5d858d2` (2026-09-07, the
framework's shape-based retirement of everything without `schema: oracle/2` and without a
script consumer); missed by the `1ac27a6` restore because it was never on
`scripts/run-verification-oracles.sh`'s list, and then recorded in `CLAUDE.md` as never built.
Restored here 2026-09-11 as `oracle.py` only — the `.sh`/`.ps1` shims were pure delegators and
were not brought back.

The allow-list additions were two entries — `DomainRepo::resolve_host` (a genuine
pre-auth boundary, the same shape as `ProjectRepo::open_for_submission`) and
`SqlxDomainRepo::new` (a pool constructor). The first draft of the repository
surface produced seven offenders; five were removed by reshaping the code rather
than by widening the allow-list — the DB-value enums moved to
`feedbackmonk-core::hosting` beside `Tier` and `ModerationStatus`, and
`mark_active` became `&TenantScope`-first.
