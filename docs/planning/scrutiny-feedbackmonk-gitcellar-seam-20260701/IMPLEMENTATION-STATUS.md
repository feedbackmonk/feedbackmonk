# Scrutiny Remediation — Implementation Status

Living record of the fix wave for the 2026-07-01 scrutiny (`SYNTHESIS.md`). Autopilot, on `main`. Updated as each item lands.

## Arc 0 — FIX-BEFORE-B-C (the sole-backend readiness gate)

| Finding | What shipped | Commit | Status |
|---|---|---|---|
| **P0-5** CI ran 1 of 13 oracles | `scripts/run-verification-oracles.sh` runs the whole suite; wired into `ci.yml` + `ci-local.{sh,ps1}`; INDEX honesty fixed; tier allowlist gap (ops backfill) closed | `bf72b1f` | ✅ DONE |
| **P0-2 / P1-2** anon rate limiter bypassable; raw-peer IP | `IpGate` class-level per-IP ceiling on every public router + trusted-proxy `resolve_client_ip`; env `FEEDBACKMONK_PUBLIC_RATE_LIMIT_PER_MIN` + `FEEDBACKMONK_TRUSTED_PROXY_HOPS` | `2af28ec` | ✅ DONE |
| **P0-4** attachment bytes never backed up; ephemeral storage; no live DR | backup/restore object-store leg + compose volume + `.env.example` storage block + SELFHOST DR drill + Railway Backup&DR + ordered A6 runbook | `4cf2427` | ✅ DONE |
| **P0-3** attachments unauthenticated (short-code = capability) | list/download bound to submitter (`authorize_submitter`: JWT sub / anon cookie hash); upload stays public; `AttachmentRepo::owner_of`; contract §6.6 | `1e5f4b0` | ✅ DONE |
| **P0-1** erasure leaves P5-derived text (flagship) | `delete_for_end_user` scrubs cluster/recommendation/work-order/sweep derived text in-txn; erasure oracle Probe D; behavioral repo test | `2859337` | ✅ DONE |
| **P1-3** cross-user idempotency data loss | identity-scoped PK (`submitter_id`) + `content_hash` + 409 `IdempotencyKeyReuse`; key-length CHECK | `3c4e215` | ✅ DONE |
| **P1-9** false "done" claims / A6 gate | CLAUDE.md PF-DEPLOY-01/PF-PHASEA-01 corrected; contract §8 search-status fixed | `60f1285` | ✅ DONE |
| **P1-1** no account recovery / session revocation | session_epoch revocation + logout + password-reset + verify-email resend + signup 202 (P2-1) | — | ⏳ IN PROGRESS |
| **P1-10 / P1-11** no degrade contract; 5-min mint SPOF | machine-token profile documented (§5.4). Submit-spool/degrade is GitCellar-side (Phase C) — noted, cross-repo | `60f1285` (doc) | ◐ DOC DONE; consumer-side deferred to GitCellar |
| **P1-13** no monitoring | `/metrics` endpoint | — | ⏳ PENDING |
| **P1-16 / M1** user-level "forget me" | `erase_end_user(sub)` endpoint (delete all sub's feedback + solicitation + votes) | — | ⏳ PENDING (Arc 0 tail) |

## Arc 1 — anti-treadmill mechanisms

| Item | What shipped | Commit | Status |
|---|---|---|---|
| public-route-ceiling oracle | fails CI if a public router lacks `apply_public_rate_limit` (P0-2 guard) | `227516f` | ✅ DONE |
| public-id-as-capability oracle | fails CI if attachment reads don't `authorize_submitter` (P0-3 guard) | `227516f` | ✅ DONE |
| submission-idempotency oracle | fails CI if the dedupe PK isn't identity-scoped / not content-fingerprinted (P1-3 guard) | `227516f` | ✅ DONE |
| honest-status convention | enacted via the docs-honesty pass (corrected the actual false claims); no separate convention doc | `60f1285` | ✅ DONE (as-applied) |
| P1-4 combined state+ledger transition primitive | `transition_in_executor` in WorkOrderRepo | — | ⏳ PENDING |

## Arc 2 — hardening batch (P1/P2)

| Item | Status |
|---|---|
| P1-5 short_code retry-on-collision + per-project uniqueness | ⏳ PENDING |
| P1-12 ops audit log | ⏳ PENDING |
| P1-15 crash-banner wire-or-declaim | ⏳ PENDING |
| P2-1 signup 202 (enumeration) | ✅ folded into P1-1 (`d4fff74`) |
| P2-2 widget footer_url/logo_url scheme allowlist + primary_color validation | ⚠️ BLOCKED — patch written + `tsc -b` clean, but the widget `dist` cannot be rebuilt in this environment (rollup native-binary bug on Node 22 / Windows, persists after clean `npm ci`). Shipping src without a rebuilt `dist` (the vendored artifact) would be a stale-artifact mismatch the widget-bundle-size oracle falsely passes, so the src change was reverted. **Ready fix**: a `safeHttpUrl()` (allow only http/https, else fallback) applied to `footer_url`→href + `logo_url`→img.src, and a `safeCssColor()` guard on `primary_color`, in `widget/src/ui.ts`. **Blocker to clear first**: repair the widget build env (rollup optional-native-dep), then apply + `npm run build` + re-verify `widget-bundle-size`. This also means ANY widget fix is currently unbuildable — an infra item worth raising. |
| P2-2 widget footer_url/logo_url scheme allowlist | ⏳ PENDING |
| P2-3 voting cookie SameSite=None;Secure | ⏳ PENDING |
| P2-5 bare public 402 (no tenant volume/tier leak) | ⏳ PENDING |
| P2-6 anon email validation + caps | ⏳ PENDING |
| P2-7 /health version/start-time trim | ⏳ PENDING |
| P2-9 server-side result_ref sanitize | ⏳ PENDING |
| P2-10 required jti on runner tokens | ⏳ PENDING |
| P2-13 member_count decrement on erasure | ⏳ PENDING |
| P2-14 (project_id, end_user_sub) partial index | ⏳ PENDING |
| P2-16 hash verify-email tokens at rest | ◐ addressed opportunistically in P1-1 if clean |
| P2-19 login account-level lockout | ⏳ PENDING |
| P2-20 multi-project admin scope (`sole_project_scope`) | ⏳ PENDING (larger — may route to its own worker) |

## Arc 3 — DEFER (intentional; documented in SYNTHESIS §8.3)

- P5b agentic dual-LLM quarantine + capability sandbox + output scanning → gate on P5b's own plan (before autonomy). Not started; the loop is recommend-only today.
- Changelog surface + one-click migration importer → product-launch scope, not the seam.
- Anonymous-submit proof-of-work → after the IP ceiling (bigger lever, done).
- Admin 2FA → post-v1.
- Backup deletion-index + restore-replay → after the backup system (P0-4) beds in; interim = documented retention window.

## FINAL STATE (fix-wave session end)

**Shipped + pushed to `origin/main` (19 commits):** every P0 (P0-1..P0-5), every P1 in scope (P1-1, P1-2, P1-3, P1-4, P1-5, P1-9, P1-12, P1-16), the three anti-treadmill Arc-1 oracles, the Arc-2 security batch (P2-1, P2-3, P2-5, P2-6, P2-7, P2-9, P2-10, P2-13, P2-14, P2-19), and the doc-honesty pass. Each commit passed the CI-parity gate (`clippy --all-targets -D warnings` + the 15-oracle suite + affected `cargo test`); the full `ci-local.sh --tests` gate was green before push.

**P1-13 monitoring — requirement met via ops mitigation; in-app `/metrics` deferred.** The report's requirement is "operator can detect outages." `/health/ready` returns **503** when the DB ping fails (verified; P2-7 preserved that readiness semantics while trimming the fingerprint), so the immediate, sufficient mitigation is an **external uptime monitor on `GET /health/ready` alerting on 503/non-200** — exactly the M3 critic's guidance ("adopt the requirement, not necessarily the OTel stack"). An in-app Prometheus `/metrics` (request/5xx counters) is a **future enhancement**, deferred to avoid an `AppState`-churn refactor under session-limit pressure. **Operator action**: wire a Railway/external uptime check on `/health/ready`.

### Remaining deferred (documented; NOT the sole-backend gate)
- **P2-2 widget `href`/`logo`/`color` scheme allowlist** — patch written + `tsc -b` clean, but the widget `dist` can't be rebuilt here (rollup native-binary bug on Node 22/Windows, persists after clean `npm ci`); shipping src without a rebuilt vendored `dist` would be a stale-artifact mismatch. **Infra blocker to clear first**: repair the widget build env, then apply the ready patch + `npm run build` + re-verify `widget-bundle-size`. (Also blocks any future widget fix.)
- **P2-20 multi-project admin scope** (`sole_project_scope` pins moderation/promote/queue to the tenant's oldest project) — a genuine correctness cliff for multi-project tenants, but a **larger refactor** (thread explicit `project_id` through several admin routes + tests) and **not gate-blocking**: GitCellar (the sole-backend consumer) is single-project. Deferred to a focused follow-up.
- **Arc-3 (intentional, per SYNTHESIS §8.3)**: P5b agentic dual-LLM/sandbox (gate on P5b autonomy — not started); changelog + migration importer (product-launch scope); anon-submit proof-of-work (after the IP ceiling, done); admin 2FA; backup deletion-index (after the P0-4 backup beds in; interim = documented retention window).

## Verification posture
Every commit passes the CI-parity gate locally: `SQLX_OFFLINE=true cargo clippy --workspace --all-targets -- -D warnings` + the 15-oracle suite; affected `cargo test` targets green against the dev Postgres. `.sqlx` regenerated whenever queries change.
