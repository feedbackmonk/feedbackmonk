# Local Development Setup

Stage 1 P0 dev environment. Stage 2 onward will extend this with admin-UI
and widget instructions.

## Prerequisites

- Rust 1.80+ (`rustup install stable`)
- Docker (for the Postgres dev container)
- `sqlx-cli` v0.8+ (`cargo install sqlx-cli --no-default-features --features rustls,postgres`)

## Postgres dev container

feedbackmonk uses Postgres 17 in a local Docker container on port **5433**
(deliberately offset from the default `5432` to avoid clashing with peer
projects on this machine -- e.g. `gitcellar-cloud-postgres-1`).

### Start

```bash
docker run -d --name feedbackmonk-pg-dev \
  -p 5433:5432 \
  -e POSTGRES_PASSWORD=dev \
  -e POSTGRES_DB=feedbackmonk_dev \
  postgres:17-alpine
```

### Connection string

```
DATABASE_URL=postgres://postgres:dev@localhost:5433/feedbackmonk_dev
```

Persist this in a local `.env` (gitignored — see `.gitignore`):

```bash
echo 'DATABASE_URL=postgres://postgres:dev@localhost:5433/feedbackmonk_dev' > .env
```

### Apply schema

```bash
docker exec -i feedbackmonk-pg-dev \
  psql -U postgres -d feedbackmonk_dev \
  < migrations/00001_p0_schema.sql
```

Or via sqlx-cli once a project-level migration runner is wired in Stage 2.

### Tear down

```bash
docker rm -f feedbackmonk-pg-dev
```

## Building

```bash
# Online build (uses live DB for sqlx::query! macro type-checking):
DATABASE_URL=postgres://postgres:dev@localhost:5433/feedbackmonk_dev cargo build --workspace

# Offline build (uses .sqlx/ query cache; no DB required):
SQLX_OFFLINE=true cargo build --workspace
```

CI runs in offline mode (`SQLX_OFFLINE=true`). After modifying any
`sqlx::query!` invocation, regenerate the cache:

```bash
DATABASE_URL=postgres://postgres:dev@localhost:5433/feedbackmonk_dev \
  cargo sqlx prepare --workspace
git add .sqlx/
```

## Running tests

```bash
DATABASE_URL=postgres://postgres:dev@localhost:5433/feedbackmonk_dev \
  cargo test --workspace
```

`#[sqlx::test]` creates an isolated database per test (rolled back at the
end), so test runs never pollute `feedbackmonk_dev`. The pool created for each
test reuses the connection in a fresh per-test database.

## Verification Oracle

The `multi-tenant-isolation-check` oracle is the AST-grade leg of the
three-leg defense for FR-FBR-01 (the type system is leg 1, clippy +
cargo-deny is leg 3). Run it before every commit during P0+:

```powershell
# Windows
powershell -NoProfile -File .claude/oracles/multi-tenant-isolation-check/oracle.ps1
```

```bash
# Unix (CI uses this form)
bash .claude/oracles/multi-tenant-isolation-check/oracle.sh
```

PASS exits 0; FAIL exits 1 with file:line offenders. CI gates the build
on PASS.

## Backend dev port

`FEEDBACKMONK_PORT` env var; default **`14304`** (claimed in
`~/.claude/MACHINE_CONFIG.md` Dev Port Registry under the `14300-14399`
backend range).

```bash
DATABASE_URL=postgres://postgres:dev@localhost:5433/feedbackmonk_dev \
  cargo run -p feedbackmonk-api
```

Stage 1 ships a placeholder binary that binds the port and serves a static
banner. Stage 2 Workers A and B layer the real router tree on top.
