# Stage 1 Worker Brief — Feedbackr P0 Foundation Contract

**Session role**: development worker (Orchestrated Execution)
**Parent orchestrator session**: S001 (monitoring this worker)
**Autonomy**: autopilot:continuous (inherited via `.claude/session-state/task-arc-autonomy.json`)
**Project root**: `E:\Developer\SourceControlled\Apps\Feedbackr`
**Strategy**: SEQUENTIAL — you are the sole agent on Stage 1. Workers A and B in Stage 2 will consume your output as a frozen library surface.

---

## Read first (in this order)

1. `CLAUDE.md` (project) — load-bearing privacy invariants + license stub constraint + tech stack + GitCellar reference rule
2. `docs/planning/plans/20260513T210133-feedbackr-p0-foundation.md` — **§Stage 1 brief; §Oracle Pre-Build Plan; §Interface Contracts §C1 (the contract you freeze); §Risks (mitigations you implement); §Testability Gate findings for FR-FBR-01**
3. `docs/specs/SPECIFICATION.md` — **FR-FBR-01** (only — Stages 2/3 cover the others)
4. `docs/specs/DECISIONS.md` — **DEC-FBR-03** (repository layer is sole query path; raw SQL outside is a security incident); **DEC-FBR-04** (JWT is sole end-user identity); **DEC-FBR-07** (greenfield — read-only GitCellar reference, no extraction)
5. `docs/planning/plans/20260513T185711-feedbackr-v1-build-arc.md` — only §P0 row + §Cross-Phase Carry-State (for downstream awareness; do not implement P1+ scope)

You may read peer GitCellar code if it exists (`E:\Developer\SourceControlled\Apps\gitcellar\` or similar — check `~/.claude/MACHINE_CONFIG.md` for the path) for **reference only**. Per DEC-FBR-07: this Feedbackr repo is greenfield; **DO NOT modify any GitCellar file** and **DO NOT extract code**. Port patterns by re-implementation, not copy-paste.

---

## Scope (Stage 1 — frozen)

### Task Zero — `multi-tenant-isolation-check` Verification Oracle

**Build BEFORE any data-write code lands.** This is the foundational defense for the entire P0+ arc.

**Output location**: `.claude/oracles/multi-tenant-isolation-check/`

**Required structure**:
- `manifest.json` — `kind: "verification"`, freshness contract (invalidate-on-change for `migrations/`, `crates/feedbackr-repository/**`, `crates/feedbackr-core/**`, `crates/feedbackr-api/**`), entry-point command
- `oracle.ps1` (and matching `oracle.sh` if shell-portable easily) — runs two probes:
  - **Probe A (AST/grep)**: scan `crates/feedbackr-api/`, `crates/feedbackr-jwt/` (if exists), and any handler crate **outside `crates/feedbackr-repository/`**. Fail if any of: `sqlx::query`, `sqlx::query!`, `sqlx::query_as`, `sqlx::query_scalar`, `pool.acquire(`, `&mut Connection`, `&mut PgConnection`, `&mut Transaction`, `Pool<Postgres>` as a free-floating param. Allow these only **inside** `crates/feedbackr-repository/`.
  - **Probe B (repository-method audit)**: parse public function signatures in `crates/feedbackr-repository/src/**` (regex-grade parsing is acceptable for Rust if AST tooling isn't immediately available — document the limitation). Every `pub async fn` and `pub fn` on every trait/impl must have `&TenantScope` or `&ProjectScope` as the first non-`&self` argument, **except**: methods marked `#[doc(hidden)]` AND `pub(crate)`, AND callsites manually allow-listed in `.claude/oracles/multi-tenant-isolation-check/allowlist.toml` (start empty; legitimate exceptions like `TenantRepo::create()` and `TenantRepo::find_by_email()` — the pre-authentication boundary — go here with rationale comments).
- Output format: machine-parseable PASS/FAIL with file:line offenders on FAIL.
- Exit code 0 on PASS; non-zero on FAIL.

**CI wiring**: add a GitHub Actions stub at `.github/workflows/ci.yml` that runs the oracle (plus `cargo build`, `cargo test`, `cargo clippy -- -D warnings`). Even though we don't have a remote yet (LICENSE-pending), the file is contract for the eventual public CI.

**Register in oracle index**: append to `.claude/oracles/INDEX.md` (create if missing) with: name, kind, freshness, consumer scope (P0+ all crates).

---

### Sub-task 1 — FR-FBR-01: Data Model + Tenant-Scoped Repository

#### Cargo workspace

Create `Cargo.toml` at repo root as a workspace:

```toml
[workspace]
resolver = "2"
members = [
    "crates/feedbackr-core",
    "crates/feedbackr-repository",
    "crates/feedbackr-api",
]

[workspace.package]
edition = "2021"
license = "AGPL-3.0-or-later"
authors = ["Feedbackr Contributors"]
version = "0.1.0"

[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
sqlx = { version = "0.8", features = ["runtime-tokio", "postgres", "uuid", "chrono", "json", "macros", "migrate"] }
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
async-trait = "0.1"
thiserror = "2"
anyhow = "1"
argon2 = "0.5"
axum = "0.7"
tracing = "0.1"
```

Pin exact versions only if a known incompatibility forces it; otherwise use minor-version pins as above for flexibility.

#### `crates/feedbackr-api` (placeholder)

Empty crate with `src/lib.rs` containing `pub fn placeholder() {}` and a `Cargo.toml` declaring dependency on `feedbackr-repository`. Stage 2 Workers A and B build into this crate.

#### `crates/feedbackr-core` — domain types

`src/lib.rs` exports: `Tenant`, `Project`, `SigningKey`, `SigningKeyId`, `Feedback`, `FeedbackId`, `AnonSubmission`, `RateLimitCounter`. All are plain data structs with `serde::Serialize + Deserialize`. UUID-typed IDs where appropriate (newtype `SigningKeyId(Uuid)`, `FeedbackId(String)` — FB-IDs are public-facing short codes like `FB-ABC123`, NOT raw UUIDs; allocate via a short-code generator).

Define `FeedbackKind` enum: `Bug | Feature | Question | Other`.

No methods that hit the DB live in `feedbackr-core` — pure types only.

#### `crates/feedbackr-repository` — Contract C1 (FROZEN)

This crate's public API IS Contract C1 from the P0 plan. **Do not deviate.** If during implementation you discover an inadequacy in C1, **stop and emit a Mid-arc Checkpoint** entry in `ltads/sessions/current-session.md` AND `ltads/execution/blockers.md` describing the inadequacy and your recommendation — do not silently widen the contract. The orchestrator will decide whether to revise the plan.

Files:
- `src/lib.rs` — re-exports `scope`, traits, and the concrete sqlx-backed impls
- `src/scope.rs` — `TenantScope`, `ProjectScope` (newtypes per Contract C1 §line 171-188). Constructors `pub(crate)` only. `ProjectScope::open()` is the SOLE constructor of `ProjectScope`, and it lives as a method on `ProjectRepo` (per Contract C1 line 205-207).
- `src/error.rs` — `RepoError` enum + `Result<T> = std::result::Result<T, RepoError>`. Variants: `Sqlx(sqlx::Error)`, `NotFound`, `Conflict`, `TenantProjectMismatch` (when `open()` is called with a project_id that doesn't belong to the tenant in scope).
- `src/tenants.rs` — `TenantRepo` trait + `SqlxTenantRepo` impl. Methods per Contract C1 lines 194-199 plus reasonable read-only auxiliaries (`get_by_id(&TenantScope) -> Result<Tenant>`).
- `src/projects.rs` — `ProjectRepo` trait + `SqlxProjectRepo` impl per C1 lines 202-208. Includes the `open()` constructor of `ProjectScope`.
- `src/signing_keys.rs` — `SigningKeyRepo` trait + impl per C1 lines 211-216.
- `src/feedback.rs` — `FeedbackRepo` trait + impl per C1 lines 219-237.
- `src/lib.rs` re-exports.

**Invariant (oracle-enforced)**: every public function on every repository trait MUST take `&TenantScope` or `&ProjectScope` as its first non-`&self` argument, OR be an allow-listed pre-auth method (`TenantRepo::create`, `TenantRepo::find_by_email` — these necessarily run before a `TenantScope` exists; document with `// allowlisted-pre-auth: ...` comments AND list in `.claude/oracles/multi-tenant-isolation-check/allowlist.toml`).

#### `migrations/00001_p0_schema.sql`

PostgreSQL schema. Required tables (column names and constraints are normative — Stage 2 workers depend on these):

```sql
-- tenants
CREATE TABLE tenants (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    verified_at TIMESTAMPTZ,
    tier TEXT NOT NULL DEFAULT 'free',  -- forward-looking: P3 tier enforcement
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- projects
CREATE TABLE projects (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    slug TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, slug)
);
CREATE INDEX projects_tenant_id_idx ON projects (tenant_id);

-- signing_keys (Ed25519 public keys for JWT verification; per FR-FBR-05 + Contract C4)
CREATE TABLE signing_keys (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    public_key BYTEA NOT NULL,  -- 32 bytes raw Ed25519
    label TEXT NOT NULL,
    active BOOLEAN NOT NULL DEFAULT TRUE,
    registered_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deactivated_at TIMESTAMPTZ
);
CREATE INDEX signing_keys_project_active_idx ON signing_keys (project_id) WHERE active = TRUE;

-- feedback (the heart of the product)
CREATE TABLE feedback (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    short_code TEXT NOT NULL UNIQUE,  -- public-facing FB-ABC123 form
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,  -- denormalized for tenant-scoped queries
    end_user_sub TEXT,           -- JWT sub claim (auth mode); NULL for anonymous
    end_user_email TEXT,         -- optional, either source
    end_user_name TEXT,          -- optional, JWT-mode only
    external_metadata JSONB,     -- ≤ 4KB enforced at app layer; auth-mode only
    anon_token_hash BYTEA,       -- 32 bytes; anonymous-mode only
    body TEXT NOT NULL CHECK (length(body) BETWEEN 1 AND 16384),
    kind TEXT NOT NULL DEFAULT 'other' CHECK (kind IN ('bug','feature','question','other')),
    accepted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((end_user_sub IS NOT NULL) <> (anon_token_hash IS NOT NULL))  -- exactly one mode
);
CREATE INDEX feedback_project_accepted_idx ON feedback (project_id, accepted_at DESC);
CREATE INDEX feedback_tenant_idx ON feedback (tenant_id);

-- anon_submissions (dedup tracking — per-cookie/project, distinct from feedback rows)
CREATE TABLE anon_submissions (
    anon_token_hash BYTEA NOT NULL,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    first_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_submission_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    submission_count INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (anon_token_hash, project_id)
);

-- rate_limit_counters (forward-looking for P0 anon mode; in-memory governor is primary, but persisted counters allow restart-recovery hints)
CREATE TABLE rate_limit_counters (
    bucket_key TEXT NOT NULL,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    window_start TIMESTAMPTZ NOT NULL,
    count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (bucket_key, project_id, window_start)
);
```

Use `sqlx::migrate!("./migrations")` from the api crate at startup. **Do not** rename or restructure columns in a follow-up migration during Stage 1 — Stage 2 workers will hard-depend on these column names.

#### sqlx offline cache

Run `cargo sqlx prepare --workspace` after the schema and all `sqlx::query!` macro uses compile. Commit `.sqlx/` so CI builds without DATABASE_URL. Add `SQLX_OFFLINE=true` to CI env.

If you don't have a local Postgres available and the `prepare` step blocks you, set up a Docker `postgres:17` sidecar:

```bash
docker run -d --name feedbackr-pg-dev -p 5432:5432 \
  -e POSTGRES_PASSWORD=dev -e POSTGRES_DB=feedbackr_dev \
  postgres:17-alpine
# DATABASE_URL=postgres://postgres:dev@localhost:5432/feedbackr_dev
```

Document this in `docs/operations/LOCAL_DEV.md` (create it).

#### Tests (Stage 1 minimum)

Unit tests in each `crates/feedbackr-repository/src/<table>.rs` file verifying:
- A `TenantScope` cannot reach another tenant's projects (call `list_for_tenant` after seeding two tenants, assert the other tenant's projects are absent)
- `ProjectScope::open()` returns `TenantProjectMismatch` when given a project_id that belongs to a different tenant
- `FeedbackRepo::submit_authenticated` and `submit_anonymous` round-trip correctly
- `SigningKeyRepo::list_active` excludes deactivated keys

Use `sqlx::test` for transactional tests against the Docker Postgres above (each test runs in a transaction that rolls back — no test pollution). If Docker isn't available, write integration tests as `#[ignore]` with documentation pointing to `LOCAL_DEV.md`.

**Test files are read-only at autopilot:continuous by default** — but no test files exist yet, so you are explicitly authorized to create them. Once written, you may not delete or weaken existing test assertions; you may only add.

#### Three-leg defense for FR-FBR-01

1. **Type system** — `TenantScope`/`ProjectScope` newtypes with `pub(crate)` construction (above). ✓ language-enforced.
2. **Oracle** — `multi-tenant-isolation-check` (Task Zero above). ✓ runtime-enforced.
3. **Clippy/cargo-deny** — add `[lints]` section to `crates/feedbackr-repository/Cargo.toml` enabling `clippy::pedantic` (allow `clippy::module_name_repetitions` per Rust idiom). Add `deny.toml` at workspace root that fails build on banned crates (start small: deny `rusqlite`, `mysql`, any non-`sqlx` DB driver to prevent accidental introduction of a second query path).

---

### Dev port registration

Append a row to `~/.claude/MACHINE_CONFIG.md` Dev Port Registry under the Feedbackr entry, claiming **`14304`** in the `14300-14399` range for the backend API. Update the Feedbackr row's `API` column to `14304`. If the registry section format is unfamiliar, follow the same column structure as adjacent rows.

The API crate's `main.rs` (placeholder OK for Stage 1; Stage 2 Worker B will flesh it out) should read `FEEDBACKR_PORT` env var with default `14304` and bind with `axum::serve` using `tokio::net::TcpListener::bind`.

---

## Stage 1 exit conditions (durable witnesses)

You may EXIT (write `development-complete.md` and close the terminal) only when **all** of these are true:

1. **Contract C1 frozen** exactly as documented in P0 plan §C1 (lines 162-237 of the plan). Verify by reading the plan and your own `crates/feedbackr-repository/src/lib.rs` re-exports side-by-side. No method signature deviation.
2. **`multi-tenant-isolation-check` oracle GREEN** on the current working tree. Run it. Capture output. Include the PASS line in your completion report.
3. **`cargo build` green** at workspace root (`cargo build --workspace`).
4. **`cargo test` green** at workspace root (or all tests `#[ignore]` with documented reason if Docker not available — but you should have Docker on this machine; check `docker info`).
5. **`cargo clippy --workspace -- -D warnings` green**.
6. **`docs/planning/handoffs/stage1-to-stage2.md`** written, listing:
   - Frozen contract paths Workers A and B will consume (`crates/feedbackr-repository/src/scope.rs`, each repo trait file with its public surface enumerated)
   - Schema columns Stage 2 may depend on (cite migration file)
   - Any allow-listed pre-auth methods (with rationale)
   - The exact `multi-tenant-isolation-check` invocation command
7. **Dev port `14304` claimed** in `~/.claude/MACHINE_CONFIG.md`.

---

## Constraints (must respect — re-read after each major change)

- **LICENSE file is a stub. DO NOT modify.** Replacement is user action pre-public-commit (per project CLAUDE.md and DEC-FBR-05). Pre-existing.
- **No source-level changes to GitCellar.** Read-only reference only (DEC-FBR-07).
- **No raw SQL outside `crates/feedbackr-repository/`** — DEC-FBR-03 declares this a security incident. The oracle enforces this; do not try to bypass.
- **Privacy invariants** (project CLAUDE.md §Privacy invariants): no third-party trackers anywhere. None of Stage 1's code surfaces a network egress, but this is a project-wide rule — if you add a dependency, verify it doesn't phone home.
- **No commits.** The orchestrator handles commits at Stage 1 exit gate via `/0-uldf-finalize`. **You write code, write the completion report, and EXIT.** Do not run `git commit`, do not run `git push`, do not run `/0-uldf-ltads-stop`, do not modify `ltads/sessions/current-session.md` (the orchestrator owns it).
- **Test files**: you are creating them fresh, so the "read-only at autopilot" rule doesn't bind yet. Once they exist, treat them as immutable contract.

---

## Tech defaults (do not re-litigate without escalating)

| Concern | Decision |
|---|---|
| Web framework | `axum` v0.7+ (placeholder import in api crate is fine for Stage 1) |
| Query layer | `sqlx` v0.8 with compile-time checking + `sqlx::migrate!` |
| Offline build | `cargo sqlx prepare` → commit `.sqlx/` |
| Password hash | `argon2` v0.5 (default params; RFC 9106) |
| JWT crypto | `jsonwebtoken` v9+ (Stage 2 concern — don't pre-import in Stage 1) |
| Env-var prefix | `FEEDBACKR_` for all runtime config |
| Async runtime | `tokio` full features |
| UUID generation | DB-side via `gen_random_uuid()` (Postgres `pgcrypto` — enable in migration if not default) |

If `pgcrypto` extension isn't auto-enabled, add `CREATE EXTENSION IF NOT EXISTS pgcrypto;` at the top of `00001_p0_schema.sql`.

---

## Risks to actively defend against

From the P0 plan §Risks — these are YOUR Stage 1 mitigations:

1. **Tenant isolation drift** — the three-leg defense (type + oracle + clippy) IS the mitigation. Wire all three before any non-trivial code lands.
2. **`sqlx::query!` requires DATABASE_URL at build** — use `cargo sqlx prepare` and commit `.sqlx/`. CI uses `SQLX_OFFLINE=true`.
3. **Stage 1 over-elaboration eating Stage 2 budget** — hard checklist above is the discipline. Six repo trait surfaces, one oracle, one migration. If you find yourself implementing FR-FBR-02/03/05/06 logic, STOP — that's Stage 2.

---

## Completion report format

Write to `ltads/execution/development-complete.md` when exit gate is met:

```markdown
# Stage 1 Completion Report

**Session ID**: (use the new worker session ID from the session-start hook briefing, or "stage1-worker" if unknown)
**Completed**: <ISO 8601 timestamp>
**Branch**: main
**Files changed**: <count>

## Exit Gate Witnesses

- [x] Contract C1 frozen (cite file: `crates/feedbackr-repository/src/lib.rs` + trait files)
- [x] `multi-tenant-isolation-check` oracle: PASS (paste output)
- [x] `cargo build --workspace`: green (paste tail)
- [x] `cargo test --workspace`: green (paste summary)
- [x] `cargo clippy --workspace -- -D warnings`: green (paste tail)
- [x] `docs/planning/handoffs/stage1-to-stage2.md`: written
- [x] Dev port 14304 claimed in MACHINE_CONFIG.md

## Notable Decisions Made During Implementation

(List anything that the plan didn't pre-decide — if there's nothing, write "None — all decisions pre-resolved in plan §Deferred Decisions.")

## Mid-arc Checkpoint / Deferrals

(Anything intentionally left for Stage 2+ that you'd surface to the next session)

## Files Created/Modified

(Brief list — full audit via git diff)
```

Then exit the terminal (close it or type `exit`).

---

## Critical operational rules

- **Do NOT run `/0-uldf-ltads-stop`** — orchestrator owns this.
- **Do NOT run `git commit` / `git push`** — orchestrator owns this via `/0-uldf-finalize`.
- **Do NOT modify** `ltads/sessions/current-session.md`, `ltads/execution/spec-progress.md`, `ltads/execution/commit-log.md` — orchestrator owns these.
- **You MAY** create/modify `ltads/execution/blockers.md` if you hit a critical blocker requiring orchestrator decision before continuing.
- **You MAY** create/modify `ltads/execution/development-complete.md` (your final report).
- **You MAY** create any file under `crates/`, `migrations/`, `.claude/oracles/`, `docs/operations/`, `docs/planning/handoffs/`, `.github/workflows/`, `Cargo.toml`, `deny.toml`.

When done: write the completion report, then EXIT this session (close terminal or type `exit`). The orchestrator session will detect the report and continue.
