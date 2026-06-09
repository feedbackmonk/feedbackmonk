<!--
Agent Context Header (ULADP):
- Purpose: HTTP request handlers for the feedbackmonk-api binary. Each file
  is one logical endpoint family; routes are wired by `router.rs` (admin /
  worker-A surface), `feedback.rs::submission_router` (public submission
  surface), and `admin_feedback.rs::routes` (admin feedback surface).
- Owner module: crates/feedbackmonk-api/src/handlers/
- Read first: this README + Contracts C2/C3/C4/C5/C7/C8 in
  docs/planning/handoffs/p1-stage1-to-stage2.md
-->

# handlers/ — HTTP request handlers

## Synopsis

The thin axum-handler layer between HTTP requests and the repository / auth / email crates. One file per logical endpoint family — onboarding (signup, verify-email, projects, signing-keys), public submission (feedback, attachments, widget-config), end-user reads (me_feedback), admin (admin_feedback, admin_tier, roadmap, promote), and health. All DB-touching handlers resolve a `TenantScope`/`ProjectScope` that flows into every repository call (DEC-FBR-03). Open the File Index below to find the handler for an endpoint.

## 1. Purpose & Responsibilities

The thin axum-handler layer between HTTP requests and the repository /
auth / email crates. Each handler file owns one logical endpoint family
and respects three load-bearing patterns:

- **Scope-bound writes (DEC-FBR-03).** Every DB-touching handler
  resolves a `TenantScope` (admin surface) or `ProjectScope` (public
  submission surface, via the allow-listed
  `ProjectRepo::open_for_submission`) before any repository call. The
  `multi-tenant-isolation-check` oracle's Probe B parses these handlers
  for unauthorized scope construction.
- **Errors propagate as `ApiError`.** Handlers `?`-bubble repository
  errors; the `ApiError` -> HTTP-status mapping lives in `error.rs`.
  Exceptions: the submission handler intercepts `JwtError` to emit a
  Contract-C3-compliant 401 JSON body and `RateLimitError` to emit a
  429 with `Retry-After`.
- **Validation at the boundary.** Body / query / path validation runs
  before scope resolution; bad input never reaches the repository.

## 2. File Index

| File | Endpoint(s) | Notes |
|---|---|---|
| `mod.rs` | — | Module surface. `pub mod` for each handler. |
| `signup.rs` | `POST /api/v1/signup` | Tenant signup (FR-FBR-02). 202 + tenant_id; mailer queues verify-email; 409 on duplicate email. |
| `verify_email.rs` | `POST /api/v1/verify-email` | Redeem verify-email token; on success, mark tenant verified + issue `feedbackmonk_session` cookie (Contract C11). |
| `login.rs` | `POST /api/v1/login` | Admin password login (DEC-FBR-IMPL-10). email+password → same `feedbackmonk_session` cookie. Pre-argon2 `LoginGate` throttle (429), enumeration-resistant generic 401 (unknown email = wrong password, with dummy-verify timing equalization), 403 for correct-password-but-unverified. Re-auth path after the verify-email session lapses. |
| `projects.rs` | `POST /api/v1/projects` + `GET /api/v1/projects` | Admin-gated CRUD over tenant's projects; emits the embed-snippet for the widget. |
| `signing_keys.rs` | `POST /api/v1/projects/:id/signing-keys` + `DELETE /api/v1/projects/:id/signing-keys/:key_id` | Ed25519 public-key registration / deactivation (FR-FBR-05, Contract C4). Admin-gated, scope-bound. |
| `feedback.rs` | `POST /api/v1/projects/:id/feedback` | Public submission endpoint (Contract C3). Auth-mode JWT dispatch + anonymous-mode rate-limit + cookie dedup. |
| `admin_feedback.rs` | `GET /api/v1/admin/feedback` + `GET /api/v1/admin/feedback/:id` + `POST /api/v1/admin/feedback/:id/transition` + `POST /api/v1/admin/feedback/:id/reply` + `GET /api/v1/admin/feedback/search` | Admin status-workflow + reply + full-text search endpoints (FR-FBR-07/08, Contracts C7 + C8). Search uses `websearch_to_tsquery` over the `00011`-migration tsvector+GIN index (GitCellar parity gap #3). |
| `me_feedback.rs` | `GET /api/v1/projects/:id/me/feedback` + `GET /api/v1/projects/:id/me/feedback/:fb/thread` | End-user JWT-`sub`-scoped read API (GitCellar parity gap #4). Own-`sub`-only + public-replies-only, enforced at the SQL predicate layer. No schema change. |
| `admin_ops.rs` | `PATCH /api/v1/ops/tenants/:tenant_id` | Operator tier + widget brand-override mutation (DEC-FBR-IMPL-11). Guarded by the `OpsAuth` bearer-token extractor (`FEEDBACKMONK_OPS_TOKEN`), NOT the per-tenant `AdminSession` — this privilege separation keeps FR-FBR-14 intact (a Free tenant cannot self-upgrade or strip its "powered by feedbackmonk" badge). 404 when the token is unset (feature-off default). Drives the GitCellar self-host flip instead of raw SQL. |
| `attachments.rs` | `POST` multipart upload (≤4 images, ≤5MB each, MIME allowlist) | Screenshot attachments + opted-in captured-log part (GitCellar parity gap #1). Uses a dedicated `AttachmentState` axum sub-state; captured logs route through the canonical `feedbackmonk_tracing::scrub` chokepoint. Backed by `storage.rs` (LocalFs + S3/SigV4) and migration `00009`. |
| `health.rs` | `GET /health` + `GET /health/ready` | 12-factor liveness/readiness (FR-FBR-18, Contract C5). |
| `README.md` | — | This file. |

## 3. Public API & Usage

Handlers are wired through three composer functions, each returning an
`axum::Router` ready to be `.merge(...)`-ed by `main::build_app`:

```rust
use feedbackmonk_api::{worker_a_router, submission_router, admin_feedback_routes};

let app = worker_a_router(state.clone())
    .merge(submission_router(state.clone()))
    .merge(admin_feedback_routes(state))
    .layer(middleware...);
```

Handler signatures follow axum convention:

```rust
pub async fn some_handler(
    State(state): State<AppState>,
    session: AdminSession,          // 401/403 extractor for admin endpoints
    Path(id): Path<Uuid>,
    Query(params): Query<...>,
    Json(req): Json<...>,
) -> Result<Json<...>, ApiError>;
```

The submission handler additionally extracts `ConnectInfo<SocketAddr>`
for the anonymous-mode rate-limit hash; tests must populate this
extension manually when using `tower::ServiceExt::oneshot`.

## 4. Constraints & Business Rules

1. **Public endpoints declare their pre-auth status.** Only
   `feedback.rs::submit` uses `ProjectRepo::open_for_submission` (the
   allow-listed pre-auth boundary). Every other handler resolves scope
   via `AdminSession::scope`. New public endpoints MUST add an entry to
   `.claude/oracles/multi-tenant-isolation-check/allowlist.toml` with a
   rationale, or the oracle fails.
2. **JWT errors return 401 with structured JSON.** The submission
   handler maps `JwtError::variant_name()` into the response body
   (`{"error": "BadSignature" | "Expired" | "WrongAudience" |
   "AlgorithmNotAllowed" | ...}`). Returning a plain 401 is forbidden
   — integrations rely on the variant name to disambiguate.
3. **Body validation precedes scope resolution.** Submission handler
   parses `kind`, length-checks `body`, THEN opens the project scope.
   Reverse order would leak project-existence signal on a malformed
   request.
4. **Admin endpoints never expose raw tenant UUIDs to end users.** The
   `admin_feedback` detail response converts `transitioned_by` /
   `author_user_id` UUIDs to tenant emails (or
   `(unknown admin: <uuid>)` placeholder until `tenant_users` exists).
5. **Plain-text reply bodies.** `admin_feedback::reply` rejects nothing
   syntactically, but Worker B's UI renders the body as plain text
   (no HTML interpretation) — stored-XSS defense lives at the
   frontend. P1 deferred decisions explicitly chose plain-text-only
   composition; the handler accepts arbitrary bytes within the
   1..=16384 length range.
6. **Same-transaction status update + audit row insert.**
   `admin_feedback::transition_status` composes
   `FeedbackRepo::update_status_in_executor` +
   `FeedbackStatusHistoryRepo::append_in_executor` inside a single
   `pool.begin()`. Contract C6 Hard Invariant #4. Rollback of either
   path rolls back both; never write the audit row without the status
   update, never write the status update without the audit row.

## 5. Relationships & Dependencies

- **Consumed by**: `src/router.rs` (Worker-A subtree),
  `src/main.rs::build_app` (top-level composition + middleware).
- **Crate deps within feedbackmonk-api**: `state::AppState`, `error::ApiError`,
  `auth::AdminSession` + `auth::issue_session_cookie`,
  `email::{Mailer, EmailNotifier, EmailKind, EmailContext}`.
- **External crate deps**: `feedbackmonk-core` (FeedbackId / FeedbackKind /
  FeedbackStatus / `legal_transitions_from`), `feedbackmonk-repository`
  (every repo trait + scope newtypes + `EmailTenantBrand`),
  `feedbackmonk-jwt` (`verify_with_leeway`, `JwtError`), `feedbackmonk-anon`
  (`AnonGate`, `ANON_COOKIE_HEADER`, `RateLimitError`).
- **Test reachability**: handlers are exercised at two layers —
  in-file unit tests on pure helpers (validators, parsers, formatters)
  and `tests/handlers.rs` + `tests/router_submission_integration.rs`
  for Router-level integration (real Postgres via `sqlx::test`).

## 6. Decision Log

- **One handler family per file, not per endpoint.** `signup.rs` owns
  the signup + verify-email pair conceptually but they ship as separate
  files because verify-email's logic (token redemption + cookie
  issuance) is independent enough to read in isolation. The "one
  handler per file" rule applies when the handler has >100 lines of
  pre/post-condition code; smaller handlers stay grouped.
- **`AdminSession` extractor not a middleware.** Per-handler extractor
  invocation makes the auth boundary explicit in the function
  signature; middleware would hide the gate. Costs one extractor call
  per request; benefit is that grep-for-`AdminSession` enumerates every
  authenticated endpoint.
- **`ApiError::Conflict(String)` carries JSON-string bodies for C7
  errors.** The transition handler serializes its 409 body once and
  pushes the result through `ApiError::Conflict(String)` — keeps the
  error type narrow without introducing per-error variants. Pattern
  is documented inline at `admin_feedback::json_err`.
- **Submission handler exposes its anon-cookie minting via
  `Set-Cookie`.** The handler mints the cookie value if no
  `X-Feedbackmonk-Anon-Cookie` header arrives, and returns it via
  `Set-Cookie` on the response. Customers' widgets can either propagate
  the cookie back (recommended; sticks the rate-limit bucket to a
  browser) or ignore it (every request gets a fresh bucket — wider
  effective quota, accepted tradeoff for unauthenticated widget
  integrations).
- **`sole_project_scope` helper in `admin_feedback`.** P0/P1 ship one
  project per tenant in practice; the admin endpoints currently scope
  to the tenant's first project. Per-project admin URLs are FR-FBR-15
  / P3 work. Documented inline; the helper is the migration point when
  the surface widens.
- **Attachments use a dedicated `AttachmentState` axum sub-state, not new
  `AppState` fields.** Gap #1 (GitCellar parity) needs storage-backend
  config (LocalFs vs S3) and image/log limits that the rest of the API
  doesn't touch. Threading those through `AppState` would force edits to
  every existing `AppState` constructor (and every test that builds one).
  Instead `attachments.rs` carries its own `AttachmentState` sub-state
  merged at router-composition time. Zero edits to existing `AppState`
  constructors; the attachment surface is self-contained and independently
  testable. Trade-off: one extra `State<...>` extractor on attachment
  handlers — acceptable for the isolation it buys.
- **Captured logs route through the single `feedbackmonk-tracing` scrubber
  chokepoint — no second scrub path.** The attachment log-capture part
  (opt-in) is PII-scrubbed via the same `feedbackmonk_tracing::scrub`
  chain that all tracing output uses. A second, attachment-local scrubber
  would be a divergence risk (two patterns to keep in sync; one could
  silently fall behind). The `pii-scrub-audit` oracle Probe A enforces
  "no tracing/scrub setup outside `crates/feedbackmonk-tracing/`" as a
  code-level invariant, so this is mechanically defended, not just
  convention.
- **Gap-#4 end-user privacy isolation is enforced at the SQL predicate
  layer, not just in tests.** `me_feedback` handlers scope every query to
  the caller's own `end_user_sub` (from the JWT) AND to
  `visibility = 'public'` replies only — both as `WHERE` predicates in the
  repository query, not as post-fetch filtering. A caller can never see
  another end-user's feedback or internal/private replies even if the
  handler logic were bypassed, because the rows never leave Postgres.
  `tests/me_feedback_isolation.rs` is the regression guard, but the
  predicate is the actual boundary.
