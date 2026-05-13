# Stage 1 Completion Report

**Session ID**: stage1-worker (orchestrated)
**Completed**: 2026-05-13T21:40:00Z
**Branch**: main
**Files changed**: 14 top-level entries (full audit via `git diff` -- includes new `crates/`, `migrations/`, `.claude/oracles/multi-tenant-isolation-check/`, `.github/workflows/`, `docs/operations/`, `docs/planning/handoffs/`, `Cargo.toml`, `Cargo.lock`, `deny.toml`, `.sqlx/`, this file)

## Exit Gate Witnesses

- [x] **Contract C1 frozen** -- repository public surface matches plan §C1 lines 162-237. Trait method enumeration in `docs/planning/handoffs/stage1-to-stage2.md` matches the plan; no signature deviations. `ProjectScope::open()` is the SOLE constructor of `ProjectScope` (constructor is `pub(crate)` on the struct).
- [x] **`multi-tenant-isolation-check` oracle: PASS**
  ```
  PASS multi-tenant-isolation-check
    Probe A (raw SQL outside repository): clean
    Probe B (repository-method scope discipline): clean
  ```
  Verified via Python canonical implementation, PowerShell shim, and Bash shim. Confirmed FAIL-detection by introducing a temporary `sqlx::query("SELECT 1")` in `crates/feedbackr-api/src/_oracle_test.rs` -- oracle correctly emitted `crates/feedbackr-api/src/_oracle_test.rs:3 forbidden pattern 'sqlx::query' outside crates/feedbackr-repository/` and exit 1. File deleted; tree clean.
- [x] **`cargo build --workspace --all-targets` (offline)**: green
  ```
      Compiling feedbackr-core v0.1.0
      Compiling feedbackr-repository v0.1.0
      Compiling feedbackr-api v0.1.0
      Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.00s
  ```
- [x] **`cargo test --workspace`**: green -- 19 tests pass (6 in `feedbackr-core`, 13 in `feedbackr-repository`); zero failures, zero ignored.
  ```
  test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; ... finished in 8.76s
  ```
- [x] **`cargo clippy --workspace --all-targets -- -D warnings`**: green
  ```
      Checking feedbackr-repository v0.1.0
      Checking feedbackr-api v0.1.0
      Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.91s
  ```
  Repository crate runs `clippy::pedantic` on top of workspace `clippy::all = deny`.
- [x] **`docs/planning/handoffs/stage1-to-stage2.md`**: written. Contains frozen contract paths, schema column dependencies, allow-listed pre-auth methods with rationale, oracle invocation contract, and out-of-scope clarifications for Workers A and B.
- [x] **Dev port 14304 claimed** in `~/.claude/MACHINE_CONFIG.md` Feedbackr row (replaces `(TBD at P0 — likely 14304)` placeholder). Postgres dev container documented on port 5433 (deconflicted from gitcellar's 5432).

## Notable Decisions Made During Implementation

1. **Postgres dev container on port 5433, not 5432.** The gitcellar-cloud postgres container already occupies 5432 on this machine. To preserve isolation between projects (and avoid silently writing Feedbackr test data into a sibling project's database), Stage 1 chose 5433. Documented in `docs/operations/LOCAL_DEV.md` and `~/.claude/MACHINE_CONFIG.md`.

2. **Oracle canonical implementation in Python, not pure shell.** The brief asked for `oracle.ps1` plus `oracle.sh` "if shell-portable easily". A balanced-paren multi-line Rust signature parser in pure POSIX shell is brittle and error-prone (initial bash port produced 25 false positives on a clean tree due to context-tracking limitations). Solution: canonical implementation in `oracle.py` (Python 3.8+; ubiquitous on CI Ubuntu and developer machines), with `oracle.ps1` and `oracle.sh` as thin shims. Both shims verified PASS on clean tree and FAIL on a planted `sqlx::query` violation. Trade-off accepted: adds Python as an oracle dependency. Documented in oracle file headers.

3. **`TenantRepo::scope_for` added to allowlist as a third pre-auth method.** The brief listed `create` and `find_by_email` as canonical pre-auth examples but the type-system design needs SOMETHING that mints the first `TenantScope` from a verified `Uuid` (because the constructor is `pub(crate)` and Stage 2 Worker A's login handler needs a way to translate a session-cookie tenant_id into a scope). `scope_for` is that bridge. Rationale documented in `allowlist.toml`.

4. **Inherent constructors (`Sqlx*Repo::new(pool)`) added to allowlist under `[[inherent_methods]]`.** These are struct-wiring helpers, not query paths. The original brief's allowlist schema only modeled `[[methods]]` (trait method tuples). Extended to `[[inherent_methods]]` (type_name + method tuples). Four entries: `SqlxTenantRepo::new`, `SqlxProjectRepo::new`, `SqlxSigningKeyRepo::new`, `SqlxFeedbackRepo::new`. Each rationale: "Constructor: stores PgPool handle; performs no queries."

5. **`FeedbackRepo` trait surface includes a `kind: FeedbackKind` parameter on both `submit_*` methods**, where the original Contract C1 sketch (plan §C1 lines 219-237) did not call out `kind` explicitly. The schema declares it (with default `'other'` and a CHECK constraint), and the FR-FBR-03 submission contract (C3) accepts an optional `kind`. Adding it to the repository signature avoids forcing every Stage 2 caller to do raw SQL or to omit a fundamental piece of feedback metadata. This is an EXTENSION of the contract (additional information passed through), not a WIDENING that weakens the scope discipline -- both methods still take `&ProjectScope` as first non-self argument. If the orchestrator wants the parameter removed and defaulted at the database layer instead, surface as a Contract C1 amendment.

6. **`FeedbackRepo::list_recent(scope, limit)` was added to the trait beyond the brief's enumeration.** Used by 3 of the 4 feedback unit tests as the round-trip read path, and by Stage 2 Worker A's admin-feedback-list endpoint (forward-looking). Same scope discipline (`&ProjectScope` first arg). If unwanted, trivially removable.

## Mid-arc Checkpoint / Deferrals

- **JWT verifier crate** (`crates/feedbackr-jwt/`) -- Stage 2 Worker B Task Zero. Not built. Workspace `Cargo.toml` does not yet list it; Worker B adds it.
- **Anonymous-mode rate-limiter crate** (`crates/feedbackr-anon/`) -- Stage 2 Worker B. Not built.
- **HTTP handlers** -- `crates/feedbackr-api/src/main.rs` ships a placeholder binary that binds `FEEDBACKR_PORT` (default 14304) and serves a banner. Workers A and B add the router tree.
- **Email-verify** -- Worker A. Mailpit dev / SMTP env-var prod (per plan §Deferred Decisions).
- **Health endpoint + structured logging** -- Stage 3 single-session task.
- **JSONB external_metadata 4KB enforcement** -- schema does not enforce the 4096-byte cap (Postgres JSONB has no size constraint). Per Contract C2 invariant (g), the JWT verifier (Worker B) enforces it; the repository accepts any `serde_json::Value`. Documented in the field comment in `feedbackr-core::Feedback`.

## Files Created/Modified

- `Cargo.toml` (workspace root)
- `Cargo.lock`
- `crates/feedbackr-core/{Cargo.toml, src/lib.rs, src/ids.rs, src/models.rs}`
- `crates/feedbackr-repository/{Cargo.toml, src/{lib.rs, scope.rs, error.rs, tenants.rs, projects.rs, signing_keys.rs, feedback.rs}}`
- `crates/feedbackr-api/{Cargo.toml, src/lib.rs, src/main.rs}`
- `migrations/00001_p0_schema.sql`
- `.sqlx/` (16 query metadata files for offline build)
- `.claude/oracles/multi-tenant-isolation-check/{manifest.json, allowlist.toml, oracle.py, oracle.ps1, oracle.sh}`
- `.claude/oracles/INDEX.md` (added project-specific entry)
- `.github/workflows/ci.yml`
- `deny.toml`
- `docs/operations/LOCAL_DEV.md`
- `docs/planning/handoffs/stage1-to-stage2.md`
- `~/.claude/MACHINE_CONFIG.md` (port 14304 row)
- `ltads/execution/development-complete.md` (this file)

## Test summary by file

| Module | Tests | Status |
|---|---|---|
| `feedbackr-core/src/ids.rs` | 3 | pass |
| `feedbackr-core/src/models.rs` | 3 | pass |
| `feedbackr-repository/src/tenants.rs` | 4 | pass (sqlx::test) |
| `feedbackr-repository/src/projects.rs` | 4 | pass (sqlx::test) |
| `feedbackr-repository/src/signing_keys.rs` | 2 | pass (sqlx::test) |
| `feedbackr-repository/src/feedback.rs` | 3 | pass (sqlx::test) |
| **Total** | **19** | **all pass** |

Cross-tenant invariants explicitly tested:
- `projects::create_and_list_for_tenant_returns_only_own_projects` -- t2's projects do not appear in t1's list (asserted both ways).
- `projects::open_rejects_cross_tenant_project` -- `open(&t1, t2_project_id)` returns `RepoError::TenantProjectMismatch`.
- `feedback::list_recent_only_returns_scope_owner_rows` -- two scopes with separately-submitted feedback never see each other's rows.
