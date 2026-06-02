# crates

## Synopsis

feedbackmonk's Rust workspace crates, layered data → DB → HTTP and enforced at the crate-dependency level: `feedbackmonk-core` (pure domain types), `feedbackmonk-repository` (the SOLE query path, DEC-FBR-03), `feedbackmonk-api` (HTTP surface), plus `feedbackmonk-jwt` (EdDSA verifier), `feedbackmonk-anon` (anonymous-mode rate limiter), and `feedbackmonk-tracing` (PII-scrubbing log chokepoint). Open this directory's `Layout` table to pick the right crate.

## Purpose

This directory holds every Rust crate in the feedbackmonk workspace. The layered architecture (data → DB → HTTP) is enforced at the crate-dependency level — `feedbackmonk-core` knows nothing about DB; `feedbackmonk-repository` is the SOLE query path (DEC-FBR-03); `feedbackmonk-api` is the HTTP-surface frontier.

## Layout

| Crate | Layer | Purpose | Shipped at |
|---|---|---|---|
| **`feedbackmonk-core`** | data | Pure domain types (no DB, no async, no network) | P0 Stage 1 |
| **`feedbackmonk-repository`** | DB | Tenant-scoped repository layer; sole query path | P0 Stage 1 |
| **`feedbackmonk-api`** | HTTP | axum router + handlers (placeholder at Stage 1) | P0 Stage 1 (placeholder) / Stage 2 (real surface) |
| **`feedbackmonk-jwt`** | auth | JWT EdDSA verifier (FR-FBR-05) | P0 Stage 2 (Worker B Task Zero) |
| **`feedbackmonk-anon`** | rate-limit | Anonymous-mode rate-limiter (FR-FBR-06) | P0 Stage 2 (Worker B) |

## Dependency direction

```
            feedbackmonk-core
                  ▲
                  │
       ┌──────────┴──────────┐
       │                     │
feedbackmonk-repository   feedbackmonk-api  ──► feedbackmonk-jwt, feedbackmonk-anon (Stage 2)
       ▲                     │
       └─────────────────────┘
                  (api depends on repository)
```

`feedbackmonk-core` has no internal dependencies. `feedbackmonk-repository` depends only on `feedbackmonk-core`. `feedbackmonk-api` depends on both. Inverting any of these arrows is a layering violation.

## Constraints

- **Raw SQL is allowed ONLY in `feedbackmonk-repository`.** Enforced by `.claude/oracles/multi-tenant-isolation-check/`. Adding a `sqlx::query(...)` to any other crate is a security incident per DEC-FBR-03.
- **`feedbackmonk-core` stays pure-data.** No async, no I/O, no DB crates. The layering enforcement at crate granularity is what makes the multi-tenant-isolation oracle's grep patterns simple and the layering provable.
- **Workspace clippy baseline**: `clippy::all = deny`. `feedbackmonk-repository` additionally runs `clippy::pedantic`.

## Decision Log

### Three-crate split at P0, not one or two

**Decision**: Ship three crates at Stage 1 (`-core`, `-repository`, `-api`) rather than collapsing the boundary into a single crate or two.

**Rationale**: The `multi-tenant-isolation-check` oracle's grep patterns are *much* simpler when "raw SQL is allowed here / forbidden elsewhere" is enforceable at the file-path level (`crates/feedbackmonk-repository/**` vs everywhere else). Inside a single crate, an oracle would have to do AST-grade module-tree analysis to distinguish "repository module" from "handler module." File-path enforcement is cheap, robust, and unambiguous. The same argument applies to `-core` being separate: the pure-data crate cannot accidentally pick up a `sqlx` dependency because Cargo's transitive-dependency rules forbid it.

**Trade-offs**: Three Cargo.toml files instead of one. Marginal maintenance cost.

**Implementation**: Workspace `Cargo.toml` lists three members; each crate has `[lints.clippy]` configuration appropriate to its layer.
