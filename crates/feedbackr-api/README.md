# feedbackr-api

<!-- agent-synopsis -->
HTTP surface of Feedbackr. Stage 1 ships a placeholder binary binding `FEEDBACKR_PORT` (default 14304); Stage 2 Workers A and B add the real router tree.
<!-- /agent-synopsis -->

## Purpose & Responsibilities

`feedbackr-api` will hold the axum router, request/response shapes (via `feedbackr-core` records), and the HTTP handlers that mount on top of the `feedbackr-repository` trait surface. In Stage 1 it is a **placeholder**: a single-route axum app and a library function used to demonstrate the workspace links together cleanly.

## File Index

| File | Purpose |
|---|---|
| `src/lib.rs` | Crate root. Currently exports only `placeholder() -> &'static str` so the workspace links and downstream consumers can probe build health. |
| `src/main.rs` | Stage 1 binary. Reads `FEEDBACKR_PORT` (default `14304`), binds axum on `127.0.0.1`, serves a banner route. |
| `Cargo.toml` | Depends on `axum`, `tokio`, `tracing`, `feedbackr-core`, `feedbackr-repository`. |

## Public API & Usage

Stage 1 surface is intentionally minimal — see `src/lib.rs`. The real surface lands in Stage 2:

- **Worker A** (FR-FBR-02): `/api/v1/signup`, `/api/v1/login`, `/api/v1/projects/...`
- **Worker B** (FR-FBR-03 + FR-FBR-05 + FR-FBR-06): `/api/v1/projects/{project_id}/feedback` (POST), JWT verifier middleware, anonymous-mode rate-limiter

Local dev port: **14304** (`strictPort: true` will be enforced at Stage 2 when the real binary lands). See `docs/operations/LOCAL_DEV.md` for Postgres-container setup and env vars.

## Constraints & Business Rules

- **NO raw SQL.** Every DB touch goes through `feedbackr-repository`. The `multi-tenant-isolation-check` oracle's triggers include this crate; a `sqlx::query(...)` here is a security incident per DEC-FBR-03.
- **Port 14304 is reserved** in `~/.claude/MACHINE_CONFIG.md` Dev Port Registry. Stage 2's `vite.config.ts` (admin UI, P1) must set `strictPort: true` against the same registry.
- **JWT customer signs is the ONLY identity** Feedbackr ever has for an end-user (DEC-FBR-04). No callbacks to customer auth providers; no long-lived bearer tokens.

## Relationships & Dependencies

- **Consumes**: `feedbackr-repository` (every DB touch), `feedbackr-core` (request/response shapes).
- **Will consume (Stage 2)**: `feedbackr-jwt` (Worker B's JWT verifier crate), `feedbackr-anon` (Worker B's anonymous-mode rate-limiter crate).
- **Consumed by**: nobody yet (binary crate + future admin-UI HTTP client).

## Decision Log

### Placeholder binary, real router lands in Stage 2

**Decision**: Stage 1's `main.rs` is intentionally a placeholder — it binds the right port and serves a banner, nothing more. The real router tree is the joint output of Stage 2 Workers A and B.

**Rationale**: Stage 1's scope is FR-FBR-01 (the data model + tenant-scoped repository layer) plus Task Zero (the oracle). Stubbing the binary here lets the workspace build, lets `cargo run` produce a real bound port for sanity-check, and avoids inventing a router shape that Workers A and B should design together at Stage 2 plan time.

**Trade-offs**: A future Stage 2 worker who runs `cargo run -p feedbackr-api` and sees the banner might be confused for a moment. Mitigated by the banner text explicitly stating "stage1 placeholder."

**Implementation**: `src/main.rs` 20 lines; one axum route at `/` returning a static string.
