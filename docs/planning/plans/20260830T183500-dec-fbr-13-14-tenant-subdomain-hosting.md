# Execution Plan — FR-FBR-32 / FR-FBR-33: commercial hosting shape (DEC-FBR-13 + DEC-FBR-14)

**Source**: /0-uldf-ldis-plan
**Generated**: 2026-08-30T18:35:00Z
**Intake**: `docs/planning/intakes/20260830T182800-dec-fbr-13-14-tenant-subdomain-hosting.md`
**Brief**: `docs/planning/deferred/DEFER-005_tenant-subdomain-hosting-shape.md`
**Decisions**: DEC-FBR-13, DEC-FBR-14 (owner) · DEC-FBR-IMPL-27/28/29 (this lane)
**Autonomy**: autopilot · **Topology**: SEQUENTIAL (rung 0) — see intake Collaboration Assessment

---

## The one sentence that matters

Resolving a Host to a tenant is the easy half; **binding every public route to the resolved tenant
is the deliverable**, because a subdomain scheme without binding renders correctly, looks isolated,
and is not.

## Sequencing constraint (from the brief)

DEC-FBR-13 before DEC-FBR-14 — the CNAME target does not exist until subdomains do. Stages 0-5 are
DEC-FBR-13; Stage 6 is DEC-FBR-14 and is **runbook + code-readiness only** (the DNS cutover is
owner/ops and is explicitly not executed by this lane).

---

## Stage 0 — Task Zero: the oracle assertion, written FIRST

The brief instructs: *"A host-to-tenant Verification Oracle is a strong candidate — write its
`assertion` block first if you build one."* Done before any implementation, so the oracle cannot be
retrofitted to whatever the code happens to do.

**Oracle**: `.claude/oracles/host-tenant-binding/`

**Assertion** (frozen before code):
> When a public HTTP request arrives on a host that resolves to tenant T, no public route may return
> or act on a resource belonging to any tenant other than T; and the admin surface is reachable on
> exactly one host — the canonical admin host — and on no tenant subdomain, custom domain, or admin
> alias.

Four probes, **detection-from-code** (never a self-reported flag), mirroring
`public-route-ceiling` (a floor every public router must carry) and `public-board-moderation-gate`
(parse the source, then run behaviour):

- **A — guard coverage.** Parse `build_app`: every router merged as a *public* surface (the ones
  already wrapped in `apply_public_rate_limit`, plus `widget_config_router`) must also be wrapped in
  `bind_public_routes`. A new public route that forgets the guard fails here, which is the whole
  point — this is the recurrent failure mode.
- **B — admin exclusivity.** Parse the host layer: the admin/ops/moderation/work-order routers must
  be reachable only under `HostKind::Admin`, and no admin-session cookie may be issued on a
  non-admin host.
- **C — resolution purity.** `resolve_host` must be the single host→tenant entry point, must live in
  the repository crate (DEC-FBR-03: sole query path), and the API must never compare a raw `Host`
  string to a tenant identifier outside it.
- **D — `--full` behavioural.** Run `tests/host_tenant_binding.rs`: tenant A's host + tenant B's
  project ⇒ 404 on board, roadmap, widget-config and submit; admin route on a tenant host ⇒ 404;
  unconfigured root domain ⇒ every legacy route behaves exactly as before.

**Graceful absence**: vacuous-PASS when `FEEDBACKMONK_ROOT_DOMAIN` is unset in the tree's config —
a self-host checkout must not go red for not using a SaaS feature.

---

## Stage 1 — Schema + core vocabulary

`migrations/00030_tenant_hosting.sql`:
- `tenants.subdomain TEXT UNIQUE` (nullable; NULL = no public subdomain yet — every existing row is
  unaffected on deploy, the same posture `public_board_enabled DEFAULT FALSE` took in 00016).
- `tenant_domains` — `id`, `tenant_id` FK CASCADE, `domain TEXT NOT NULL UNIQUE` (lowercased),
  `kind TEXT CHECK (kind IN ('public','admin_alias'))`, `status TEXT CHECK (status IN
  ('pending','active'))`, `created_at`, `verified_at`.
  - `public` = the sellable FR-FBR-33 surface (board + widget/API).
  - `admin_alias` = operator-registered only, 301s to the canonical admin host (DEC-FBR-IMPL-27).
    Not sellable, not tenant-creatable.

`crates/feedbackmonk-core/src/hosting.rs`:
- `normalize_host` (lowercase, strip port, strip trailing dot).
- `validate_subdomain_label` — RFC-1123 label: 3-63 chars, `[a-z0-9-]`, no leading/trailing hyphen,
  not all-numeric.
- `RESERVED_LABELS` — `www api cdn app admin dashboard mail smtp ftp static assets docs blog
  status support help account accounts billing login auth signup ns1 ns2 test staging dev
  feedbackmonk monk` (+ the canonical admin label). Claiming one is a 409.
- Unit tests for each rule, including the reserved set and the all-numeric case.

## Stage 2 — Repository layer (DEC-FBR-03 discipline)

`crates/feedbackmonk-repository/src/domains.rs`:
- `DomainRepo::resolve_host(host) -> Option<HostBinding>` — **pre-auth boundary**, allow-listed in
  `multi-tenant-isolation-check/allowlist.toml` with the same rationale shape as
  `open_for_submission`: it takes untrusted input and *mints* a scope rather than accepting one.
  Single SQL statement over `tenants.subdomain` ∪ `tenant_domains`.
- Scope-bound tenant-facing methods: `list_for_tenant`, `claim`, `delete` — all `&TenantScope`-first.
- `SqlxTenantRepo::set_subdomain(&TenantScope, label)` + `find_by_subdomain` (pre-auth, allow-listed).
- `sqlx::test` coverage: resolution hit/miss, cross-tenant claim collision ⇒ `Conflict`, cascade on
  tenant delete.

## Stage 3 — Host scope + the binding guard (the load-bearing stage)

`crates/feedbackmonk-api/src/hosting.rs`:
- `HostConfig` from env — `FEEDBACKMONK_ROOT_DOMAIN`, `FEEDBACKMONK_ADMIN_HOST`. Both unset ⇒
  `HostConfig::disabled()` and the whole layer is a pass-through (DEC-FBR-IMPL-28 part 3).
- `HostKind` = `Unbound | Admin | Tenant { tenant_id, source } | AdminAlias { .. } | Unknown`.
- Resolution order: canonical admin host → admin alias → `*.root_domain` label → `tenant_domains` →
  apex/reserved → unknown.
- `bind_public_routes(router, state)` — a `middleware::from_fn_with_state` layer that (i) resolves
  the host, (ii) stashes `HostScope` as a request extension, (iii) when the host is tenant-bound and
  the matched path carries a `project_id`, verifies the project belongs to that tenant and returns
  **404** otherwise, (iv) 301s admin aliases, (v) 404s admin routes on non-admin hosts.
- Applied in `build_app` to the same set the rate-limit floor covers, plus `widget_config_router`.
- `X-Forwarded-Host` honoured only when `FEEDBACKMONK_TRUSTED_PROXY_HOPS > 0` — reusing the existing
  trusted-proxy switch rather than inventing a second trust knob.

## Stage 4 — Public discovery + the edge authorisation seam

`crates/feedbackmonk-api/src/handlers/public_site.rs`:
- `GET /api/v1/public/site` — for the current host: `{ tenant_id, subdomain, projects: [{id, slug,
  name, public_board_enabled}] }`, or 404 on an unbound/unknown host. This is what lets the public
  SPA drop `project_id` from the URL without forking a single handler.
  **Privacy**: project metadata only — no tenant email, no counts, no feedback. Mirrors the
  widget-config posture.
- `GET /api/v1/public/tls-authorize?domain=` — the Caddy `on_demand_tls.ask` seam. 200 iff the host
  is known **and** (for a `public` custom domain) the owning tenant's tier still carries
  `custom_domain`. 404 otherwise. Metadata-free: the body is empty, the status is the whole answer.

## Stage 5 — Tenant-facing domain management + admin UI + edge config

- `handlers/domains.rs` — `GET|POST /api/v1/admin/domains`, `DELETE /api/v1/admin/domains/{id}`,
  and `PUT /api/v1/admin/subdomain`. All behind `AdminSession`. Claim is **tier-gated server-side**:
  no `custom_domain` capability ⇒ **402**, matching the existing cap-exceeded convention the
  admin-UI axios interceptor already tags.
- `admin-ui/src/pages/settings/DomainSettings.tsx` at `/admin/settings/domains` — subdomain editor,
  claimed-domain list with status, CNAME instructions, and an upgrade prompt on 402 (reusing
  `UpgradePrompt`). Vitest + an axe-core a11y spec, matching `TierSettings`.
- `deploy/caddy/Caddyfile` + compose profile — the reference edge: wildcard TLS for
  `*.{root}`, on-demand TLS for custom domains gated by `tls-authorize`. Documented in
  `docs/operations/SELFHOST_ENV.md` (Contract C21) so `selfhost-compose-smoke` Probe B stays green.
- `TierSettings.tsx`: drop `notImplemented` from the custom-domain row — it stops being a lie.

## Stage 6 — DEC-FBR-14 (readiness only; the cutover is NOT executed)

- `docs/operations/SAAS_HOSTING.md` — stand-up runbook for `feedbackmonk.com`: wildcard DNS, wildcard
  TLS, env matrix, and the ordered GitCellar migration (create tenant → set subdomain → register
  `feedback.gitcellar.com` as a `public` domain and `triage.gitcellar.com` as an `admin_alias` →
  data migration → DNS cutover → verify → rollback).
- **No GitCellar repo edit.** `TRIAGE_URL` is untouched by construction: the alias 301 is why.
- **Blocked, surfaced to the owner, not stalled on**: the hosting account + DNS/TLS provisioning, and
  the `feedback.gitcellar.com` cutover (live GitCellar sessions on this machine may be measuring
  against that surface — the brief says coordinate first).

---

## Verification gates

| Gate | Command | Must be |
|---|---|---|
| CI parity (mandatory before any push) | `bash scripts/ci-local.sh` | green |
| sqlx offline cache | `cargo sqlx prepare --workspace -- --all-targets` + commit `.sqlx/` | committed |
| Multi-tenant isolation (must not regress) | `/0-uldf-oracle multi-tenant-isolation-check` | PASS |
| Host binding (new) | `/0-uldf-oracle host-tenant-binding --full` | PASS |
| Tier enforcement | `/0-uldf-oracle tier-enforcement-status` | PASS |
| Self-host compose (Caddy must not break it) | `/0-uldf-oracle selfhost-compose-smoke` | PASS |

## Explicit non-goals

Custom email `From:` (DEC-FBR-13 defers it) · splitting the board out of the admin bundle (host
separation already provides the origin split DEC-FBR-13 reason #1 asks for; tracked as optional
follow-on) · migrating the existing `project_id` routes away (DEC-FBR-IMPL-28 part 1) · executing
any DNS change.
