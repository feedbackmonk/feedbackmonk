# feedbackmonk-api

## Synopsis

The HTTP surface of feedbackmonk (FR-FBR-02..18) — axum router + handlers mounted on the `feedbackmonk-repository` trait surface. Holds the full endpoint tree: signup/verify-email, projects, signing-keys, public feedback submission, end-user "my feedback", attachments, admin feedback triage, tier status, widget-config, public+admin roadmap, promote-to-roadmap, and health. Binds `FEEDBACKMONK_PORT` (default 14304). Open the File Index below to find the handler for an endpoint.

## Purpose & Responsibilities

`feedbackmonk-api` holds the axum router, request/response shapes (via `feedbackmonk-core` records), and the HTTP handlers that mount on top of the `feedbackmonk-repository` trait surface. It is built as both a library (exposing the composed router + `AppState` so integration tests wire the same router the binary uses) and a binary (`main.rs`). The full router tree spanning P1–P4 is now in place; the original Stage-1 placeholder note is preserved in the Decision Log below for historical context.

## File Index

### Top-level (`src/`)

| File | Purpose |
|---|---|
| `src/lib.rs` | Crate root. Declares the modules (`auth`, `cors`, `crash_correlation`, `email`, `error`, `handlers`, `roadmap_voting_cache`, `router`, `state`, `storage`) and exposes the composed router + `AppState` so integration tests wire the same router the binary uses. |
| `src/main.rs` | Binary entrypoint. Loads env, connects Postgres, builds repo handles + env-selected mailer, constructs `AppState`, composes the full router tree (applying the public-endpoint CORS layer from `FEEDBACKMONK_CORS_ORIGINS`), binds `FEEDBACKMONK_PORT` (default `14304`) and serves. |
| `src/cors.rs` | Credentialed CORS policy for the public widget endpoints (submission + attachments). `parse_origins` + `public_cors_layer` build a `tower_http` `CorsLayer` from the `FEEDBACKMONK_CORS_ORIGINS` allowlist — echo-origin (never `*`) + `allow_credentials` so the anonymous `credentials: include` path works. Implements DEC-FBR-04's domain allowlist; see DEC-FBR-IMPL-09. |
| `src/router.rs` | Composes the Worker A subtree (signup, verify-email, projects, signing-keys). Other handler modules expose their own routers that `main.rs` merges into the single binary `Router`. |
| `src/state.rs` | `AppState` — the shared application context cloned into every handler via axum's `State` extractor. Holds the pool, `Arc<dyn _Repo>` handles (swappable for test fakes), env-selected mailer, session secret, and the voting cache. |
| `src/error.rs` | `ApiError` — the single HTTP error type handlers return. Maps repository / validation / auth failures to status codes and implements `IntoResponse` so handlers can `?` freely. |
| `src/auth/` | Admin authentication submodule — argon2id password hashing + signed-cookie admin session. See `auth/README.md`. |
| `src/email/` | Outbound email submodule — `Mailer` trait + Mailpit (dev) / SMTP-env (prod) impls + template renderers. See `email/README.md`. |
| `src/handlers/` | HTTP handler families — see `handlers/README.md` and the sub-table below. |
| `src/roadmap_voting_cache.rs` | 60-second in-process per-project voting-cache aggregator (Contract C15). Held in `AppState`; refreshed by a background tokio task; consumed by the public-roadmap top-voted endpoint. Pattern adapted from `gitcellar-cloud/src/feedback/roadmap_voting.rs`. |
| `src/crash_correlation.rs` | **GitCellar parity gap #2.** Best-effort pull-mode crash-event correlation worker. Runs OFF the submit hot path: a Glitchtip outage degrades correlation to null, it never fails a submission. Resolves `feedback.crash_event_id` (migration `00010`) to crash detail for the Desktop crash-link banner. |
| `src/storage.rs` | **GitCellar parity gap #1.** Attachment storage abstraction: `LocalFsStorage` (dev/self-host default) + `S3Storage` (hand-rolled SigV4, S3/MinIO) behind one trait, env-selected via `from_env`. Consumed by `handlers/attachments.rs`. |
| `Cargo.toml` | Depends on `axum`, `tokio`, `tracing`, `feedbackmonk-core`, `feedbackmonk-repository`, `feedbackmonk-jwt`, `feedbackmonk-anon`, multipart + S3/SigV4 deps. |

### Handlers (`src/handlers/`)

| File | Purpose |
|---|---|
| `mod.rs` | Module declarations for the handler families (Worker A onboarding + Worker B public/admin endpoints). |
| `health.rs` | `GET /health` (always 200; liveness + DB ping + version + uptime, `status` flips to `"degraded"` on DB failure) and `GET /health/ready` (200 healthy / 503 otherwise). FR-FBR-18, Contract C5. |
| `signup.rs` | `POST /api/v1/signup` — tenant signup: validate, argon2id-hash, create pending-verification tenant (409 on conflict), mint + store verify token, send verify email, return 202. FR-FBR-02. |
| `verify_email.rs` | `POST /api/v1/verify-email` — redeem a verify token: first redemption marks used + sets `verified_at` + mints session cookie; replay-window second hit re-mints; expired or post-window → 410 Gone. FR-FBR-02. |
| `projects.rs` | `POST /api/v1/projects` (create) + `GET /api/v1/projects` (list). Admin-session-gated; the session's `TenantScope` enforces the tenant boundary at the type level. |
| `signing_keys.rs` | `POST` (register) + `DELETE` (mark inactive) under `…/projects/{id}/signing-keys`. Contract C4: customer registers only the PUBLIC Ed25519 key (32 raw bytes, base64-encoded). DEC-FBR-04. |
| `feedback.rs` | `POST /api/v1/projects/{id}/feedback` — public submission endpoint. Auth-mode dispatch on the `Authorization` header (JWT-verified authenticated vs. anonymous rate-limited). FR-FBR-03/05/06, Contract C3. |
| `me_feedback.rs` | `GET …/me/feedback` + `GET …/me/feedback/{fb}/thread` — end-user (JWT-`sub`-scoped) read surface (GitCellar parity gap #4). No schema change; reads existing `feedback.end_user_sub` + public-visibility replies. |
| `attachments.rs` | `POST …/feedback/{id}/attachments` — multipart attachment upload (Gap #1). `files[]` image parts (≤4, ≤5 MB, png/jpeg/webp, magic-byte sniffed) + optional `service_log`/`console_log` text parts. Stores via `storage.rs`. |
| `admin_feedback.rs` | Admin feedback endpoints (Contracts C7 + C8) behind `AdminSession`: status `transition`, `reply`, list, and detail — all inside the session's resolved `TenantScope` (DEC-FBR-03). |
| `admin_tier.rs` | `GET /api/v1/admin/tier` — read-only tier-status endpoint: current tier + static Contract-C19 quotas + live usage (projects count + rolling-30d feedback count + `period_start`). P3, FR-FBR-14, Contract C17. |
| `widget_config.rs` | `GET …/projects/{id}/widget-config` — public (no-auth) endpoint returning the widget's runtime config; `project_id` is the widget's public key. Contract C12. |
| `roadmap.rs` | Public + admin roadmap handlers (8 endpoints split across two routers). Public roadmap + voting + admin item management. Contract C15, FR-FBR-11/13. |
| `promote.rs` | Admin one-shot promote-to-roadmap action — adds a `roadmap_items` row, atomically transitions the source feedback to `Duplicate` (`transition_reason = "promoted to roadmap"`); idempotent on `roadmap_items.origin_feedback_id` UNIQUE. FR-FBR-12, Contract C16. |

## Public API & Usage

Stage 1 surface is intentionally minimal — see `src/lib.rs`. The real surface lands in Stage 2:

- **Worker A** (FR-FBR-02): `/api/v1/signup`, `/api/v1/login`, `/api/v1/projects/...`
- **Worker B** (FR-FBR-03 + FR-FBR-05 + FR-FBR-06): `/api/v1/projects/{project_id}/feedback` (POST), JWT verifier middleware, anonymous-mode rate-limiter

Local dev port: **14304** (`strictPort: true` will be enforced at Stage 2 when the real binary lands). See `docs/operations/LOCAL_DEV.md` for Postgres-container setup and env vars.

## Constraints & Business Rules

- **NO raw SQL.** Every DB touch goes through `feedbackmonk-repository`. The `multi-tenant-isolation-check` oracle's triggers include this crate; a `sqlx::query(...)` here is a security incident per DEC-FBR-03.
- **Port 14304 is reserved** in `~/.claude/MACHINE_CONFIG.md` Dev Port Registry. Stage 2's `vite.config.ts` (admin UI, P1) must set `strictPort: true` against the same registry.
- **JWT customer signs is the ONLY identity** feedbackmonk ever has for an end-user (DEC-FBR-04). No callbacks to customer auth providers; no long-lived bearer tokens.

## Relationships & Dependencies

- **Consumes**: `feedbackmonk-repository` (every DB touch), `feedbackmonk-core` (request/response shapes).
- **Will consume (Stage 2)**: `feedbackmonk-jwt` (Worker B's JWT verifier crate), `feedbackmonk-anon` (Worker B's anonymous-mode rate-limiter crate).
- **Consumed by**: nobody yet (binary crate + future admin-UI HTTP client).

## Decision Log

### Placeholder binary, real router lands in Stage 2

**Decision**: Stage 1's `main.rs` is intentionally a placeholder — it binds the right port and serves a banner, nothing more. The real router tree is the joint output of Stage 2 Workers A and B.

**Rationale**: Stage 1's scope is FR-FBR-01 (the data model + tenant-scoped repository layer) plus Task Zero (the oracle). Stubbing the binary here lets the workspace build, lets `cargo run` produce a real bound port for sanity-check, and avoids inventing a router shape that Workers A and B should design together at Stage 2 plan time.

**Trade-offs**: A future Stage 2 worker who runs `cargo run -p feedbackmonk-api` and sees the banner might be confused for a moment. Mitigated by the banner text explicitly stating "stage1 placeholder."

**Implementation**: `src/main.rs` 20 lines; one axum route at `/` returning a static string.
