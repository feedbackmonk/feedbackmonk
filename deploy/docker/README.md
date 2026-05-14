# deploy/docker/ — feedbackmonk Self-Host Docker Distribution

> **Stage**: P4 Stage 1 SKELETON. Scaffolded with a README only; the Docker stack itself (docker-compose.yml, Dockerfiles, migration runner, backup scripts) is authored by **P4 Stage 2 Worker B** per the plan at `docs/planning/plans/20260514T163356-feedbackmonk-p4-go-public.md`.

## Purpose & Responsibilities

The one-line `docker compose up` self-host distribution for feedbackmonk (FR-FBR-17). Worker B authors:

- `docker-compose.yml` — the canonical multi-service compose file
- `Dockerfile.api` — multi-stage Rust build for `feedbackmonk-api`
- `Dockerfile.admin-ui` (or admin-ui served from api binary) — TBD by Worker B
- `migrate.sh` — sqlx migration runner invoked on container startup
- `backup.sh` / `restore.sh` — pg_dump-based backup runbook
- `.env.example` — operator-facing template of the canonical env-var schema from `docs/operations/SELFHOST_ENV.md`

End goal: an operator clones the repo, copies `.env.example` → `.env`, fills three required values (`DATABASE_URL`, `FEEDBACKMONK_PUBLIC_URL`, `FEEDBACKMONK_SESSION_SECRET`), runs `docker compose up -d`, and has a working feedbackmonk instance on the configured port.

## File Index

(Stage 1 SKELETON: only this README. Stage 2 Worker B populates the full stack.)

| File | Purpose |
|---|---|
| `README.md` | This file. |

## Public API & Usage

**Quickstart** (Stage 2 onward):

```bash
cd deploy/docker
cp .env.example .env
# Edit .env: set DATABASE_URL, FEEDBACKMONK_PUBLIC_URL, FEEDBACKMONK_SESSION_SECRET
docker compose up -d
curl http://localhost:14304/health    # → {"status":"ok",...}
```

**Backup**: `./backup.sh > backup-$(date +%Y%m%d).sql.gz`
**Restore**: `gunzip -c backup-YYYYMMDD.sql.gz | ./restore.sh`

(Concrete commands replaced by Worker B with real scripts at Stage 2.)

## Constraints & Business Rules

- **Env-var schema**: `docker-compose.yml` `environment:` blocks MUST reference only names documented in `docs/operations/SELFHOST_ENV.md` (Contract C21). Drift is caught by `selfhost-compose-smoke` Verification Oracle Probe B. Adding a new var = update SELFHOST_ENV.md FIRST, then compose.
- **No bundled secrets**: `.env.example` ships placeholder values for secrets (`FEEDBACKMONK_SESSION_SECRET=replace-with-...`); real secrets are operator-set and gitignored.
- **Single-image-per-service**: don't build the api and admin-ui as one monolithic image unless serving-from-api-binary is chosen (Stage 2 Worker B decides; document the choice in this README).
- **Default ports**: api on `14304` (host) → `14304` (container); postgres on `5433` (host, deconflicted from gitcellar-cloud's 5432 per DEC-FBR-IMPL-04) → `5432` (container, standard). Operators override as needed via `.env`.
- **Migration safety**: `migrate.sh` runs sqlx migrations on every `up`; sqlx migrations are idempotent and forward-only. Do NOT add destructive-migration logic here.
- **Mailpit dev profile**: docker-compose ships a `mailpit` service in a Compose profile (`docker compose --profile dev up`); production self-host operators select `FEEDBACKMONK_MAILER=smtp` and skip the dev profile.

## Relationships & Dependencies

- **Env-var catalog consumer**: `docker-compose.yml` references `docs/operations/SELFHOST_ENV.md` (Contract C21). Probe B of the smoke oracle enforces this.
- **API binary consumer**: Dockerfile.api builds `crates/feedbackmonk-api/` from the workspace root.
- **Admin-UI artifact consumer** (if separate from api binary): bundles `admin-ui/dist/` after Vite build.
- **Migration consumer**: `migrate.sh` runs `sqlx migrate run` against `DATABASE_URL`, reading `migrations/*.sql` from the workspace root.
- **Smoke oracle consumer**: `.claude/oracles/selfhost-compose-smoke/` (built by Worker B Task Zero) tests this directory's contents per DEC-FBR-IMPL-06.

## Decision Log

- **2026-05-14** — `deploy/docker/` is the chosen top-level location (vs root-level `docker-compose.yml`, vs `docker/`). Rationale: explicit, scoped, leaves room for future `deploy/helm/` or `deploy/k8s/` siblings without polluting root. The arc-plan defers Helm/K8s to v1.1+; this directory structure accommodates them when they land.
- **2026-05-14** — `selfhost-compose-smoke` oracle is Worker B Task Zero. Rationale: Testability Gate scored FR-FBR-17 at composite ~14; oracle is the high-leverage scaffolding. Per DEC-FBR-IMPL-06.
- **2026-05-14** — Operator-facing env-var template lives at `deploy/docker/.env.example`, distinct from the workspace-root `.env.example` (which is developer-machine-shaped). Rationale: clearer mental model — root `.env.example` is for `cargo run` dev workflow; `deploy/docker/.env.example` is for `docker compose up` self-host workflow. Both reference the same canonical schema (`docs/operations/SELFHOST_ENV.md`); they differ in defaults.
