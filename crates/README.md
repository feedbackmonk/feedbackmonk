# crates

<!-- agent-synopsis -->
Feedbackr's Rust workspace crates. Three crates ship at P0 Stage 1 (`-core`, `-repository`, `-api`); two more land in Stage 2 (`-jwt`, `-anon`).
<!-- /agent-synopsis -->

## Purpose

This directory holds every Rust crate in the Feedbackr workspace. The layered architecture (data → DB → HTTP) is enforced at the crate-dependency level — `feedbackr-core` knows nothing about DB; `feedbackr-repository` is the SOLE query path (DEC-FBR-03); `feedbackr-api` is the HTTP-surface frontier.

## Layout

| Crate | Layer | Purpose | Shipped at |
|---|---|---|---|
| **`feedbackr-core`** | data | Pure domain types (no DB, no async, no network) | P0 Stage 1 |
| **`feedbackr-repository`** | DB | Tenant-scoped repository layer; sole query path | P0 Stage 1 |
| **`feedbackr-api`** | HTTP | axum router + handlers (placeholder at Stage 1) | P0 Stage 1 (placeholder) / Stage 2 (real surface) |
| **`feedbackr-jwt`** | auth | JWT EdDSA verifier (FR-FBR-05) | P0 Stage 2 (Worker B Task Zero) |
| **`feedbackr-anon`** | rate-limit | Anonymous-mode rate-limiter (FR-FBR-06) | P0 Stage 2 (Worker B) |

## Dependency direction

```
            feedbackr-core
                  ▲
                  │
       ┌──────────┴──────────┐
       │                     │
feedbackr-repository   feedbackr-api  ──► feedbackr-jwt, feedbackr-anon (Stage 2)
       ▲                     │
       └─────────────────────┘
                  (api depends on repository)
```

`feedbackr-core` has no internal dependencies. `feedbackr-repository` depends only on `feedbackr-core`. `feedbackr-api` depends on both. Inverting any of these arrows is a layering violation.

## Constraints

- **Raw SQL is allowed ONLY in `feedbackr-repository`.** Enforced by `.claude/oracles/multi-tenant-isolation-check/`. Adding a `sqlx::query(...)` to any other crate is a security incident per DEC-FBR-03.
- **`feedbackr-core` stays pure-data.** No async, no I/O, no DB crates. The layering enforcement at crate granularity is what makes the multi-tenant-isolation oracle's grep patterns simple and the layering provable.
- **Workspace clippy baseline**: `clippy::all = deny`. `feedbackr-repository` additionally runs `clippy::pedantic`.

## Decision Log

### Three-crate split at P0, not one or two

**Decision**: Ship three crates at Stage 1 (`-core`, `-repository`, `-api`) rather than collapsing the boundary into a single crate or two.

**Rationale**: The `multi-tenant-isolation-check` oracle's grep patterns are *much* simpler when "raw SQL is allowed here / forbidden elsewhere" is enforceable at the file-path level (`crates/feedbackr-repository/**` vs everywhere else). Inside a single crate, an oracle would have to do AST-grade module-tree analysis to distinguish "repository module" from "handler module." File-path enforcement is cheap, robust, and unambiguous. The same argument applies to `-core` being separate: the pure-data crate cannot accidentally pick up a `sqlx` dependency because Cargo's transitive-dependency rules forbid it.

**Trade-offs**: Three Cargo.toml files instead of one. Marginal maintenance cost.

**Implementation**: Workspace `Cargo.toml` lists three members; each crate has `[lints.clippy]` configuration appropriate to its layer.
