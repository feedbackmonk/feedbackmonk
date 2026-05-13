# feedbackr-repository

<!-- agent-synopsis -->
The SOLE query path for Feedbackr domain data. Tenant-scoped repository layer per DEC-FBR-03; raw SQL anywhere else is a security incident. Built in P0 Stage 1 per FR-FBR-01 + Contract C1.
<!-- /agent-synopsis -->

## Purpose & Responsibilities

This crate is **the** path from application code to the database. Every domain read or write goes through one of the four repository traits here (`TenantRepo`, `ProjectRepo`, `SigningKeyRepo`, `FeedbackRepo`). Per **DEC-FBR-03**, raw SQL anywhere outside this crate is a **security incident** — the `multi-tenant-isolation-check` Verification Oracle enforces this at AST grade on every commit.

The crate implements a three-leg defense against tenant-isolation drift:

| Leg | Mechanism | Where |
|---|---|---|
| **Leg 1** — type system | `TenantScope` / `ProjectScope` newtypes with `pub(crate)` constructors | `src/scope.rs` |
| **Leg 2** — AST oracle | `multi-tenant-isolation-check` greps for raw-SQL patterns + verifies first-arg discipline | `.claude/oracles/multi-tenant-isolation-check/` |
| **Leg 3** — lint baseline | `clippy::all = deny` workspace-wide; `clippy::pedantic` on this crate; `cargo-deny` on `deny.toml` | `Cargo.toml`, `deny.toml` |

## File Index

| File | Purpose |
|---|---|
| `src/lib.rs` | Crate root. Re-exports the public surface (the four traits, their sqlx impls, the scope newtypes, the error type). |
| `src/scope.rs` | `TenantScope` and `ProjectScope` newtypes. Constructors are `pub(crate)` — the type-system half of leg 1. |
| `src/error.rs` | `RepoError` (incl. `TenantProjectMismatch` for cross-tenant `ProjectRepo::open` rejection) and `Result` alias. |
| `src/tenants.rs` | `TenantRepo` trait + `SqlxTenantRepo` impl. **Three pre-auth allow-listed methods**: `create`, `find_by_email`, `scope_for`. 4 tests including cross-tenant lookup invariants. |
| `src/projects.rs` | `ProjectRepo` trait + `SqlxProjectRepo` impl. `open(&TenantScope, project_id)` is the **sole** `ProjectScope` constructor in the public API. 4 tests including `open_rejects_cross_tenant_project`. |
| `src/signing_keys.rs` | `SigningKeyRepo` trait + `SqlxSigningKeyRepo` impl. Always takes `&ProjectScope` first. 2 tests. |
| `src/feedback.rs` | `FeedbackRepo` trait + `SqlxFeedbackRepo` impl. `submit_authenticated`, `submit_anonymous`, `list_recent`. 3 tests including `list_recent_only_returns_scope_owner_rows`. |
| `Cargo.toml` | Adds `clippy::pedantic` on top of the workspace's `clippy::all = deny`. Depends on `sqlx`, `uuid`, `chrono`, `async-trait`, `thiserror`, `serde_json`, `feedbackr-core`. |

## Public API & Usage

```rust
use feedbackr_repository::{TenantRepo, ProjectRepo, FeedbackRepo, TenantScope, ProjectScope};

// Stage 2+ login handler:
let tenant = tenant_repo.find_by_email(email).await?;          // pre-auth (allow-listed)
verify_password(&tenant.password_hash, password)?;             // proves identity
let scope: TenantScope = tenant_repo.scope_for(tenant.id).await?;  // pre-auth bridge (allow-listed)

// Now every downstream call is type-checked tenant-isolated:
let project_scope: ProjectScope = project_repo.open(&scope, project_id).await?;
let feedback = feedback_repo.submit_authenticated(&project_scope, body, kind, ...).await?;
let recent = feedback_repo.list_recent(&project_scope, 20).await?;
```

The contract is **frozen** for Stage 2 consumption — Workers A and B treat the trait set as a library surface. See `docs/planning/handoffs/stage1-to-stage2.md`.

## Constraints & Business Rules

- **Constructor discipline (leg 1)**: `TenantScope::new` and `ProjectScope::new` are `pub(crate)`. `ProjectRepo::open` is the **sole** `ProjectScope` constructor in the public API.
- **Allowlist discipline (leg 2)**: The three pre-auth methods on `TenantRepo` (`create`, `find_by_email`, `scope_for`) are the **only** methods that may legitimately take a non-`&TenantScope` first argument. Adding a fourth requires updating `.claude/oracles/multi-tenant-isolation-check/allowlist.toml` with a documented rationale.
- **Pedantic clippy (leg 3)**: this crate runs `clippy::pedantic` in addition to the workspace `clippy::all = deny`.
- **`FeedbackRepo::submit_*` accept `kind: FeedbackKind`**: this is an EXTENSION of plan §C1 (additional info), not a WIDENING (both methods still take `&ProjectScope` first). The schema declares `kind` with CHECK constraint and FR-FBR-03 Contract C3 accepts optional `kind`.
- **`FeedbackRepo::list_recent` exists beyond plan §C1 enumeration**: used by 3/4 feedback tests as round-trip read path and by Stage 2 Worker A's forward-looking admin-feedback-list endpoint. Same scope discipline.

## Relationships & Dependencies

- **Consumes**: `feedbackr-core` (every query returns a `feedbackr_core::*` record).
- **Consumed by**: `feedbackr-api` (Stage 2+ HTTP handlers — Workers A and B), `feedbackr-jwt` (Stage 2 — signing-key lookup), `feedbackr-anon` (Stage 2 — rate-limit counters), future health crate, future admin UI backend.
- **Schema source of truth**: `migrations/00001_p0_schema.sql`. Every query in this crate hard-depends on those column names; schema column renames require a follow-up migration AND a coordinated change here.
- **Oracle**: `.claude/oracles/multi-tenant-isolation-check/` polices the layer boundary on every commit; CI gates the build on its exit code.

## Decision Log

### Constructors are `pub(crate)`; `ProjectRepo::open` is the sole `ProjectScope` constructor

**Decision**: `TenantScope::new` and `ProjectScope::new` are crate-private. The only public path to a `ProjectScope` is `ProjectRepo::open(&TenantScope, project_id)`, which enforces tenant→project ownership at the type-system boundary.

**Rationale**: `DEC-FBR-03` declares any raw SQL outside this crate a security incident. The type system half of the defense (leg 1) requires that *just having a `ProjectScope` value* proves the caller has already passed a tenant-ownership check. Public constructors would let any caller fabricate a `ProjectScope` from arbitrary `Uuid`s, defeating the type-system guarantee and reducing leg 1 to a naming convention. The `pub(crate)` discipline is what makes `ProjectScope` a real proof-carrying type rather than a tagged tuple.

**Trade-offs**: Slightly inconvenient for external consumers — they cannot mint a scope for testing without going through `open`. Mitigated by the in-crate tests living inside `crates/feedbackr-repository/src/**.rs` (`pub(crate)` is reachable), and by the fact that integration tests in higher crates SHOULD use the real `open` path anyway (that's how production code obtains a scope).

**Implementation**: `src/scope.rs` — `pub(crate) fn new(...)` on both newtypes. `src/projects.rs::ProjectRepo::open` is the sole public constructor of `ProjectScope`. Multi-tenant-isolation-check oracle enforces no `ProjectScope { ... }` struct literals outside this crate.

### `TenantRepo::scope_for(Uuid)` is allow-listed pre-auth

**Decision**: `TenantRepo::scope_for(uuid) -> Result<TenantScope>` is the third allow-listed pre-auth method (alongside `create` and `find_by_email`). It bridges a verified session-cookie tenant_id to a fresh `TenantScope`. Documented in `.claude/oracles/multi-tenant-isolation-check/allowlist.toml`.

**Rationale**: The pre-authentication boundary necessarily mints the **first** `TenantScope` from a verified caller. The `TenantScope` constructor is `pub(crate)`, so without `scope_for` (or an equivalent), Stage 2 Worker A's login handler would have no path from "I've validated this session cookie" to "...therefore here is a `TenantScope` for downstream calls." Naming the boundary explicitly — and gating it through a single method documented in the allowlist — is more honest than back-channels.

**Trade-offs**: Adds a third entry to the pre-auth allowlist. The risk is that the allowlist grows over time without corresponding tightening. Mitigated by requiring per-entry rationale in `allowlist.toml` and by the oracle freshness contract triggering on allowlist changes.

**Implementation**: `src/tenants.rs` — `async fn scope_for(&self, tenant_id: Uuid) -> Result<TenantScope>`. Returns `RepoError::NotFound` for unknown tenant_id (covered by `scope_for_unknown_tenant_returns_not_found` test). Allowlist entry at `.claude/oracles/multi-tenant-isolation-check/allowlist.toml` lines 32-35 carries the rationale.

### `FeedbackRepo::submit_*` accept `kind`; `list_recent` exists

**Decision**: Both `submit_authenticated` and `submit_anonymous` carry an explicit `kind: FeedbackKind` parameter, and `FeedbackRepo` includes a `list_recent(scope, limit)` method beyond the plan §C1 enumeration.

**Rationale**: Plan §C1 sketched the submit methods without explicit `kind`, but the schema declares `kind` with a CHECK constraint and FR-FBR-03 Contract C3 accepts optional `kind`. Forcing every caller to omit `kind` or default it at the DB layer pushes a fundamental piece of feedback metadata through the wrong seam. Same principle for `list_recent`: 3/4 of the feedback unit tests need a round-trip read path, and Stage 2 Worker A's admin-feedback-list endpoint will consume it.

**Trade-offs**: Mild departure from the plan's literal §C1 sketch. Both additions are EXTENSIONS (additional info / additional method) rather than WIDENINGS (the `&ProjectScope` first-arg discipline is preserved on every method). If the orchestrator ratifies a Contract C1 amendment in the other direction (default `kind` at DB layer; drop `list_recent`), both are trivially removable.

**Implementation**: `src/feedback.rs` trait definitions. The orchestrator has explicitly noted these as forward-evolutions of C1 in the Stage 1 completion brief and in `docs/specs/DECISIONS.md` (DEC-FBR-IMPL-01).

### Pedantic clippy on the repository crate

**Decision**: `crates/feedbackr-repository/Cargo.toml` declares `[lints.clippy] pedantic = { level = "warn", priority = -1 }` on top of the workspace `clippy::all = deny`.

**Rationale**: This crate IS the security boundary for tenant isolation. The opportunity cost of letting an avoidable footgun ship here is much higher than in `-core` (pure data) or `-api` (HTTP plumbing). Pedantic clippy catches several classes of subtle issue (lossy casts, awkward error propagation, unnecessarily-loose types) that show up in DB-touching code disproportionately.

**Trade-offs**: Some pedantic lints are noisy; we'll allow individual lints case-by-case rather than blanket-disable. The cost is mild lint-management overhead.

**Implementation**: `Cargo.toml` `[lints.clippy]` block. Verified GREEN on this commit via `cargo clippy --workspace --all-targets -- -D warnings`.
