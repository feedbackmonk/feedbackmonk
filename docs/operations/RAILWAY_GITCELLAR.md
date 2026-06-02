# Deploying feedbackmonk on GitCellar's Railway (customer-#1, reuse-Postgres)

**Decision**: GitCellar self-hosts feedbackmonk on its **existing Railway**, **reusing GitCellar's
existing Postgres** (feedbackmonk gets its own database on that server; it is multi-tenant on a
single Postgres by design — DEC-FBR-03). Chosen for lowest incremental cost: one small always-on
API container, **no second database server**. The `feedbackmonk.com` SaaS standup is a separate,
later product decision and is NOT required for GitCellar's integration.

This runbook is the operator (GitCellar-side) procedure. It complements — does not replace —
`docs/operations/SELFHOST.md` (full self-host) and `docs/operations/SELFHOST_ENV.md` (Contract C21
env catalog). The only deviation from a vanilla self-host is **point `DATABASE_URL` at a new
database on the existing Railway Postgres instead of standing up a new `db` service.**

---

## 1. Create feedbackmonk's database on the existing Postgres

Against GitCellar's existing Railway Postgres (psql or Railway's DB console):

```sql
CREATE DATABASE feedbackmonk;
-- Optional dedicated role (or reuse the existing superuser for simplicity):
CREATE ROLE feedbackmonk WITH LOGIN PASSWORD '<strong-pass>';
GRANT ALL PRIVILEGES ON DATABASE feedbackmonk TO feedbackmonk;
```

feedbackmonk shares the server, not the data: every domain row is `tenant_id`+`project_id`-scoped,
and migration `00001` enables `pgcrypto` inside its own database. No collision with GitCellar tables.

The connection string becomes:
```
DATABASE_URL=postgres://feedbackmonk:<pass>@<railway-pg-host>:<port>/feedbackmonk
```
(Use Railway's private network host for the in-project DB; it appears in the Postgres service's
Connect tab.)

## 2. Add the feedbackmonk-api service

Deploy the API from this repo's existing image build — **no new Dockerfile needed**:

- **Source**: this repo; **Dockerfile**: `deploy/docker/Dockerfile.api` (multi-stage Rust build).
- Railway builds from the Dockerfile; set the build context to the repo root.

### Required env (🔒 = secret; see SELFHOST_ENV.md C21)
| Var | Value |
|---|---|
| `DATABASE_URL` 🔒 | the string from §1 |
| `FEEDBACKMONK_PUBLIC_URL` | `https://feedback.gitcellar.com` (no trailing slash) |
| `FEEDBACKMONK_SESSION_SECRET` 🔒 | `openssl rand -hex 32` (64 hex chars) |
| `FEEDBACKMONK_BIND_ADDR` | `0.0.0.0` |
| `FEEDBACKMONK_PORT` | Railway injects `PORT`; set `FEEDBACKMONK_PORT=${{PORT}}` (reference var) **or** fix `14304` and set Railway's exposed target port to match |
| `FEEDBACKMONK_MAILER` | `smtp` |
| `FEEDBACKMONK_SMTP_HOST/USER/PASS/FROM` 🔒 | reuse GitCellar's existing mail provider |
| `FEEDBACKMONK_LOG_FORMAT` | `json` |

All other vars take documented defaults.

### Run migrations against the new database
One-off, before/with first boot (Railway "deploy command" or a one-off shell):
```
DATABASE_URL=postgres://feedbackmonk:<pass>@<host>:<port>/feedbackmonk \
  sqlx migrate run --source migrations
```
or run the bundled runner inside the image: `deploy/docker/migrate.sh` (idempotent, forward-only).

### Verify
```
curl -fsS https://feedback.gitcellar.com/health/ready    # expect HTTP 200
```

## 3. (Optional) admin-ui triage console

feedbackmonk's admin-ui (React/nginx, `deploy/docker/Dockerfile.admin-ui`) is the triage dashboard.
It is **optional for the integration** (the widget + Desktop only need the API), but recommended so
GitCellar can triage feedback. Deploy as a second small Railway service; it reverse-proxies `/api/*`
to the API over the private network. If skipped, triage happens via API calls directly.

## 4. Widget hosting

Serve `widget/dist/widget.js` + `widget.css` (≈16.8 KB) as static assets. Cheapest options, in order:
1. From GitCellar's existing CDN/static host (it already serves gitcellar.com assets).
2. From the admin-ui nginx service (add a static location).
Point the embed's `src` at wherever you host it (see integration contract §4).

## 5. DNS

Point `feedback.gitcellar.com` at the Railway API (or admin-ui edge) service. This is the
`FEEDBACKMONK_PUBLIC_URL` and the integration contract's API base.

## 6. Provision the GitCellar tenant + project + signing key

Once `/health/ready` is 200, run `scripts/provision-gitcellar.sh` (see that file) — it walks the
signup → verify-email → create-project → register-signing-key flow and prints the **`project_id`**.
**Paste that `project_id` into `docs/integrations/gitcellar-adoption.md` §3.2 and flip the contract
to ACTIVE.**

---

## Cost note

Incremental cost ≈ one small always-on Rust container (+ optional tiny nginx). **No second Postgres**
— that's the saving versus a standalone feedbackmonk Railway project. Compute for an idle/low-traffic
Rust API is minimal; scales with feedback volume.
