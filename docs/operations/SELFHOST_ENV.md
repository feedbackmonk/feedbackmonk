# feedbackmonk — Self-Host Environment Variable Schema

**Status**: FROZEN for P4 Stage 2. Authoritative source for `docker-compose.yml` `environment:` declarations, `.env.example` content, and the marketing site's `/docs/self-host` page. Contract **C21**.
**Frozen at**: P4 Stage 1, 2026-05-14.
**Audit source**: grepped `crates/feedbackmonk-*/` for `std::env::var`, `env!`, and `FEEDBACKMONK_*` references; cross-referenced `.env.example` (P0 baseline); confirmed against `crates/feedbackmonk-api/src/main.rs` runtime read sites.

> **Identity**: this file is the **one-and-only catalog** of env vars that a self-host operator needs to set. Worker A's `/docs/self-host` page is generated from / references this; Worker B's `docker-compose.yml` env section enumerates exactly these names. The `selfhost-compose-smoke` Verification Oracle's Probe B cross-references compose-env against this catalog and fails on drift.

---

## Conventions

- **Prefix**: every project-owned env var begins with `FEEDBACKMONK_` (set during PF-RENAME-01).
- **Required-by-default**: a var with no default value must be set to start the binary; the api `main.rs` calls `.context("…")` on `env::var()` for required vars, producing a startup error with the name when missing.
- **Optional**: vars with `unwrap_or_else(default)` or `.unwrap_or(default)` are optional; if omitted, the documented default applies.
- **Security-sensitive flag** (🔒): vars carrying secrets, signing keys, or credentials. Operators MUST NOT commit these to public repos; docker-compose users should source from `.env` (gitignored) or a secrets manager (Vault, AWS Secrets Manager, K8s secret).
- **Self-host vs SaaS**: the SAME vars work in both deployments. SaaS additionally consumes some-not-yet-introduced vars (e.g., Polar webhook secret) when Polar billing un-defers per DEC-FBR-DEFER-01 — those will be added here as that work lands.

---

## Canonical Env-Var Catalog

### Database

| Name | Required | Default | 🔒 | Semantics |
|---|---|---|---|---|
| `DATABASE_URL` | **REQ** | — | 🔒 | PostgreSQL connection string. Dev: `postgres://postgres:dev@localhost:5433/feedbackmonk_dev`. Self-host: usually `postgres://feedbackmonk:<pass>@db:5432/feedbackmonk` (intra-compose service name `db`). Connection-string format: `postgres://USER:PASS@HOST:PORT/DBNAME`. Source: `crates/feedbackmonk-api/src/main.rs:115`. |

### HTTP Binding

| Name | Required | Default | 🔒 | Semantics |
|---|---|---|---|---|
| `FEEDBACKMONK_PORT` | optional | `14304` | | TCP port the api binary binds to. Integer 1-65535. In docker-compose this typically stays at `14304` and the container port is mapped externally (`ports: ["14304:14304"]`) or proxied behind nginx. Source: `crates/feedbackmonk-api/src/main.rs:51`. |
| `FEEDBACKMONK_BIND_ADDR` | optional | `127.0.0.1` | | IP address the api binary binds to. Default `127.0.0.1` preserves the dev-machine pattern (don't expose api to LAN during `cargo run`). Docker-compose self-host sets this to `0.0.0.0` so the admin-ui edge container can reach api via docker-network DNS. Source: `crates/feedbackmonk-api/src/main.rs:59`. **Note**: appended P4 Stage 2 by Worker B (self-mediated widening per GUIDE §8, ratification pending at convergence — needed to unblock B2 topology where admin-ui nginx must reach api over the docker bridge network). |
| `FEEDBACKMONK_PUBLIC_URL` | **REQ** | — | | Customer-facing base URL used in verify-email links and any URL the customer follows back to the api. **No trailing slash.** Dev: `http://localhost:14304`. Self-host behind TLS: `https://feedback.example.com`. Source: `crates/feedbackmonk-api/src/main.rs:141`. |

### Logging / Observability

| Name | Required | Default | 🔒 | Semantics |
|---|---|---|---|---|
| `FEEDBACKMONK_LOG_FORMAT` | optional | `json` | | `json` (production default — emits structured JSON to stdout) or `text` (human-readable for local dev). Anything else → defaults to `json`. Source: `crates/feedbackmonk-api/src/main.rs:99`. **Note**: missing from current `.env.example`; Stage 2 Worker B should add it. |
| `RUST_LOG` | optional | `info` (implicit) | | tracing-subscriber `EnvFilter` directive. Common values: `info`, `feedbackmonk=debug,info`, `warn`. Source: `crates/feedbackmonk-tracing/src/lib.rs:101`. **Note**: missing from current `.env.example`; Stage 2 Worker B should add it. |

### Sessions & Authentication

| Name | Required | Default | 🔒 | Semantics |
|---|---|---|---|---|
| `FEEDBACKMONK_SESSION_SECRET` | **REQ** | — | 🔒 | HMAC key for signed admin-session cookies. **64 hex chars = 32 bytes**. Generate: `openssl rand -hex 32`. Rotation rotates all admin sessions (admins must re-login). Source: `crates/feedbackmonk-api/src/main.rs:268`, `crates/feedbackmonk-api/src/auth/session.rs:11`. |
| `FEEDBACKMONK_VERIFY_TOKEN_TTL_HOURS` | optional | `24` | | TTL in hours for the verify-email tokens minted at signup. Integer ≥ 1. Source: `crates/feedbackmonk-api/src/main.rs:144`. |
| `FEEDBACKMONK_JWT_LEEWAY_SECONDS` | optional | `5` | | Clock-skew tolerance for the JWT `iat` claim **ONLY**. `exp` remains strict per Contract C2 invariant 5. Integer ≥ 0. Source: `crates/feedbackmonk-api/src/main.rs:157`. |

### Anonymous Mode Rate Limiting

| Name | Required | Default | 🔒 | Semantics |
|---|---|---|---|---|
| `FEEDBACKMONK_ANON_RATE_LIMIT_PER_HOUR` | optional | `10` | | Per-`(anon_hash, project)` hourly submissions cap for FR-FBR-06 anonymous mode. Integer ≥ 1. Higher = more permissive; lower = stricter. P3+ tier enforcement gates raises by tier. Source: `crates/feedbackmonk-api/src/main.rs:149`. |

### Email Delivery

| Name | Required | Default | 🔒 | Semantics |
|---|---|---|---|---|
| `FEEDBACKMONK_MAILER` | optional | `mailpit` | | Mailer selection: `mailpit` (dev — no auth, talks to a local Mailpit server) or `smtp` (production — uses `FEEDBACKMONK_SMTP_*` vars). Anything else → startup error. Source: `crates/feedbackmonk-api/src/main.rs:188`. |
| `FEEDBACKMONK_SMTP_FROM` | optional | `no-reply@feedbackmonk.local` | | Visible `From:` address on outgoing emails. Override per deployment. Source: `crates/feedbackmonk-api/src/main.rs:189`. |
| `FEEDBACKMONK_MAILPIT_HOST` | optional* | `localhost` | | Mailpit dev SMTP host. *Required iff `FEEDBACKMONK_MAILER=mailpit`. Source: `crates/feedbackmonk-api/src/main.rs:192`. |
| `FEEDBACKMONK_MAILPIT_PORT` | optional* | `1025` | | Mailpit dev SMTP port. *Required iff `FEEDBACKMONK_MAILER=mailpit`. Source: `crates/feedbackmonk-api/src/main.rs:193`. |
| `FEEDBACKMONK_SMTP_HOST` | optional* | — | | Production SMTP server hostname. *Required iff `FEEDBACKMONK_MAILER=smtp`. Source: `crates/feedbackmonk-api/src/main.rs:201`. |
| `FEEDBACKMONK_SMTP_PORT` | optional* | `587` | | Production SMTP server port. *Required iff `FEEDBACKMONK_MAILER=smtp`. Source: `crates/feedbackmonk-api/src/main.rs:202`. |
| `FEEDBACKMONK_SMTP_USER` | optional* | — | 🔒 | Production SMTP username. *Required iff `FEEDBACKMONK_MAILER=smtp`. Source: `crates/feedbackmonk-api/src/main.rs:206`. |
| `FEEDBACKMONK_SMTP_PASS` | optional* | — | 🔒 | Production SMTP password. *Required iff `FEEDBACKMONK_MAILER=smtp`. Source: `crates/feedbackmonk-api/src/main.rs:207`. |
| `FEEDBACKMONK_SMTP_STARTTLS` | optional | `true` | | Whether to negotiate STARTTLS with the SMTP server. `true` / `false`. Defaults to `true` (modern SMTP is always STARTTLS). Source: `crates/feedbackmonk-api/src/main.rs:209`. |

---

## Self-Host Quickstart Env Profile

For a fresh self-host (`docker compose up` against the stack Worker B will ship under `deploy/docker/`), the **minimum required** vars are:

```
DATABASE_URL=postgres://feedbackmonk:CHANGEME@db:5432/feedbackmonk
FEEDBACKMONK_PUBLIC_URL=https://feedback.example.com  # your TLS endpoint
FEEDBACKMONK_SESSION_SECRET=<64 hex chars; openssl rand -hex 32>
```

If you skip Mailpit and need real email:

```
FEEDBACKMONK_MAILER=smtp
FEEDBACKMONK_SMTP_HOST=smtp.your-provider.com
FEEDBACKMONK_SMTP_USER=feedbackmonk@example.com
FEEDBACKMONK_SMTP_PASS=<smtp password>
FEEDBACKMONK_SMTP_FROM=no-reply@example.com
```

All other vars get their documented defaults.

---

## Worker-side Consumption

**Worker A** (`marketing/src/pages/docs/self-host.{md,mdx,astro}`): renders this catalog as a doc page. **Pulls names + defaults + semantics from this file** so Worker A and Worker B cannot disagree on the schema. Implementation options for Worker A: (a) hand-write a parallel table and rely on `selfhost-compose-smoke` Probe B parity check at Stage 3, or (b) parse this file's tables at marketing-build time and template-render the doc page. Option (b) is structurally drift-proof; option (a) is simpler. Stage 2 Worker A decides.

**Worker B** (`deploy/docker/docker-compose.yml`, `deploy/docker/.env.example`, `deploy/docker/README.md`): the `environment:` section of the `api` service lists every required var by name; optional vars are documented in the `deploy/docker/.env.example`. The `selfhost-compose-smoke` Verification Oracle Probe B walks the compose-env section and asserts every name is in this catalog (no orphan vars; catches typos like `FEEDBACKMONK_MAILEER` → not-in-catalog → FAIL).

---

## Decision Log

- **2026-05-14** — Catalog is the source of truth; `.env.example` (P0 baseline) is a *consumer*, not the authority. Rationale: P4 self-host docs need a single canonical surface; `.env.example` was developer-machine-shaped (dev defaults, dev port), while this file documents the **whole space** including SaaS vs self-host overlap. Worker B will update `.env.example` to add the two missing-from-baseline vars (`FEEDBACKMONK_LOG_FORMAT`, `RUST_LOG`) as part of Stage 2.
- **2026-05-14** — Security-sensitive flag (🔒) inline in the table, not separate. Rationale: scanability — an operator copying values can see at a glance which lines must NOT be committed.
- **2026-05-14** — Mailpit is the dev-mode default; production self-host expected to use `smtp`. Rationale: Mailpit isn't a real outbound mailer; a self-host operator who wants real emails configures `smtp`. The dev-mode default of `mailpit` is preserved for `docker compose up` quickstart-with-Mailpit-dev-profile (Worker B implements the `mailpit` profile-service).

---

## Out of Scope (deferred)

These vars do NOT exist yet but are anticipated for future phases. When their feature lands, they'll be appended to the table above:

- `FEEDBACKMONK_POLAR_WEBHOOK_SECRET` — Polar billing webhook HMAC verification (FR-FBR-15, currently DEFERRED per DEC-FBR-DEFER-01).
- `FEEDBACKMONK_S3_*` — attachment storage (DEC-FBR-08 OUT list, v1.1+).
- `FEEDBACKMONK_REDIS_URL` — distributed rate-limiter backend (D-FBR-08 deferred to v1.1).

Do not introduce these in P4; they're enumerated here only so future work doesn't accidentally clash with the canonical naming pattern.
