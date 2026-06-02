# feedbackmonk-core

## Synopsis

Pure domain vocabulary of feedbackmonk — ID newtypes, plain data records mirroring the P0 schema (`Tenant`, `Project`, `SigningKey`, `Feedback`, `AnonSubmission`, `RateLimitCounter`), and the `Tier` enum + quota table. No DB, no async, no network — the bottom layer every other crate depends on. Open it to find a domain type's shape or the canonical tier quotas. FR-FBR-01, Contract C1.

## Purpose & Responsibilities

`feedbackmonk-core` is the **domain vocabulary** of feedbackmonk. It holds:

- ID newtypes (`FeedbackId`, `SigningKeyId`) and their generators
- Plain data records that mirror the P0 schema (`Tenant`, `Project`, `SigningKey`, `Feedback`, `AnonSubmission`, `RateLimitCounter`)
- The `FeedbackKind` enum that backs the `feedback.kind` schema column + CHECK constraint

It deliberately holds **no** DB access, **no** async code, and **no** network I/O. The DB-touching layer is `feedbackmonk-repository`; the HTTP layer is `feedbackmonk-api`.

## File Index

| File | Purpose |
|---|---|
| `src/lib.rs` | Crate root. Re-exports the public type set. `#![deny(unsafe_code)]`. |
| `src/ids.rs` | `FeedbackId` (FB-1234-style display ID generator) and `SigningKeyId`. Includes 3 unit tests covering format invariants and uniqueness. |
| `src/models.rs` | Plain `struct` / `enum` records mirroring the P0 schema. Includes 3 unit tests over `FeedbackKind` parse round-trips and `Feedback` field defaults. |
| `src/roadmap.rs` | P2: `RoadmapItem`, `RoadmapStatus`, public-roadmap value types (Contract C13). |
| `src/status.rs` | P1: `FeedbackStatus` enum + transitions (Contract C6). |
| `src/tier.rs` | **P3 Stage 1**: `Tier` enum (`Free | Starter | Pro | SelfHost`), `ResourceKind`, `TierQuotas` config struct, and the `tier_quotas(tier) -> TierQuotas` const fn that is the **single source of truth** for per-tier caps + capability flags + footer copy. `as_db_str` / `from_db_str` are the canonical conversion path used by `feedbackmonk-repository`. 11 unit tests cover round-trip, default, exhaustive matching, and `tier_quotas()` shape per Contract C19. |
| `Cargo.toml` | Depends on `uuid`, `chrono`, `serde`, `serde_json`. No DB / HTTP crates. |

## Public API & Usage

```rust
use feedbackmonk_core::{FeedbackId, FeedbackKind, Tenant, Project};

// Generate a new feedback display ID (FB-####)
let id = FeedbackId::generate();

// Parse a feedback-kind string (used by the submission API)
let kind: FeedbackKind = "bug".parse()?;
```

Consumers: `feedbackmonk-repository` (records returned from queries), `feedbackmonk-api` (request/response shapes — Stage 2+).

## Constraints & Business Rules

- **No async, no DB, no network.** This crate must remain pure-data; introducing `sqlx`, `tokio`, `reqwest`, or `axum` here is a layering violation.
- **All types must be `Serialize` + `Deserialize`** (they cross the HTTP boundary in `feedbackmonk-api`).
- **`FeedbackKind` variants are load-bearing** — they back the schema's `CHECK (kind IN (...))` constraint. Adding a variant requires a migration AND a coordinated update to `migrations/`.
- **`#![deny(unsafe_code)]`** — pure data has no excuse for `unsafe`.

## Relationships & Dependencies

- **Consumed by**: `feedbackmonk-repository` (every query result type), `feedbackmonk-api` (every request/response shape), Stage 2 workers (forward-looking).
- **Depends on**: standard ecosystem crates only (`uuid`, `chrono`, `serde`, `serde_json`).
- **Schema source of truth**: `migrations/00001_p0_schema.sql` is the authoritative reference; record fields here must reflect column names there.

## Decision Log

### Pure-data crate, no DB or async

**Decision**: Keep `feedbackmonk-core` free of `sqlx`, `tokio`, `axum`, and any I/O surface.

**Rationale**: Three crates (`-core`, `-repository`, `-api`) implement a layered architecture where the boundary between *what the domain looks like* and *how it gets fetched* is enforceable at the crate-dependency level. The `multi-tenant-isolation-check` Verification Oracle relies on `sqlx::*` patterns appearing **only** in `feedbackmonk-repository`; a `sqlx` dependency leaking into `feedbackmonk-core` would create a false-positive surface that the oracle's allowlist would have to grow to accommodate. Layer discipline at crate granularity is cheaper than layer discipline within a single crate.

**Trade-offs**: Forces a slightly heavier mapping layer in `feedbackmonk-repository` (sqlx rows → core records). The mapping cost is sub-microsecond and bounded; the architectural clarity it preserves is permanent.

**Implementation**: `Cargo.toml` workspace-dependencies declaration; CI clippy + cargo-deny enforce no sqlx/tokio/axum imports in this crate.

### `Tier` enum is the single source of truth for the four-tier matrix (P3 Stage 1)

**Decision**: `Tier::{Free, Starter, Pro, SelfHost}` is **the** representation of pricing-tier identity across feedbackmonk. `tier_quotas(tier) -> TierQuotas` is the single source of truth for per-tier caps + capability flags + footer copy (Contract C19). The DB column `tenants.tier TEXT` is funneled through `Tier::from_db_str` (strict — unknown values error rather than silently defaulting to Free).

**Rationale**: A pricing tier is a **product invariant**, not a configuration value. Distributing the four-tier knowledge across handler match-arms, fixture seeds, or env-var registries would let drift compound: a code path that quietly assumed three tiers, a config file that knew about a fifth tier that no Rust code handled, a fallback-to-Free that masked a corrupted Pro row at security cost (the unexpectedly-Free Pro tenant suddenly hits caps). One enum + one `tier_quotas()` function makes drift a compile-time error.

**Trade-offs**: Adding a tier requires touching three files coordinated in one commit (the enum, the `tier_quotas()` table, and migration `00008`'s CHECK constraint). The Verification Oracle Probe B catches Contract-C19 drift if any of the three lags. Trade is favorable: tier additions are low-frequency and high-stakes; making them mandate one atomic commit is correct.

**Implementation**: `src/tier.rs` + re-exports from `src/lib.rs` (`pub use tier::*;`). Triple-layer defense — direct DB writes cannot smuggle invalid values past the schema CHECK (`migrations/00008_tenant_tier_check.sql`); Rust code cannot read invalid values past the `Tier::from_db_str` codec; programmatic API consumers see a closed enum at the type level. The codec error type `TierParseError` is exposed so the repository layer propagates cleanly. Probandurgy lineage: `tier-enforcement-status` Probe B (Contract C19 shape) + repository allowlist entry for `SqlxTierQuotaRepo::new`.
