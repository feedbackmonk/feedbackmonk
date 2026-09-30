# host-tenant-binding — Verification Oracle

## Summary

The single `build_app` wiring check. It proves from source that every router merged in
`crates/feedbackmonk-api/src/main.rs::build_app` is classified public, admin or
intentionally-unbound, and wired exactly as its class demands — host binding (FR-FBR-32,
DEC-FBR-13), the per-IP rate floor (P0-2), and credentialed CORS on exactly the embed routers
(DEC-FBR-IMPL-09) — plus the host layer's own refusal semantics. Come here when you add,
rename or rewire a router, or touch `hosting.rs`.

Run `python .claude/project-oracles/host-tenant-binding/oracle.py` (`--full` adds the cargo
behavioural leg; `--root <path>` points it at another tree, used by the self-test). Exit 0 PASS,
1 FAIL, 2 environment error. Static run ≈ 0.5 s.

## Assertion

> When a public HTTP request arrives on a host that resolves to tenant T, no public route may
> return or act on a resource belonging to any tenant other than T; and the admin surface is
> reachable on exactly one host — the canonical admin host — and on no tenant subdomain, custom
> domain, or admin alias.

Frozen at Task Zero, before any implementation existed, per the DEFER-005 brief.

## Why one classification table

Resolution without binding is invisible: a missing guard renders a correct-looking page of
another tenant's feedback on your origin, and no happy-path test sees it. The same is true of a
missing rate floor or a missing CORS layer. The two oracles this one absorbed on 2026-09-30 —
`public-route-ceiling` (rate floor) and `cors-allowlist-enforcement` Probe A (CORS wiring) —
each checked a *hand-kept list of routers that should carry a wrapper*, so a router missing from
the list was never examined: four public routers (`widget_config_router`, `me_feedback_router`,
`me_feedback_data_router`, `solicitation_router`) sat bound-but-unlimited that way until
2026-09-30, and `tenant_settings_router` was absent from this oracle's own admin list.

Now `ROUTERS` in `oracle.py` is the one table, and **every** merged router must be in it:

| Class | Required merge shape |
|---|---|
| public | `bind_public_routes(apply_public_rate_limit(<r>(..)[.layer(cors..)], prl), hs)` — the `cors` column makes `.layer(cors` **required** (submission, attachments, board) or **forbidden** (everything else, notably `widget_config_router`, which is `*`-public with `credentials:'omit'`) |
| admin | `bind_admin_routes(<r>(..), hs)` — no other wrapper, no layer |
| unbound | `<r>(..)` bare, with a written reason: `health_router`, `capabilities_router` |

CORS *policy* (echo-origin, credentials, never `*`) is not checked here — `tests/cors_preflight.rs`
covers it behaviourally.

## Probes

| Probe | What it proves |
|---|---|
| **A** — build_app wiring | comments stripped, then the `let app = …;` chain is parsed paren-balanced: every element is `.merge(` (a chain `.layer(`/`.route(` FAILS); no `.merge(` outside that chain; no inline `.route/.nest/.fallback`; every merged router classified (unclassified FAILS), merged once (twice FAILS), and in its class's exact shape; every classified router still merged; the wrappers' second args are `prl`/`hs`; `let cors = public_cors_layer(..)`, `let prl = PublicRateLimit::new(..)` and `FEEDBACKMONK_CORS_ORIGINS` present; `.layer(cors` appears nowhere but on a cors=yes router |
| **B** — host layer | `bind_public_routes`/`bind_admin_routes` install `public_host_binding`/`admin_host_binding` respectively; `admin_host_binding` maps `Tenant` → `not_found()` and `AdminAlias` → `redirect_to_admin`; `public_host_binding` compares the path project's tenant to the host's tenant and 404s both mismatch and unknown; no `StatusCode::FORBIDDEN` (a 403 is an existence oracle); `effective_host` gates `x-forwarded-host` on `trust_forwarded_host` |
| **C** — resolution purity | host resolution lives in the repository crate (`sqlx::query!` in `domains.rs`, DEC-FBR-03); no `resolve_host(` call outside the allowed files; no ad-hoc `Host` header read outside `hosting.rs` |
| **D** — behavioural (`--full`) | `cargo test --test host_tenant_binding` + `--test domains_repo` |

A missing `main.rs`, `hosting.rs` or repository `domains.rs` is a **FAIL**. The former
"vacuous PASS when the host layer is absent" branch is gone: the host layer exists in this
codebase, so its absence is a regression. Self-host inertness (no root domain configured) is a
runtime property, asserted by Probe D's `unconfigured_deployment_is_inert`.

Adding a router means adding a row to `ROUTERS` with its class and reason. That friction is the
feature.

## Adversarial self-test (2026-09-30)

Harness: every `crates/**/*.rs` copied to a scratch tree, the oracle run with `--root <scratch>`,
one mutation at a time, restored and re-run green after each. Baseline: the scratch copy of the
live tree, exit **0** — the other session's edit wrapping the four named routers in
`apply_public_rate_limit` had already landed in `main.rs`, so no hand-wrapping of the baseline was
needed. All 28 mutations exit **1**, all 28 restores exit **0**.

| # | Mutation | Probe | Exit |
|---|---|---|---|
| 1 | merge an unclassified `evil_router` (fully wrapped) | A | 1 |
| 2 | merge `roadmap_router` twice | A | 1 |
| 3 | drop `apply_public_rate_limit` from `roadmap_router` | A | 1 |
| 4 | drop `apply_public_rate_limit` from `solicitation_router` | A | 1 |
| 5 | drop `.layer(cors)` from `submission_router` | A | 1 |
| 6 | replace `attachments_router`'s cors layer with another `.layer(` | A | 1 |
| 7 | add `.layer(cors)` to `widget_config_router` | A | 1 |
| 8 | swap `bind_admin_routes` → `bind_public_routes` on `ops_router` | A | 1 |
| 9 | drop `bind_admin_routes` from `tenant_settings_router` | A | 1 |
| 10 | drop `bind_public_routes` from `board_router` | A | 1 |
| 11 | `moderation_router`'s wrapper present only inside a `/* */` comment | A | 1 |
| 12 | wrap `health_router` (classified unbound) | A | 1 |
| 13 | global `.layer(cors)` on the `let app` chain | A | 1 |
| 14 | inline `.route(` in the chain | A | 1 |
| 15 | `let app = app.merge(..)` after the chain | A | 1 |
| 16 | replace `public_cors_layer` with `CorsLayer::permissive()` | A | 1 |
| 17 | remove `PublicRateLimit::new` | A | 1 |
| 18 | rate-limit budget arg not `prl` | A | 1 |
| 19 | delete `main.rs` | A | 1 |
| 20 | `admin_host_binding`: `Tenant` → `next.run` | B | 1 |
| 21 | `admin_host_binding`: `AdminAlias` served | B | 1 |
| 22 | `bind_public_routes` installs `admin_host_binding` | B | 1 |
| 23 | `public_host_binding` drops the tenant comparison | B | 1 |
| 24 | a `StatusCode::FORBIDDEN` in hosting.rs | B | 1 |
| 25 | `effective_host` trusts `x-forwarded-host` unconditionally | B | 1 |
| 26 | delete `hosting.rs` | B | 1 |
| 27 | second `resolve_host(` caller in a handler | C | 1 |
| 28 | handler reads `header::HOST` directly | C | 1 |

The paren-balanced parser matters: real arguments nest
(`bind_public_routes(apply_public_rate_limit(board_router(state).layer(cors.clone()), prl), hs)`),
and a regex split truncates at the first `)` — a silent false PASS. Comment stripping matters
too: #11 passed the pre-2026-09-30 oracle, which matched wrapper names in raw text.

## Files

| File | Role |
|---|---|
| `oracle.py` | the implementation and the `ROUTERS` table |
| `oracle.json` | the oracle contract |
| `README.md` | this file |

## History

Installed 2026-09-01 under `.claude/oracles/`; deleted by `5d858d2` (2026-09-07); restored
2026-09-11 as `oracle.py` only; put on `scripts/run-verification-oracles.sh` 2026-09-14 (Probes
A–C gate every push; D stays `--full`). Redone 2026-09-30 as the single `build_app` wiring check,
absorbing `public-route-ceiling` and `cors-allowlist-enforcement` Probe A, dropping the
vacuous-PASS branch, and adding `--root`.

The install also appended two entries to
`.claude/project-oracles/multi-tenant-isolation-check/allowlist.toml` —
`DomainRepo::resolve_host` (a genuine pre-auth boundary, like `ProjectRepo::open_for_submission`)
and `SqlxDomainRepo::new` (a pool constructor).
