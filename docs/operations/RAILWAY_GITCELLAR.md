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
| `FEEDBACKMONK_TRUSTED_PROXY_HOPS` | `1` — Railway fronts the app with a single load balancer. **Required**, or the per-IP public rate ceiling (scrutiny P0-2) keys off the LB address and collapses every client to one bucket. `1` trusts the rightmost `X-Forwarded-For` hop = the real client IP (scrutiny P1-2). |
| `FEEDBACKMONK_STORAGE_BACKEND` 🔒(keys) | **`s3`** for this deploy. Railway's container filesystem is **ephemeral** — the `local` backend would lose every attachment on each redeploy while its DB rows survive (dangling references, scrutiny P0-4). Set `s3` + the `FEEDBACKMONK_S3_*` vars (bucket/region/keys; see SELFHOST_ENV.md § Attachment Storage) pointing at a durable bucket (AWS S3, Cloudflare R2, Backblaze B2). Enable bucket **versioning + cross-region replication** for DR. |

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

## 7. Backup & DR

feedbackmonk is GitCellar's **sole, no-fallback** feedback backend, so its DR
posture is load-bearing (scrutiny P0-4). Both durable stores must be covered:

**Postgres (shared Railway instance):**

- **Confirm/enable Railway's managed Postgres backups / point-in-time recovery
  (PITR)** on the shared instance. Railway offers automated snapshots on paid
  Postgres plans — verify they are ON and note the retention window. This is
  the first line of defense for the shared DB.
- **Scheduled `pg_dump` offsite** as a second, portable line (Railway snapshots
  are provider-locked). Run a periodic logical dump of the `feedbackmonk`
  database to offsite storage (e.g. a Railway cron service or an external
  scheduler):
  ```bash
  pg_dump --no-owner --no-acl --clean --if-exists \
    "$DATABASE_URL" | gzip -9 > feedbackmonk-$(date -u +%Y%m%d).sql.gz
  # then copy offsite (S3/R2/B2). Keep 30 daily + 12 monthly (match policy).
  ```
  (The compose `backup.sh`/`restore.sh` scripts assume the self-host `db`
  service; on Railway you drive `pg_dump`/`psql` against `DATABASE_URL`
  directly, same flags.)

**Attachment objects (S3):**

- With `FEEDBACKMONK_STORAGE_BACKEND=s3` (required here — Railway's container FS
  is ephemeral), attachment durability is the **bucket's** responsibility, not
  a local dump. Enable bucket **versioning** (recover overwrite/delete) and
  **cross-region replication** (survive a region loss). Set lifecycle/expiry
  rules to match GitCellar's retention policy.
- ⚠️ Do NOT run this deploy on the `local` backend without a verified
  persistent volume — a redeploy would silently orphan every attachment
  reference in the DB.

**DR drill:** at least quarterly, restore the offsite `pg_dump` into a
throwaway database and fetch one attachment object URL end-to-end (the seam a
DB-only restore breaks). See `docs/operations/SELFHOST.md` § Disaster-recovery
(DR) drill for the step-by-step.

## 8. A6 redeploy runbook (ordered — Phase A capabilities cutover)

Bringing `feedback.gitcellar.com` up to ≥ v0.3.0 with the Phase-A contract
surfaces (delete / attachments / reply-state / severity / idempotency /
export). This is the A6 GATE that unblocks GitCellar Phases B/C. Run in order:

1. **Backup FIRST** (both halves — see §7). Take a fresh `pg_dump` of the
   `feedbackmonk` database offsite, and confirm the attachment bucket has
   versioning enabled. Never migrate an un-backed-up sole backend.
2. **Deploy the new image** (≥ v0.3.0) from this repo's `deploy/docker/Dockerfile.api`.
3. **Run migrations** — apply forward-only up to and including `00020`
   (severity) and `00021` (idempotency key). Either the Railway deploy command
   or a one-off shell:
   ```bash
   DATABASE_URL=postgres://feedbackmonk:<pass>@<host>:<port>/feedbackmonk \
     sqlx migrate run --source migrations
   # or run the bundled runner inside the image: deploy/docker/migrate.sh
   ```
   `sqlx` is idempotent (consults `_sqlx_migrations`); it applies only what is
   pending. If a migration fails, STOP and restore from the §7 backup —
   forward-only, no rollback.
4. **Verify the capability advertisement** — the API must advertise all six
   Phase-A capability strings:
   ```bash
   curl -fsS https://feedback.gitcellar.com/api/v1/capabilities | \
     tr ',' '\n' | grep -E 'feedback\.(delete|attachments|reply_state|severity|idempotency|export)'
   ```
   Expect all six: `feedback.delete`, `feedback.attachments`,
   `feedback.reply_state`, `feedback.severity`, `feedback.idempotency`,
   `feedback.export`. If any is missing, the deployed build predates Phase A —
   do NOT cut over.
5. **Smoke each new route** (delete a test item + verify its attachment bytes
   are purged; list attachments; `?since=` reply-state; submit with a
   `severity` + an `Idempotency-Key`; `GET …/me/feedback/export`). Confirm
   `FEEDBACKMONK_TRUSTED_PROXY_HOPS=1` is set (per-client rate limiting).
6. **Cut over** — flip GitCellar's integration to depend on the new
   capabilities only after steps 4–5 are green. This is what unblocks
   GitCellar Phases B/C.

---

## Cost note

Incremental cost ≈ one small always-on Rust container (+ optional tiny nginx). **No second Postgres**
— that's the saving versus a standalone feedbackmonk Railway project. Compute for an idle/low-traffic
Rust API is minimal; scales with feedback volume.
