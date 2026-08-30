# feedbackmonk — SaaS hosting runbook (DEC-FBR-13 / DEC-FBR-14)

**Scope**: standing up the multi-tenant SaaS at `feedbackmonk.com`, and migrating the first-party
products (GitCellar, then quiqpic and SessionHelm) onto it as **tenants** rather than separate
self-hosted instances.

**This is not the self-host story.** Plain self-hosters follow
[`SELFHOST.md`](SELFHOST.md) and are untouched by everything here: with
`FEEDBACKMONK_ROOT_DOMAIN` and `FEEDBACKMONK_ADMIN_HOST` unset, the host layer is inert and behaviour
is byte-identical to a deployment that predates FR-FBR-32.

> **Status (2026-08-30): the CODE is complete and tested; the OPS half is not started.** Nothing
> below has been executed. The two blocked items are called out in § 5 — both need a hosting account
> and a DNS panel, neither of which a build session has. **The GitCellar DNS cutover in § 4 must be
> coordinated with the GitCellar side before anyone runs it** (live GitCellar sessions may be
> measuring against `feedback.gitcellar.com`), and **no GitCellar source file is edited by any step
> here** — that is a hard constraint of DEC-FBR-14, not a preference.

---

## 1. What the hosting shape is

Per [`DEC-FBR-13`](../specs/DECISIONS.md#dec-fbr-13-public-surface-url-shape--tenant-subdomain-by-default-customer-custom-domain-as-the-paid-upgrade):

| Surface | Host | Custom domain? |
|---|---|---|
| Public board / roadmap | `{tenant}.feedbackmonk.com` | **Yes** — paid tier, via CNAME |
| Widget script + API endpoint | `{tenant}.feedbackmonk.com` | **Yes** — same CNAME |
| Admin / triage | `app.feedbackmonk.com` — **only** | **Never** |
| Transactional email `From:` | feedbackmonk-owned sending domain | Deferred (needs DKIM delegation) |

The mechanism (FR-FBR-32) is **host binding**, not merely host routing: on a host that resolves to
tenant T, every public route is restricted to T's projects and another tenant's `project_id` returns
**404**. That restriction is what DEC-FBR-13's origin-isolation argument actually buys — resolution
on its own would render a correct-looking page of another tenant's feedback on your origin. Guarded
by the `host-tenant-binding` Verification Oracle.

## 2. Standing up `feedbackmonk.com`

### 2.1 DNS

| Record | Type | Value | Why |
|---|---|---|---|
| `feedbackmonk.com` | A / ALIAS | marketing site | FR-FBR-16 Astro site; **not** the app |
| `app.feedbackmonk.com` | A / CNAME | the app edge | the one admin origin |
| `*.feedbackmonk.com` | A / CNAME | the app edge | tenant subdomains |

The wildcard covers **exactly one level**. The app deliberately refuses to bind `a.b.feedbackmonk.com`
— a wildcard certificate cannot cover it, so binding it would mint a tenant scope for a hostname that
can never present a valid certificate.

### 2.2 TLS

Two mechanisms, and they are not interchangeable:

- **`*.feedbackmonk.com`** — a wildcard certificate, which requires **DNS-01**. That means a Caddy
  build with your DNS provider's module (`xcaddy build --with github.com/caddy-dns/<provider>`) and
  an API token in the environment. Without DNS-01 each subdomain gets its own HTTP-01 certificate,
  which works but is chattier and slower on a tenant's first visit.
- **Customer custom domains** — **on-demand TLS**, gated by the app's `ask` endpoint. See § 3.

Reference configuration: [`deploy/caddy/Caddyfile`](../../deploy/caddy/Caddyfile).

### 2.3 App environment

Beyond the standard catalog in [`SELFHOST_ENV.md`](SELFHOST_ENV.md):

```bash
FEEDBACKMONK_ROOT_DOMAIN=feedbackmonk.com
FEEDBACKMONK_ADMIN_HOST=app.feedbackmonk.com
FEEDBACKMONK_TRUSTED_PROXY_HOPS=1     # REQUIRED behind any edge/PaaS
```

**`FEEDBACKMONK_TRUSTED_PROXY_HOPS=1` is not optional here.** Behind a proxy the real client-facing
hostname arrives in `X-Forwarded-Host`, and that header is honoured **only** when a trusted proxy is
declared. Leave it at `0` and every tenant host resolves to the internal proxy hostname instead —
which fails closed (nothing resolves), so the symptom is "no tenant host works", not a silent leak.

### 2.4 Verify before pointing any real DNS at it

```bash
# Admin answers on the admin host, and only there.
curl -sS -o /dev/null -w '%{http_code}\n' -H 'Host: app.feedbackmonk.com' \
  https://<edge>/api/v1/admin/tier          # expect 401 (handler, not guard)
curl -sS -o /dev/null -w '%{http_code}\n' -H 'Host: acme.feedbackmonk.com' \
  https://<edge>/api/v1/admin/tier          # expect 404 (guard)

# A tenant host describes itself, and nobody else.
curl -sS -H 'Host: acme.feedbackmonk.com' https://<edge>/api/v1/public/site

# Cross-tenant read is refused.
curl -sS -o /dev/null -w '%{http_code}\n' -H 'Host: acme.feedbackmonk.com' \
  https://<edge>/api/v1/projects/<OTHER-TENANTS-PROJECT>/board   # expect 404
```

## 3. Custom domains (FR-FBR-33)

**Tenant side**, in the admin console at `/admin/settings/hosting`:

1. Claim the hostname (`POST /api/v1/admin/hosting/domains`). Tier-gated: a tier without
   `custom_domain` gets **402** with the upgrade hint.
2. Create a CNAME: `feedback.customer.com → {tenant}.feedbackmonk.com`.
3. The certificate is issued automatically on the first HTTPS request.

**How issuance is authorised** (DEC-FBR-IMPL-29): the edge asks the app before ordering a
certificate for an unfamiliar SNI.

```
GET /api/v1/public/tls-authorize?domain={sni}    200 → issue    404 → refuse
```

Two consequences worth understanding before changing anything here:

- **The tier gate is enforced at issuance, not only at claim.** A tenant who downgrades off Pro keeps
  their claim row but stops getting certificates. A claim-time check alone would let someone claim on
  Pro, downgrade, and serve forever.
- **The `ask` endpoint is what stops unbounded issuance.** On-demand TLS without one lets anyone who
  points a DNS record at your edge trigger ACME orders and exhaust your account's rate limit.

**Known limitation (documented, not fixed):** a tenant can *claim* a hostname they do not own,
denying it to another tenant, because the claim precedes the DNS proof. They gain nothing — no
certificate is issued and no traffic reaches them without controlling the DNS — but they can squat
the name. The fix is a re-claim window on stale `pending` rows; it is deferred, and recorded in
§ "Deferred" below rather than left implicit.

## 4. Migrating GitCellar onto the SaaS (DEC-FBR-14)

GitCellar currently runs its own single-tenant instance on its Railway
(`docs/planning/feedbackmonk-deploy-state.md`). The migration makes it **tenant #1 of the SaaS** with
**no user-visible change and no GitCellar source edit**.

### 4.1 The `triage.gitcellar.com` question — settle it before you start

DEC-FBR-13 says admin lives on exactly one host and custom-domain admin is "Never". DEC-FBR-14 says
`triage.gitcellar.com` keeps working with `TRIAGE_URL` untouched. Both hold via
[`DEC-FBR-IMPL-27`](../specs/DECISIONS.md): `triage.gitcellar.com` is registered as an **admin
alias** and answered with a **301 to `app.feedbackmonk.com`**, preserving path and query.

So: admin is *served* from one origin (no per-domain session, no admin cookie on a customer domain),
the existing link still lands on triage, and `apps/gitcellar-cloud/admin-ui/src/featureFlags.ts` is
never touched. **If a migration plan you are following requires editing that constant, the plan has
misread DEC-FBR-14 — stop and re-read it.**

### 4.2 Order of operations

Each step is reversible until step 6.

1. **Create the tenant** on the SaaS instance and set its subdomain (`gitcellar`), giving
   `gitcellar.feedbackmonk.com` — the CNAME target the next steps need.
2. **Migrate the data** from GitCellar's instance into the SaaS database. Both run the same schema,
   so this is a filtered `pg_dump` of the GitCellar tenant's rows. Preserve `projects.id` — the
   deployed widget embeds carry `data-project-id`, and changing it breaks every embed in the field.
3. **Register the hosts**, both against the GitCellar tenant:
   - `feedback.gitcellar.com` → `kind = 'public'` (board + widget/API)
   - `triage.gitcellar.com` → `kind = 'admin_alias'` (301 to the admin host)

   The alias kind is **operator-registered only** — it is not offered on the tenant-facing surface and
   is not sold. It exists to keep first-party links alive across this migration.
4. **Verify against the SaaS with the Host header spoofed locally**, before any DNS moves:
   ```bash
   curl -sS -H 'Host: feedback.gitcellar.com' https://<saas-edge>/api/v1/public/site
   curl -sS -o /dev/null -w '%{http_code} %{redirect_url}\n' \
     -H 'Host: triage.gitcellar.com' https://<saas-edge>/admin/moderation   # 301 → app.feedbackmonk.com/…
   ```
5. **Lower the TTL** on both records to 60s and wait out the old TTL.
6. **Cut over DNS**: repoint both CNAMEs at the SaaS edge. Certificates issue on first request via the
   `ask` endpoint.
7. **Verify live**: `/api/v1/capabilities`, an anonymous submit, the board read, and the triage 301.
8. **Restore the TTL**, and leave the old instance running (not serving) for a rollback window.

**Rollback**: repoint the two CNAMEs back. The old instance still has its data; anything submitted
during the SaaS window lives only in the SaaS database, so a rollback after real traffic needs a
delta migration — which is why the TTL is lowered first and the window kept short.

### 4.3 Constraints that bind this section

- **No commit to the GitCellar repo.** DNS + data only. (DEC-FBR-07 and DEC-FBR-14.)
- **Coordinate first.** Live GitCellar sessions exist on this machine and may be measuring against
  `feedback.gitcellar.com`. A DNS move under them is a surprise, not a deploy.
- **`TRIAGE_URL` is not edited.** See § 4.1.

## 5. Blocked on the owner

| # | Item | Why it is blocked | Recommendation |
|---|---|---|---|
| 1 | Provision the `feedbackmonk.com` deployment: hosting account, wildcard DNS, wildcard TLS (DNS-01 token) | Needs a hosting account and DNS panel access | Run it **separately from GitCellar's Railway**. The current arrangement — the vendor's SaaS living inside customer #1's infrastructure — is the thing DEC-FBR-14 is trying to undo, and reproducing it would leave the dogfooding gap exactly where it was |
| 2 | Execute the GitCellar cutover (§ 4) | Needs #1, and coordination with the GitCellar side | Do it after #1 has served synthetic traffic for a few days. GitCellar is the dogfood, not the smoke test |

Neither blocks any further **code**. FR-FBR-32/33 are complete and tested against a real Postgres;
what is missing is a place to run them.

## 6. Deferred

- **Stale-`pending` custom-domain re-claim window** — closes the squatting limitation in § 3.
- **Custom email `From:` per tenant** — DEC-FBR-13 defers it on its own terms (DKIM delegation).
- **Splitting the public board out of the admin bundle** — host separation already puts the board on a
  different *origin* from the admin console, which is all DEC-FBR-13 reason #1 asks for. The split is
  bundle hygiene: worth doing, not a prerequisite.
- **quiqpic and SessionHelm onboarding** — same recipe as § 4, once GitCellar has proven it.
