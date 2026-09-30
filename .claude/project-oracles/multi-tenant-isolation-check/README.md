# multi-tenant-isolation-check

## Summary

Defends DEC-FBR-03: the tenant-scoped repository crate is the **only** path to the database, and every public repository method is scope-bound. Raw SQL anywhere else is a cross-tenant leak waiting to happen, so this runs in the CI suite on every push. Come here when a change touches query code, a handler that opens a transaction, a repository signature, or `allowlist.toml`.

## Probes

**Probe A — nothing outside `crates/feedbackmonk-repository/` reaches the database** (every `.rs` under `crates/`, tests included; comments and string-literal contents are lexed out first, so `"http://x"; sqlx::query(..)` on one line cannot hide the call):

| Check | Fails on |
|---|---|
| A1 sqlx whitelist | any `sqlx::` path or `use sqlx::…` leaf other than `PgPool`, `postgres::PgPoolOptions`, `Error`, `test` — so `raw_sql`, `query*`, `QueryBuilder`, `Executor`, `Acquire`, `Transaction`, `PgConnection`, glob imports and `use sqlx as x` all fail |
| A2 bare builders | `query(` / `query_as(` / `query_scalar(` / `*_with(` / `raw_sql(` (turbofish included) in a file that mentions sqlx; `query!`-family macros and `raw_sql(` anywhere |
| A3 string SQL | `.execute/.fetch*/.prepare/.describe(` whose first argument is a string literal, `format!`/`concat!`, or an executor (`pool`, `tx`, `conn`, `&mut *x`) |
| A4 pool executor | `pool.execute*/fetch*/prepare*/describe(`, plus the legacy tokens `pool.acquire(`, `&mut (Pg)Connection`, `&mut Transaction`, `Pool<Postgres>`, `extern crate sqlx` |
| A5 transaction discipline | a `.begin()` not bound as `let [mut] tx = <pool>.begin().await?;`; or any later use of that `tx` other than `&mut tx` / `tx` passed **directly** to a repository method whose signature takes a `PgConnection` / `Transaction` / `Executor` / `Acquire` (discovered from the repository crate on every run), `tx.commit()` or `tx.rollback()`. So `&mut *tx`, `tx.execute(..)`, or handing the tx to a local helper fails. The api's handler transactions (`admin_feedback.rs:175`, `clusters.rs:197/374/448`, `moderation.rs:126`, `promote.rs:267`, `work_orders.rs:467`) all have the allowed shape: `&mut tx` into `*_in_executor` repository methods, then commit/rollback. |
| A6 no re-export | the repository crate `pub use`-ing anything from sqlx (would launder the query API past A1) |

**Probe B — repository scope discipline.** Every public fn in `crates/feedbackmonk-repository/src/**` (pub-trait methods, `impl Trait for` methods, inherent/free `pub [const|async|unsafe] fn`) must take `&TenantScope` / `&ProjectScope` as its first non-self argument, or be listed in `allowlist.toml`. Generic fns such as `claim_idempotency_key<'t>(` (`feedback.rs:1021`) are parsed — the old `fn\s+(\w+)\s*\(` skipped every fn with a generic list. The allowlist is parsed as TOML and must be clean: every entry has a rationale, no key repeats, and no key is stale (names a method that no longer exists).

A missing `crates/` or repository crate, or an unreadable source file, is a **FAIL** — never a vacuous PASS. Probe A has no allowlist by design.

## Usage

```bash
python .claude/project-oracles/multi-tenant-isolation-check/oracle.py            # this checkout
python .claude/project-oracles/multi-tenant-isolation-check/oracle.py --root DIR # a tree with the same layout (self-test)
```

Exit 0 PASS, 1 FAIL (offenders with `file:line`), 2 environment error (bad `--root`). `--full` is accepted for suite uniformity; the oracle is static-only. About 0.5 s CPU on the current tree (~2.2 MB of Rust); wall time adds interpreter start-up.

## File Index

| File | Purpose |
|---|---|
| `oracle.json` | Oracle metadata: triggers, freshness, consumer scope. |
| `allowlist.toml` | Probe B exemptions (12 trait methods, 27 inherent methods), each with a rationale. |
| `oracle.py` | Canonical implementation. |

## Constraints & Business Rules

- **Probe A has NO allowlist.** DEC-FBR-03 declares any raw SQL outside `crates/feedbackmonk-repository/` a security incident. Widening the A1 whitelist needs a written DEC-FBR-* amendment; every item on it today is inert (a pool handle, its builder, the error type, the test attribute).
- **Probe B allowlist entries need a rationale**, enforced by the oracle.
- **CI gate.** Run by `scripts/run-verification-oracles.sh` (CI job `verification-oracles`, and `scripts/ci-local.sh`).

## Relationships & Dependencies

- **Type-system half (leg 1)**: `crates/feedbackmonk-repository/src/scope.rs` — `TenantScope` / `ProjectScope` with `pub(crate)` constructors.
- **Lint half (leg 3)**: `Cargo.toml` workspace `clippy::all = deny` + per-crate `clippy::pedantic`, plus `cargo-deny` checks in `deny.toml`.
- **Consumed by**: every commit in P0+; CI workflow at `.github/workflows/ci.yml`; Stage 2 Workers A and B (gates their commits); future P1+ code (gates indefinitely).

## Decision Log

### Three-leg defense: type system + AST oracle + clippy

**Decision**: Tenant isolation is enforced by three independent mechanisms — `TenantScope`/`ProjectScope` newtypes at the type-system layer, this oracle at the AST layer, and clippy/cargo-deny at the static-analysis layer.

**Rationale**: Q2=5 (silent fidelity risk) on FR-FBR-01: a passing unit test does NOT prove cross-tenant isolation under all future query paths. One mechanism is brittle; two is fragile; three is resilient — the legs are independent (a bug that defeats the type system likely doesn't defeat AST grep, and vice versa). This is the canonical Probandurgy pattern for high-Q2 surfaces.

**Trade-offs**: Three legs to maintain. The maintenance cost is bounded — the oracle is `<300` lines of Python, the newtypes are `~60` lines of Rust, and clippy/cargo-deny are config-only. The cost of one undetected cross-tenant leak (months in the wild, customer trust damage, GDPR exposure) dwarfs the maintenance.

**Implementation**: All three legs live in this repo and are exercised on every commit. See `crates/feedbackmonk-repository/README.md` for the leg-1 details and `Cargo.toml` + `deny.toml` for leg-3.

### Canonical implementation in Python, not pure shell

**Decision**: `oracle.py` is the only implementation. It once had `oracle.ps1`/`oracle.sh` shims that only delegated to it; they were removed on 2026-09-30, and every consumer invokes `python .../oracle.py`.

### Allowlist entries require inline rationale

**Decision**: Every entry in `allowlist.toml` must carry an inline `rationale = "..."` field documenting WHY the method deviates from the first-arg-scope discipline.

**Rationale**: Allowlists drift. Without per-entry rationale, future maintainers cannot tell whether an entry is load-bearing (e.g. `TenantRepo::create` — pre-auth signup, no scope exists yet) or vestigial (e.g. an entry added during debugging and never removed). Forcing the WHY at entry time pays back at audit time. This is the same principle as `cargo deny` advisories carrying explanations.

**Trade-offs**: Adding an allowlist entry is slightly more work. By design — the friction is the feature.

**Implementation**: `allowlist.toml` schema: `[[methods]]` or `[[inherent_methods]]` blocks, each with `trait`/`type_name`, `method`, and `rationale` fields. Oracle code does not judge the rationale's content (that's a human review job); it FAILS an entry whose rationale is missing or empty, a repeated key, and a stale key.

### Freshness contract triggers on allowlist changes

**Decision**: `allowlist.toml` is listed in `oracle.json` `freshness.triggers` — editing it invalidates the oracle and forces re-run.

**Rationale**: An allowlist edit is precisely the kind of action that should re-trigger the oracle, because it changes the rules. If allowlist edits did NOT invalidate, a developer could add an over-broad entry and commit while the oracle's cached result still showed green from before the edit — defeating the audit trail.

**Trade-offs**: Slightly more frequent oracle invocations. Cost is ~0.5 s CPU per run; negligible.

**Implementation**: `oracle.json` `freshness.triggers` line 24 includes the allowlist path.

## Adversarial self-test

Run 2026-09-30 against a copy of `crates/` (no `target/`) under the session scratchpad, via `--root`. Each mutation was applied alone, the oracle run, the file restored. "old" is the pre-hardening oracle (HEAD `4f1b88d`) on the same mutated tree. After every restore the new oracle returned exit 0.

| # | Mutation (file) | new | old | Caught by |
|---|---|---|---|---|
| M1 | `let _q = sqlx::raw_sql("DELETE FROM feedback");` after the `begin()` in `handlers/admin_feedback.rs` | 1 | 0 | A1 |
| M2 | `use sqlx::{query, Executor};` + `query("DELETE FROM feedback")` (admin_feedback.rs) | 1 | 0 | A1 (both leaves) + A2 |
| M3 | `ex.execute("DELETE FROM feedback")` on a fn parameter (admin_feedback.rs) | 1 | 0 | A3 |
| M4 | `(&mut *tx).execute(q).await?;` on the handler transaction | 1 | 0 | A5 |
| M5 | `&mut tx` → `&mut *tx` in the `append_in_executor` call (admin_feedback.rs:200) | 1 | 0 | A5 |
| M6 | `local_helper(&mut tx).await?;` | 1 | 0 | A5 (not a repository executor method) |
| M7 | `state.pool.begin().await?.commit().await?;` (unbound) | 1 | 0 | A5 |
| M8 | `state.pool.fetch_all(q).await?` | 1 | 0 | A4 |
| M9 | `let _u = "http://x"; let _q = sqlx::query("…");` on one line | 1 | 0 | A1 (old per-line `//` strip ate the call) |
| M10 | `async fn leak_all<'t>(&self, tenant_id: uuid::Uuid)` added to `pub trait FeedbackRepo` | 1 | 0 | Probe B (generic fn) |
| M11 | `pub use sqlx::query as raw;` in repository `lib.rs` | 1 | 0 | A6 |
| M12 | `use sqlx::Executor as _;` alone | 1 | 0 | A1 |
| M13 | `query_as::<_, (i64,)>("SELECT 1")` in a file already importing `sqlx::PgPool` | 1 | 0 | A2 (turbofish) — first cut of A2 missed this; regex fixed |
| M14 | `query!("DELETE FROM feedback")` in `feedbackmonk-core/src/lib.rs` (no sqlx mention) | 1 | 0 | A2 (macro anywhere) |
| M15 | `repo_like.fetch_one(&*format!("SELECT {}", 1))` | 1 | 0 | A3 |
| M16 | rename allowlisted `TrendBucket::parse` → `parse_v2` (repository `feedback.rs`) | 1 | 1 | Probe B + stale-allowlist check (`TrendBucket::parse` names no fn) |
| — | `--root` at an empty directory | 1 | n/a | missing crates/ and repository crate are FAILs |

The harness is not kept in the tree; re-create it by copying `crates/` elsewhere, applying one row, and running `oracle.py --root <copy>`.
