# Stage 1 → Stage 2 Handoff (Feedbackr P0 Foundation)

**Stage 1 worker**: stage1-worker (orchestrated session, autopilot:continuous)
**Concluded**: 2026-05-13
**Plan**: `docs/planning/plans/20260513T210133-feedbackr-p0-foundation.md`
**Arc plan**: `docs/planning/plans/20260513T185711-feedbackr-v1-build-arc.md`

This document is the FROZEN carry-state for Stage 2 Workers A and B. Worker A
owns FR-FBR-02 (signup/onboarding) + Contract C4 (signing-key registration).
Worker B owns FR-FBR-03 (submission) + FR-FBR-05 (JWT verification) +
FR-FBR-06 (anonymous mode) + Contracts C2/C3 + JWT fixture corpus.

---

## Frozen Contract C1 — Repository Surface

The `feedbackr-repository` crate IS Contract C1. **DO NOT widen any signature
without escalating via `channels/messages.md` and Lead Developer involvement.**
The `multi-tenant-isolation-check` Verification Oracle will fail the build if
you add a public method that lacks `&TenantScope` or `&ProjectScope` as the
first non-`&self` argument (unless allow-listed in `allowlist.toml` with
documented rationale — and the only allowed rationale is "pre-authentication
boundary").

### Files

| Path | Public surface |
|---|---|
| `crates/feedbackr-repository/src/lib.rs` | Re-exports of all traits, impls, scope types, errors. Read this first. |
| `crates/feedbackr-repository/src/scope.rs` | `TenantScope`, `ProjectScope` newtypes (`pub(crate)` constructors). Methods: `tenant_id()`, `project_id()`, `tenant()`. |
| `crates/feedbackr-repository/src/error.rs` | `RepoError { Sqlx, NotFound, Conflict, TenantProjectMismatch }`, `Result<T>` alias. |
| `crates/feedbackr-repository/src/tenants.rs` | `TenantRepo` trait + `SqlxTenantRepo` impl. |
| `crates/feedbackr-repository/src/projects.rs` | `ProjectRepo` trait + `SqlxProjectRepo` impl. **`open(&TenantScope, project_id) -> Result<ProjectScope>` is the SOLE constructor of `ProjectScope`.** |
| `crates/feedbackr-repository/src/signing_keys.rs` | `SigningKeyRepo` trait + `SqlxSigningKeyRepo` impl. |
| `crates/feedbackr-repository/src/feedback.rs` | `FeedbackRepo` trait + `SqlxFeedbackRepo` impl. Auth-mode + anonymous-mode submission, scoped list. |

### Trait method enumeration (frozen — no widening without escalation)

#### `TenantRepo`

```rust
async fn create(&self, email: &str, password_hash: &str) -> Result<Tenant>;          // pre-auth allowlisted
async fn find_by_email(&self, email: &str) -> Result<Option<Tenant>>;                // pre-auth allowlisted
async fn get(&self, scope: &TenantScope) -> Result<Tenant>;
async fn mark_verified(&self, scope: &TenantScope) -> Result<()>;
async fn scope_for(&self, id: uuid::Uuid) -> Result<TenantScope>;                    // pre-auth allowlisted
```

`scope_for` is the SOLE bridge from a verified-tenant Uuid to a `TenantScope`.
Worker A wraps it behind login/session handlers; nothing else may mint a
`TenantScope` because the type's constructor is `pub(crate)`.

#### `ProjectRepo`

```rust
async fn create(&self, scope: &TenantScope, name: &str, slug: &str) -> Result<Project>;
async fn list_for_tenant(&self, scope: &TenantScope) -> Result<Vec<Project>>;
async fn get(&self, scope: &ProjectScope) -> Result<Project>;
async fn open(&self, scope: &TenantScope, project_id: Uuid) -> Result<ProjectScope>;
```

`open` returns `RepoError::TenantProjectMismatch` when `project_id` does not
belong to `scope.tenant_id()`. Map to HTTP 403.

#### `SigningKeyRepo`

```rust
async fn register(&self, scope: &ProjectScope, public_key: &[u8; 32], label: &str) -> Result<SigningKeyId>;
async fn list_active(&self, scope: &ProjectScope) -> Result<Vec<SigningKey>>;
async fn deactivate(&self, scope: &ProjectScope, id: SigningKeyId) -> Result<()>;
```

`list_active` returns active keys in `registered_at ASC` order; the JWT
verifier in Worker B should try each in turn and return the first success.

#### `FeedbackRepo`

```rust
async fn submit_authenticated(
    &self,
    scope: &ProjectScope,
    end_user_sub: &str,
    end_user_email: Option<&str>,
    end_user_name: Option<&str>,
    external_metadata: Option<&serde_json::Value>,
    body: &str,
    kind: FeedbackKind,
) -> Result<FeedbackId>;

async fn submit_anonymous(
    &self,
    scope: &ProjectScope,
    anon_token_hash: &[u8; 32],
    optional_email: Option<&str>,
    body: &str,
    kind: FeedbackKind,
) -> Result<FeedbackId>;

async fn list_recent(&self, scope: &ProjectScope, limit: i64) -> Result<Vec<Feedback>>;
```

Both submit methods generate the public-facing `FB-XXXXXX` short code via
`FeedbackId::generate()`. The schema's UNIQUE constraint on `short_code`
enforces collision-free identifiers.

### Allow-listed pre-auth exceptions

`.claude/oracles/multi-tenant-isolation-check/allowlist.toml`:

| Trait | Method | Rationale |
|---|---|---|
| `TenantRepo` | `create` | Pre-auth: signup creates the tenant; no scope can exist yet. |
| `TenantRepo` | `find_by_email` | Pre-auth: login lookup runs before password verification. |
| `TenantRepo` | `scope_for` | Pre-auth bridge: mints first `TenantScope` from a verified `Uuid`. |

Plus inherent constructors (`Sqlx*Repo::new(pool: PgPool)`), which perform no
queries and are listed under `[[inherent_methods]]`.

**Adding to this allowlist is a Stage 1 / Lead Developer decision.** Workers A
and B may NOT add allowlist entries unilaterally.

---

## Schema columns Stage 2 may depend on

Migration: `migrations/00001_p0_schema.sql`. **Column names are normative;
renaming requires a coordinated downstream change. Do not introduce a
follow-up migration that renames columns during Stage 2.**

| Table | Columns Stage 2 hard-depends on |
|---|---|
| `tenants` | `id`, `email`, `password_hash`, `verified_at`, `tier`, `created_at`, `updated_at`. **Worker A adds nothing here in Stage 2 unless escalated.** |
| `projects` | `id`, `tenant_id`, `name`, `slug`, `created_at`. UNIQUE(`tenant_id`, `slug`). |
| `signing_keys` | `id`, `project_id`, `public_key` (BYTEA, 32 raw Ed25519 bytes), `label`, `active`, `registered_at`, `deactivated_at`. Partial index `signing_keys_project_active_idx` on (`project_id`) WHERE active=TRUE. |
| `feedback` | `id` (PK UUID), `short_code` (UNIQUE FB-XXXXXX), `project_id`, `tenant_id`, `end_user_sub`, `end_user_email`, `end_user_name`, `external_metadata` (JSONB), `anon_token_hash` (BYTEA 32), `body` (1..16384), `kind` (`bug`/`feature`/`question`/`other`), `accepted_at`. CHECK enforces XOR(`end_user_sub`, `anon_token_hash`). |
| `anon_submissions` | `(anon_token_hash, project_id)` PK; `first_seen_at`, `last_submission_at`, `submission_count`. The repository's `submit_anonymous` upserts this row. |
| `rate_limit_counters` | `(bucket_key, project_id, window_start)` PK; `count`. Worker B's in-memory governor MAY persist hints here, optional for P0. |

---

## Verification Oracle — invocation contract

Run on every commit during P0+. CI gates the build on PASS.

```bash
# Unix
bash .claude/oracles/multi-tenant-isolation-check/oracle.sh

# Windows
powershell -NoProfile -ExecutionPolicy Bypass -File .claude/oracles/multi-tenant-isolation-check/oracle.ps1
```

Exit 0 = PASS. Exit 1 = FAIL with file:line offenders printed.

Freshness invalidation triggers (per `manifest.json`): any change under
`migrations/`, `crates/feedbackr-repository/`, `crates/feedbackr-core/`,
`crates/feedbackr-api/`, `crates/feedbackr-jwt/` (when added by Worker B),
`crates/feedbackr-anon/` (when added by Worker B), or the allowlist.

**If you (Worker A or B) introduce a new crate**, update `manifest.json`'s
`freshness.triggers` list to include the new crate path so the oracle
invalidates correctly.

---

## What Stage 1 deliberately did NOT build

- **No JWT verifier.** Stage 2 Worker B's Task Zero is the JWT fixture corpus
  (per the plan's Testability Gate finding for FR-FBR-05). Contract C2 in the
  plan is the verifier API to implement.
- **No HTTP handlers.** `feedbackr-api/src/main.rs` ships a placeholder that
  binds the port and returns a banner. Worker A and Worker B add the real
  router tree (no overlap: A owns `/api/v1/signup`, `/api/v1/projects`,
  `/api/v1/projects/.../signing-keys`; B owns
  `/api/v1/projects/{id}/feedback`).
- **No anonymous-mode rate limiter.** Worker B picks the in-memory governor
  crate (per plan §Deferred Decisions; `governor` is the working default).
- **No email integration.** Worker A picks the SMTP wrapper (Mailpit in dev).
- **No `/health` endpoint.** Stage 3's scope.

---

## Test discipline

19 tests pass at Stage 1 (6 in `feedbackr-core`, 13 in `feedbackr-repository`).
**Workers A and B may add tests, but may not delete or weaken existing
assertions** (per the autopilot test-immutability rule).

`#[sqlx::test(migrations = "../../migrations")]` is the established pattern
for repository tests — each test runs in an isolated database, rolled back at
end. Use the same pattern for any new repo-touching tests in Stage 2.

---

## Local dev environment

See `docs/operations/LOCAL_DEV.md`. Key points for Workers A and B:

- Postgres dev container on **port 5433** (deconflicted from gitcellar's 5432).
- `DATABASE_URL=postgres://postgres:dev@localhost:5433/feedbackr_dev`.
- After modifying any `sqlx::query!` invocation: re-run `cargo sqlx prepare
  --workspace` and commit `.sqlx/`. CI uses `SQLX_OFFLINE=true`.
- Backend dev port: **`14304`** (env `FEEDBACKR_PORT`).

---

## Open questions for Lead Developer (NONE blocking Stage 2)

All decisions in P0 plan §Deferred Decisions remain valid. Workers A and B
should re-read that section when starting their first task.

---

## Convergence at Stage 2 end

Both workers report exit-gate-met → Lead Developer runs `/0-uldf-pods-converge`
→ Stage 3 (single-session) builds `/health` + structured logging in the
converging tree → `/0-uldf-finalize` closes P0.
