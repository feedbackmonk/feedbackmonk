# feedbackmonk Self-Host Runbook

> One-page operator guide for running feedbackmonk on your own hardware via
> `docker compose up`. Closes FR-FBR-17.
>
> Companion artifacts:
> - **`docs/operations/SELFHOST_ENV.md`** — Contract C21, the canonical
>   env-var catalog with code line refs.
> - **`deploy/docker/`** — the compose stack itself + Dockerfiles + scripts.
> - **`.claude/oracles/selfhost-compose-smoke/`** — Verification Oracle that
>   defends the contract between this runbook, the compose stack, and C21.

---

## 1. Prerequisites

| Requirement | Version | Why |
|---|---|---|
| Docker Engine | 24+ (with Compose v2) | docker-compose v1 syntax is unsupported; profiles + `condition:` `service_completed_successfully` require Compose v2.20+. |
| RAM | 1 GB minimum, 2 GB recommended | postgres + Rust api + nginx + (briefly) a Rust build during first `docker compose up --build`. |
| Disk | 5 GB | ~80 MB for the api image, ~25 MB for nginx, ~250 MB for postgres data per ~10k feedback items. Add headroom for backups. |
| Open ports | 1 (default 14304) | Operator-facing admin-ui edge. Postgres + api are not exposed to the host by default. |
| Optional: TLS proxy | Caddy / nginx / Cloudflare Tunnel | This stack speaks HTTP only. Production deployments terminate TLS upstream. |

Verify Docker:

```bash
docker --version
docker compose version
```

Both commands should return; if `docker compose` errors, your Docker is too
old.

## 2. Quickstart

```bash
# 1. Clone the repo (replace org name with whatever your fork is at):
git clone https://github.com/feedbackmonk/feedbackmonk.git
cd feedbackmonk/deploy/docker

# 2. Copy the env template and edit the four required values:
cp .env.example .env
${EDITOR:-vim} .env
#   DATABASE_URL=postgres://feedbackmonk:<your-password>@db:5432/feedbackmonk
#   FEEDBACKMONK_PUBLIC_URL=https://feedback.example.com   (or http://localhost:14304 for local)
#   FEEDBACKMONK_SESSION_SECRET=<output of `openssl rand -hex 32`>
#   POSTGRES_PASSWORD=<same password as in DATABASE_URL>

# 3. Bring up the stack (first run builds the Rust + node images — 5-15 min cold):
docker compose up -d

# 4. Verify health:
curl http://localhost:14304/health/ready
# Expected: HTTP 200 with body like:
# {"status":"ok","db_connected":true,"version":"0.1.0","uptime_seconds":7,"started_at":"2026-05-14T..."}
```

If `/health/ready` returns 200 with `db_connected: true`, your instance
is live. Visit `http://localhost:14304/` in a browser — you should see
the admin-ui sign-in screen.

### Generate the session secret

```bash
openssl rand -hex 32
# 64 hex chars; copy the output into FEEDBACKMONK_SESSION_SECRET=...
```

If you don't have `openssl`, alternatives:
- `python -c 'import secrets; print(secrets.token_hex(32))'`
- `head -c 32 /dev/urandom | xxd -p -c 64`

**Rotating** `FEEDBACKMONK_SESSION_SECRET` invalidates all admin
sessions (admins must sign in again). Treat it like a long-lived
secret.

## 3. Environment Variables

The full canonical catalog with code line refs and security flags lives
at [`docs/operations/SELFHOST_ENV.md`](./SELFHOST_ENV.md) (Contract C21).
Short reference of the **required** vars:

| Var | Required | Notes |
|---|---|---|
| `DATABASE_URL` | ✅ | `postgres://USER:PASS@db:5432/feedbackmonk`. The host `db` is the docker-compose service name; do not change it. |
| `FEEDBACKMONK_PUBLIC_URL` | ✅ | Customer-facing base URL. No trailing slash. Must match the URL operators (and verify-email recipients) actually visit. |
| `FEEDBACKMONK_SESSION_SECRET` | ✅ | 64 hex chars (32 bytes). Generate with `openssl rand -hex 32`. |
| `POSTGRES_PASSWORD` | ✅ | Used by the `db` container's init protocol AND must appear in `DATABASE_URL` (keep them in sync). |
| `POSTGRES_USER` | optional | Default `feedbackmonk`. Override if you set a different user. |
| `POSTGRES_DB` | optional | Default `feedbackmonk`. |

The `.env.example` ships sensible defaults for the remaining ~16
optional vars (logging format, mailer selection, token TTLs, anon
rate-limit, SMTP). Uncomment lines in `.env` to override.

### Switching to real email (SMTP)

By default `FEEDBACKMONK_MAILER=mailpit` (works only with the
`--profile dev` mailpit container; for self-host production, switch to
SMTP):

```bash
# In .env:
FEEDBACKMONK_MAILER=smtp
FEEDBACKMONK_SMTP_HOST=smtp.your-provider.com
FEEDBACKMONK_SMTP_PORT=587
FEEDBACKMONK_SMTP_USER=feedbackmonk@example.com
FEEDBACKMONK_SMTP_PASS=<your-smtp-password>
FEEDBACKMONK_SMTP_FROM=no-reply@example.com
FEEDBACKMONK_SMTP_STARTTLS=true     # default; set false only for legacy servers
```

Then restart: `docker compose up -d` (compose picks up env changes and
recreates the api container).

## 4. Backup and Restore

### Daily backup (gzipped pg_dump → file)

```bash
cd deploy/docker
mkdir -p backups
./backup.sh > backups/feedbackmonk-$(date +%Y%m%d-%H%M%S).sql.gz
```

This invokes `docker compose --profile backup run --rm backup` under
the hood, which spawns a one-shot `postgres:16-alpine` container that
runs `pg_dump --clean --if-exists | gzip -9` and pipes the result
to stdout.

### Scheduled backup (cron)

```cron
# /etc/cron.d/feedbackmonk-backup — daily at 03:15 UTC
15 3 * * *  root  cd /opt/feedbackmonk/deploy/docker && ./backup.sh > /var/backups/feedbackmonk/$(date -u +\%Y\%m\%d).sql.gz 2>>/var/log/feedbackmonk-backup.log
```

### Scheduled backup (systemd-timer)

`/etc/systemd/system/feedbackmonk-backup.service`:

```ini
[Unit]
Description=feedbackmonk daily db backup
After=docker.service

[Service]
Type=oneshot
WorkingDirectory=/opt/feedbackmonk/deploy/docker
ExecStart=/bin/sh -c './backup.sh > /var/backups/feedbackmonk/$(date -u +%Y%m%d).sql.gz'
```

`/etc/systemd/system/feedbackmonk-backup.timer`:

```ini
[Unit]
Description=Daily feedbackmonk backup

[Timer]
OnCalendar=daily
Persistent=true

[Install]
WantedBy=timers.target
```

Then:

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now feedbackmonk-backup.timer
```

### Restore (destructive)

The dump is `--clean --if-exists` — restoring will **DROP and recreate**
every object in the live database. The `restore.sh` script requires an
interactive confirmation OR `--force` for non-TTY invocation:

```bash
cd deploy/docker
gunzip -c backups/feedbackmonk-20260514.sql.gz | ./restore.sh
# (interactive): type 'yes' at the confirmation prompt.

# Non-interactive (e.g., DR drill from a script):
gunzip -c backups/feedbackmonk-20260514.sql.gz | ./restore.sh --force
```

Restore runs inside the live `db` service container via
`docker compose exec -T db psql --single-transaction`. The
`--single-transaction` flag means the entire restore is atomic — on
any failure, the database is left in its pre-restore state.

### Volume-level alternative

If you prefer file-level backups of the postgres data volume:

```bash
docker run --rm \
    -v feedbackmonk_pgdata:/data \
    -v "$PWD/backups":/backup \
    alpine:latest \
    tar czf /backup/pgdata-$(date +%Y%m%d).tar.gz -C /data .
```

Volume snapshots are faster to take but require stopping the db service
for a consistent snapshot (or accept a fuzzy snapshot). `pg_dump` is
the recommended path for online backups.

## 5. Troubleshooting

### `docker compose up` fails immediately

```bash
docker compose logs api
docker compose logs migrate
docker compose logs db
```

Common causes:

| Symptom | Likely cause | Fix |
|---|---|---|
| `DATABASE_URL is required (set in .env)` | `.env` not present or DATABASE_URL not set | `cp .env.example .env` and edit. |
| `FEEDBACKMONK_SESSION_SECRET is required` | secret not set or wrong length | Run `openssl rand -hex 32` and paste the output. |
| `password authentication failed for user "feedbackmonk"` in db logs | `POSTGRES_PASSWORD` not in sync with the password inside `DATABASE_URL` | Edit `.env` to match. Then: `docker compose down -v && docker compose up -d` (note: `-v` removes the data volume — destructive). |
| `port is already allocated` | something else is on FEEDBACKMONK_PORT (default 14304) | Set `FEEDBACKMONK_PORT=...` in `.env` to a free port. |
| `migrate` container exits 1 with `relation already exists` | rare; corrupt `_sqlx_migrations` table | Manual fix: `docker compose exec db psql -U feedbackmonk -d feedbackmonk -c 'TRUNCATE _sqlx_migrations'` (DESTRUCTIVE — only if you know what you're doing), or restore from backup. |

### `/health/ready` returns 503

Means the api booted but cannot reach the db. Try in order:

1. `docker compose ps` — is `db` `Up (healthy)`? If not, `docker compose logs db`.
2. `docker compose exec db pg_isready -U feedbackmonk` — does the db respond? If not, the volume may be corrupt; consider `down -v` (destructive) and restore from backup.
3. Wait 10–20s — first boot of postgres in a fresh volume runs initdb (creates the db, applies POSTGRES_* env vars, sets up the user); takes longer than steady-state restarts.
4. `docker compose logs api` — look for `db ping failed` warnings (this is what the api emits when health.rs `ping_db` returns false).

### Verify-email links 404

The api emits links using `FEEDBACKMONK_PUBLIC_URL` verbatim. Common
mistakes:

- Trailing slash (`https://feedback.example.com/`) — strip it.
- Wrong scheme (`http://` when the actual visit is `https://`) — fix it.
- Wrong port (`http://localhost:14304` when behind a reverse proxy on `:443`) — set to the public URL the user actually visits.

After changing `FEEDBACKMONK_PUBLIC_URL`, `docker compose up -d` to
restart the api container.

### SMTP failures

```bash
docker compose logs api | grep -i smtp
```

Common causes:

- `connection refused` — wrong `FEEDBACKMONK_SMTP_HOST` / `_PORT`.
- `authentication failed` — wrong `FEEDBACKMONK_SMTP_USER` / `_PASS`.
- `STARTTLS not offered by server` — set `FEEDBACKMONK_SMTP_STARTTLS=false` for legacy servers (rare; modern SMTP is always STARTTLS).
- Provider-specific block (e.g., Gmail) — use an app password, not your account password. Or switch to a transactional-email provider (Postmark, Resend, Mailgun, etc.).

### Container restart loops

```bash
docker compose ps
# If a service is "Restarting (N) X seconds ago", inspect its last exit:
docker compose logs --tail 50 <service-name>
```

`restart: unless-stopped` is set for db, api, and admin-ui. The
`migrate` service is `restart: "no"` (it's an init container — runs
once, exits 0). If migrate keeps restarting, that's a bug; if it
exited 0 once, it stays exited until the next `up`.

### Reset everything (DESTRUCTIVE)

```bash
docker compose down -v          # removes containers + the pgdata volume — DATA LOSS
docker compose up -d            # fresh first-boot
```

Use only if you have a current backup or are intentionally
starting over.

## 6. Upgrade Procedure

```bash
cd /opt/feedbackmonk           # wherever you cloned
git pull
cd deploy/docker
./backup.sh > backups/pre-upgrade-$(date +%Y%m%d-%H%M%S).sql.gz    # always backup first
docker compose pull            # if using published images
docker compose build           # if building locally
docker compose up -d           # `up` recreates only changed services
```

### Migration behavior on upgrade

Every `docker compose up` runs the `migrate` init-container before
the api starts. `sqlx migrate run` is **idempotent**: it consults the
`_sqlx_migrations` table inside your database and applies only
migrations not already recorded there. Skipping or rolling back
migrations is **not supported** — restore from backup if a migration
fails.

### Version skew

The api image's bundled `migrations/*.sql` MUST be newer than or equal
to the version of feedbackmonk that wrote the data. Downgrading to an
older image while keeping a newer database is not supported (sqlx
will silently no-op pending migrations, but the api may rely on
columns or tables added in newer migrations and crash).

### Major-version upgrades

For major-version bumps (currently theoretical — feedbackmonk is at
v1), follow these steps in order:

1. **Read** the `CHANGELOG.md` or release notes for the target version.
2. **Backup** explicitly before upgrade (`./backup.sh > ...`).
3. **Test** the upgrade on a non-production instance with a copy of
   production data, if you can.
4. **Upgrade**: `git pull && docker compose build && docker compose up -d`.

## 7. Where to get help

- **Bug reports**: GitHub Issues at `https://github.com/feedbackmonk/feedbackmonk/issues` (org currently pending registration; see project CLAUDE.md PF-REGISTER-01).
- **Security issues**: security@feedbackmonk.com once the domain is registered (placeholder; do not publicly disclose vulnerabilities until then).
- **License**: AGPL-3.0-or-later. Source code is in the same repo you cloned. Modifications must be made available under AGPL.
- **Contracts**: this runbook is the cold-readable surface for FR-FBR-17. The Verification Oracle `selfhost-compose-smoke` (`.claude/oracles/selfhost-compose-smoke/`) defends the contract between this doc, `docs/operations/SELFHOST_ENV.md` (C21), and `deploy/docker/docker-compose.yml`.

---

*Last reviewed: 2026-05-14 (P4 Stage 2). Generated by feedbackmonk
P4 Stage 2 Worker B in collab-20260514-170323. Frozen at this
revision as part of the Stage 2 → Stage 3 freeze ping for Worker A's
`/docs/self-host` page.*
