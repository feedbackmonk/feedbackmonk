# feedbackr-core

<!-- agent-synopsis -->
Pure domain types (no DB, no async, no network) shared across Feedbackr crates. Built in P0 Stage 1 per FR-FBR-01 and P0 plan Contract C1.
<!-- /agent-synopsis -->

## Purpose & Responsibilities

`feedbackr-core` is the **domain vocabulary** of Feedbackr. It holds:

- ID newtypes (`FeedbackId`, `SigningKeyId`) and their generators
- Plain data records that mirror the P0 schema (`Tenant`, `Project`, `SigningKey`, `Feedback`, `AnonSubmission`, `RateLimitCounter`)
- The `FeedbackKind` enum that backs the `feedback.kind` schema column + CHECK constraint

It deliberately holds **no** DB access, **no** async code, and **no** network I/O. The DB-touching layer is `feedbackr-repository`; the HTTP layer is `feedbackr-api`.

## File Index

| File | Purpose |
|---|---|
| `src/lib.rs` | Crate root. Re-exports the public type set. `#![deny(unsafe_code)]`. |
| `src/ids.rs` | `FeedbackId` (FB-1234-style display ID generator) and `SigningKeyId`. Includes 3 unit tests covering format invariants and uniqueness. |
| `src/models.rs` | Plain `struct` / `enum` records mirroring the P0 schema. Includes 3 unit tests over `FeedbackKind` parse round-trips and `Feedback` field defaults. |
| `Cargo.toml` | Depends on `uuid`, `chrono`, `serde`, `serde_json`. No DB / HTTP crates. |

## Public API & Usage

```rust
use feedbackr_core::{FeedbackId, FeedbackKind, Tenant, Project};

// Generate a new feedback display ID (FB-####)
let id = FeedbackId::generate();

// Parse a feedback-kind string (used by the submission API)
let kind: FeedbackKind = "bug".parse()?;
```

Consumers: `feedbackr-repository` (records returned from queries), `feedbackr-api` (request/response shapes — Stage 2+).

## Constraints & Business Rules

- **No async, no DB, no network.** This crate must remain pure-data; introducing `sqlx`, `tokio`, `reqwest`, or `axum` here is a layering violation.
- **All types must be `Serialize` + `Deserialize`** (they cross the HTTP boundary in `feedbackr-api`).
- **`FeedbackKind` variants are load-bearing** — they back the schema's `CHECK (kind IN (...))` constraint. Adding a variant requires a migration AND a coordinated update to `migrations/`.
- **`#![deny(unsafe_code)]`** — pure data has no excuse for `unsafe`.

## Relationships & Dependencies

- **Consumed by**: `feedbackr-repository` (every query result type), `feedbackr-api` (every request/response shape), Stage 2 workers (forward-looking).
- **Depends on**: standard ecosystem crates only (`uuid`, `chrono`, `serde`, `serde_json`).
- **Schema source of truth**: `migrations/00001_p0_schema.sql` is the authoritative reference; record fields here must reflect column names there.

## Decision Log

### Pure-data crate, no DB or async

**Decision**: Keep `feedbackr-core` free of `sqlx`, `tokio`, `axum`, and any I/O surface.

**Rationale**: Three crates (`-core`, `-repository`, `-api`) implement a layered architecture where the boundary between *what the domain looks like* and *how it gets fetched* is enforceable at the crate-dependency level. The `multi-tenant-isolation-check` Verification Oracle relies on `sqlx::*` patterns appearing **only** in `feedbackr-repository`; a `sqlx` dependency leaking into `feedbackr-core` would create a false-positive surface that the oracle's allowlist would have to grow to accommodate. Layer discipline at crate granularity is cheaper than layer discipline within a single crate.

**Trade-offs**: Forces a slightly heavier mapping layer in `feedbackr-repository` (sqlx rows → core records). The mapping cost is sub-microsecond and bounded; the architectural clarity it preserves is permanent.

**Implementation**: `Cargo.toml` workspace-dependencies declaration; CI clippy + cargo-deny enforce no sqlx/tokio/axum imports in this crate.
