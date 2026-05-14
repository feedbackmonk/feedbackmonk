# deploy/docker/ — feedbackmonk Self-Host Docker Distribution

> **Agent-context**: this directory ships the one-line `docker compose up`
> self-host distribution for feedbackmonk (FR-FBR-17). The
> `selfhost-compose-smoke` Verification Oracle
> (`.claude/oracles/selfhost-compose-smoke/`) defends the env-var contract
> with C21 (`docs/operations/SELFHOST_ENV.md`) and the clean-state smoke
> against `/health/ready`. Operator-facing runbook lives at
> `docs/operations/SELFHOST.md`.

## Synopsis

`docker compose up -d` self-host stack — postgres + sqlx migrate init-container + Rust api binary + nginx admin-ui edge (reverse-proxies `/api/*` and `/health*`). Operators set 3 required env vars (`DATABASE_URL`, `FEEDBACKMONK_PUBLIC_URL`, `FEEDBACKMONK_SESSION_SECRET`); everything else has a sensible default.

## Purpose & Responsibilities

Ship the AGPL-3.0 self-host distribution for feedbackmonk per FR-FBR-17.
Greenfield directory; no pattern port from any peer repo.

End goal: an operator clones the repo, copies
`deploy/docker/.env.example` → `deploy/docker/.env`, fills three required
values, runs `docker compose up -d`, and has a working feedbackmonk
instance at the configured port returning HTTP 200 at `/health/ready`.

## File Index

| File | Purpose |
|---|---|
| `README.md` | This file. ULADP module README — agent-orientation surface. |
| `docker-compose.yml` | Canonical multi-service compose stack (db + migrate + api + admin-ui + optional mailpit + optional backup). |
| `Dockerfile.api` | Multi-stage Rust build (cargo-chef → release → slim-debian runtime). Image carries the `feedbackmonk-api` binary, `sqlx` CLI, migrations dir, curl, tini. |
| `Dockerfile.admin-ui` | Multi-stage node→nginx build. Stage 1 builds `admin-ui/dist/`; stage 2 serves it via nginx with reverse-proxy to api. |
| `admin-ui-nginx.conf` | nginx site config: SPA fallback for `/`, reverse-proxy for `/api/*` + `/health` + `/health/ready` → `api:14304`. |
| `migrate.sh` | Init-container entrypoint. Runs `sqlx migrate run --source /app/migrations`. Idempotent (sqlx tracks applied migrations). |
| `backup.sh` | Operator-side script — `docker compose --profile backup run --rm backup` piping gzipped pg_dump to stdout. |
| `restore.sh` | Operator-side script — reads gzipped sql from stdin, pipes into `docker compose exec db psql`. Requires `--force` for non-TTY invocation (destructive). |
| `.env.example` | Operator-facing env template. Distinct from workspace-root `.env.example` (which is developer-machine-shaped). |

(The workspace-root `.dockerignore` excludes node_modules, target/,
test artifacts, and tooling state from the build context for all Docker
builds in this project.)

## Public API & Usage

**Quickstart** (operator):

```bash
cd deploy/docker
cp .env.example .env
# Edit .env — required: DATABASE_URL, FEEDBACKMONK_PUBLIC_URL, FEEDBACKMONK_SESSION_SECRET, POSTGRES_PASSWORD
docker compose up -d
curl http://localhost:14304/health/ready   # expect HTTP 200 + {"status":"ok",...}
```

**Backup** (operator):

```bash
./backup.sh > backups/feedbackmonk-$(date +%Y%m%d).sql.gz
```

**Restore** (operator, destructive):

```bash
gunzip -c backups/feedbackmonk-20260514.sql.gz | ./restore.sh --force
```

**Mailpit dev profile** (catches outgoing email at http://localhost:8025):

```bash
docker compose --profile dev up -d
```

**One-off migration run** (against existing db):

```bash
docker compose run --rm migrate
```

For the cold-readable operator path (prerequisites, troubleshooting,
upgrade procedure, env-var reference), see
`docs/operations/SELFHOST.md`.

## Constraints & Business Rules

- **C21 is the SSOT for env-var names**. `docker-compose.yml`'s
  `environment:` sections reference only names documented in
  `docs/operations/SELFHOST_ENV.md`. The `selfhost-compose-smoke`
  oracle's Probe B enforces this at every commit. Adding a new var
  requires APPENDing to SELFHOST_ENV.md FIRST (pre-authorized
  self-mediated widening per GUIDE §8), THEN referencing in compose —
  never silence the oracle.
- **POSTGRES_USER/PASSWORD/DB are container-init protocol**, not
  C21 application surface. They configure the official `postgres:16`
  image's first-boot initialization. Operators set them in `.env` and
  keep them in sync with the `DATABASE_URL` user/password/dbname.
- **Migrations are forward-only**. `migrate.sh` runs `sqlx migrate run`
  on every `up`; sqlx tracks applied migrations in `_sqlx_migrations`
  and skips already-applied ones. There is no rollback. To roll back a
  migration: restore from backup.
- **No bundled secrets**. `.env.example` ships placeholder values
  (`replace-with-...`, `CHANGE-ME-IN-PRODUCTION`); real secrets are
  operator-set and `.env` is gitignored.
- **db port not exposed to host by default**. The api accesses
  postgres via the internal docker network (`db:5432`). Operators
  wanting direct psql access add `ports:` to the `db` service
  themselves.
- **api port not exposed to host by default**. The admin-ui nginx
  edge fronts it (port 80 in the nginx container, mapped to the
  operator-chosen host port via `${FEEDBACKMONK_PORT:-14304}:80`).
  Operators wanting headless self-host (no admin-ui) can expose api
  directly by adding `ports:` to the `api` service.
- **TLS termination is operator's responsibility**. This stack speaks
  HTTP only. Production self-host deployments put Caddy, nginx,
  Traefik, or Cloudflare Tunnel in front to terminate TLS. Set
  `FEEDBACKMONK_PUBLIC_URL=https://your.domain` so verify-email
  links use the TLS endpoint.
- **`FEEDBACKMONK_SESSION_SECRET` must be 64 hex chars** (32 bytes
  from `openssl rand -hex 32`). The api binary validates length at
  startup and refuses to boot otherwise.
- **AGPL-3.0**: this stack ships an AGPL-3.0 release; self-host
  operators receive identical functionality to SaaS. Modifications
  to feedbackmonk are subject to AGPL.

## Relationships & Dependencies

- **C21 env-var catalog**: `docker-compose.yml` references only names
  documented in `docs/operations/SELFHOST_ENV.md`. Probe B enforces.
- **api binary source**: `Dockerfile.api` builds
  `crates/feedbackmonk-api/` from the workspace root. The image
  bundles all 8 migrations (`migrations/00001..00008_*.sql`).
- **admin-ui static**: `Dockerfile.admin-ui` builds `admin-ui/` with
  `npm run build` and serves the resulting `dist/` via nginx.
- **Verification Oracle**: `.claude/oracles/selfhost-compose-smoke/`
  defends this directory (yaml-lint + env-doc-xref + `--full`
  clean-state smoke against `/health/ready`).
- **Operator runbook**: `docs/operations/SELFHOST.md` is the
  cold-readable operator path.
- **`/health/ready` shape**: defined in
  `crates/feedbackmonk-api/src/handlers/health.rs:57`. Probe C polls
  this endpoint and asserts `body.status == "ok"` AND
  `body.db_connected == true`.

## Decision Log

- **2026-05-14 — Topology B2 (separate nginx edge) over B1**
  (api binary serves admin-ui static). Rationale: B1 requires
  modifying `crates/feedbackmonk-api/src/router.rs` to add
  `tower-http::services::ServeDir` for the admin-ui assets. router.rs
  is P0-P3 surface; P4 is net-additive only. Task instructions
  explicitly say "If [static-asset-serving] not [wired], document the
  gap and flag at convergence." B2 (separate nginx container that
  reverse-proxies to api) is purely additive to `deploy/docker/`,
  doesn't touch any frozen crate code, and gives operators a clean
  single-port self-host story (admin-ui + api co-located behind one
  nginx). Future B2→B1 migration is trivial when api gains ServeDir
  (~5 LOC + COPY in Dockerfile + remove admin-ui container).
- **2026-05-14 — Dockerfile.api base = `debian:bookworm-slim`** over
  distroless. Rationale: distroless (gcr.io/distroless/cc-debian12)
  is smaller (~25MB vs ~80MB) but has no shell, no curl, no apt.
  slim-debian gives operators ergonomic `docker exec` for
  troubleshooting AND lets the compose `healthcheck` use `curl` AND
  lets `migrate.sh` (a bash script) run in the same image. The size
  penalty (~55MB) is acceptable for a self-host distribution where
  operability beats minimal-footprint.
- **2026-05-14 — Single image (Dockerfile.api) for both api and
  migrate services**. Rationale: both need the migrations dir and the
  sqlx-cli binary. Building a separate migrate image would duplicate
  the cargo-chef cook stage. Compose simply picks the command per
  service (api → feedbackmonk-api binary; migrate → migrate.sh).
- **2026-05-14 — cargo-chef pattern for dependency caching**.
  Rationale: without it, every source change recompiles all
  dependencies (~5-10 min). With it, only the source-touch layer
  rebuilds. Standard Rust-in-Docker pattern.
- **2026-05-14 — sqlx-cli pinned to `^0.8`**. Rationale: matches the
  workspace `sqlx = "0.8"` dep so the migration runner uses
  compatible serialization of `_sqlx_migrations`.
- **2026-05-14 — `SQLX_OFFLINE=true` at build time + `.sqlx/`
  bundled in the build context**. Rationale: lets `cargo build` use
  the offline cache for compile-time-checked queries without
  requiring a live db at image-build time. The `.sqlx/` directory is
  committed to git and regenerated via `cargo sqlx prepare` when
  queries change.
- **2026-05-14 — Mailpit in `dev` profile, not default**. Rationale:
  production self-host operators use real SMTP via
  `FEEDBACKMONK_MAILER=smtp`. Including mailpit by default would
  spawn an unused container and surface an 8025 port that has no
  business being there in prod. Operators run
  `docker compose --profile dev up` for local testing with email
  capture.
- **2026-05-14 — backup in opt-in `backup` profile**. Rationale: it
  is a one-shot job, not a long-running service. Profile keeps it
  out of the default `compose up` set; operators invoke explicitly
  via `--profile backup run`.
- **2026-05-14 — Restore script requires `--force` for non-TTY
  invocation**. Rationale: `pg_dump --clean --if-exists` is destructive
  (drops + recreates every object). Without `--force`, an operator
  who pipes the dump in via cron without `--force` would silently
  drop their live database. Explicit acknowledgement reduces
  foot-gun risk.
- **2026-05-14 — `.dockerignore` at workspace root, not in
  `deploy/docker/`**. Rationale: build context is `../..` (the
  workspace root). Docker reads `.dockerignore` from the context
  root, so the file lives there. Co-located alternative
  (`Dockerfile.api.dockerignore` per BuildKit 1.6+) was rejected for
  shareable simplicity across both Dockerfiles.
- **2026-05-14 — nginx admin-ui-nginx.conf has 14304 hard-coded in
  the proxy_pass for the api service**. Rationale: parameterizing
  via envsubst at container-start would add a stamp-template stage
  to Dockerfile.admin-ui. For v1 self-host the C21 default
  (FEEDBACKMONK_PORT=14304) is the path of least friction; operators
  who change it rebuild the admin-ui image. Documented in the
  nginx config inline.
- **2026-05-14 — admin-ui healthcheck uses `wget` (busybox in
  nginx:alpine)** while api healthcheck uses `curl` (installed in
  slim-debian). Both produce the same outcome; choosing the
  natively-present tool avoids gratuitous package installs.

## Frozen / load-bearing invariants

| Invariant | Where | Why |
|---|---|---|
| compose `environment:` refs ⊆ C21 catalog (Probe B) | docker-compose.yml + docs/operations/SELFHOST_ENV.md | DEC-FBR-IMPL-06 three-probe oracle + Contract C21 SSOT |
| compose YAML parseable + service refs resolve (Probe A) | docker-compose.yml | DEC-FBR-IMPL-06 |
| clean-state up → `/health/ready` 200 in <90s (Probe C `--full`) | full stack | DEC-FBR-IMPL-06 + FR-FBR-17 |
| `FEEDBACKMONK_SESSION_SECRET` length validation | api binary startup | crates/feedbackmonk-api/src/main.rs:268 |
| migrations forward-only + idempotent | migrate.sh + sqlx | `_sqlx_migrations` table semantics |
| `_health/ready` body shape (`status`, `db_connected`) | api binary | `crates/feedbackmonk-api/src/handlers/health.rs:57` |

## See also

- `docs/operations/SELFHOST.md` — operator runbook (cold-readable)
- `docs/operations/SELFHOST_ENV.md` — Contract C21 env-var catalog
- `.claude/oracles/selfhost-compose-smoke/` — Verification Oracle
- `docs/planning/plans/20260514T163356-feedbackmonk-p4-go-public.md` — P4 plan §Stage 2 Worker B
- `docs/specs/DECISIONS.md` — DEC-FBR-IMPL-06 (three-probe smoke oracle)
- `migrations/README.md` — migration authoring conventions
