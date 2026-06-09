<!--
Agent Context Header (ULADP):
- Purpose: Authentication primitives for the admin surface — argon2id
  password hashing + signed-cookie admin session (HMAC-SHA256). Sole
  source of the `AdminSession` extractor that gates every admin endpoint.
- Owner module: crates/feedbackmonk-api/src/auth/
- Read first: this README + Contract C11 (admin session cookie) in
  docs/planning/handoffs/p1-stage1-to-stage2.md
-->

# auth/ — Admin authentication

## Synopsis

Admin-authentication primitives for the feedbackmonk-api binary — argon2id password hashing + verification and the signed-cookie (`feedbackmonk_session`, HMAC-SHA256) admin session. Sole source of the `AdminSession` axum extractor that gates every admin endpoint and yields the `TenantScope` flowing into every repository call (DEC-FBR-03). Contract C11.

## 1. Purpose & Responsibilities

Stage 2 Worker A's admin-auth deliverable consumed by every admin
endpoint behind the `feedbackmonk_session` cookie (Contract C11):

- Hash + verify tenant passwords (argon2id, OWASP P0 defaults).
- Build + verify the signed-cookie admin session (HMAC-SHA256 over the
  16-byte tenant UUID concatenated with the 8-byte big-endian Unix
  issuance timestamp).
- Provide the `AdminSession` axum extractor that:
  - rejects (401) on missing / malformed / HMAC-invalid / expired /
    future-dated cookies,
  - rejects (401) when the tenant row has been deleted,
  - rejects (403) when the tenant is still in pending-verification
    state (`verified_at IS NULL`),
  - returns `AdminSession { scope: TenantScope }` on success — the
    scope flows into every repository call, upholding DEC-FBR-03's
    "scope-bound query path" invariant.

## 2. File Index

| File | One-line summary |
|---|---|
| `mod.rs` | Module surface — re-exports `ops::OpsAuth`, `password::*`, `session::{issue_session_cookie, AdminSession, SESSION_COOKIE_NAME}`. |
| `password.rs` | argon2id `hash_password` + `verify_password` (constant-time-safe wrapper around `argon2::PasswordHasher`). |
| `session.rs` | Cookie format, HMAC verification, `issue_session_cookie`, `AdminSession` `FromRequestParts` impl. Cookie value: `<b64(tenant_uuid_16)>.<b64(issued_unix_be_8)>.<b64(hmac_sha256_32)>`. |
| `ops.rs` | `OpsAuth` operator bearer-token guard for the ops mutation surface (DEC-FBR-IMPL-11). Deliberately separate from `AdminSession`: constant-time compares `Authorization: Bearer` against `FEEDBACKMONK_OPS_TOKEN`. Token unset ⇒ 404 (endpoint invisible); missing/wrong token ⇒ 401. Keeps a Free tenant's own admin session from flipping its tier or stripping the FR-FBR-14 badge. |
| `README.md` | This file. |

## 3. Public API & Usage

```rust
use feedbackmonk_api::auth::{
    hash_password, verify_password, issue_session_cookie, AdminSession,
    SESSION_COOKIE_NAME,
};

// Sign up / verify-email handler — mint the cookie after a successful
// password check or after redeeming a verify-email token.
let cookie = issue_session_cookie(tenant.id, state.session_secret.as_ref());
response.headers_mut().insert(SET_COOKIE, cookie.to_string().parse().unwrap());

// Admin endpoint — extract the session via axum extractor.
async fn admin_only_handler(
    session: AdminSession,            // 401/403 on failure
    State(state): State<AppState>,
) -> Result<Json<...>, ApiError> {
    let tenant_scope = session.scope;
    // tenant_scope flows into every repository call (DEC-FBR-03).
}
```

Cookie attributes (Contract C11):

| Property | Value |
|---|---|
| Name | `feedbackmonk_session` (the `SESSION_COOKIE_NAME` constant) |
| Max-Age | 7 days (`SESSION_MAX_AGE_SECS`) |
| HttpOnly | `true` |
| Secure | `true` (browsers tolerate `Secure` on localhost in dev) |
| SameSite | `Lax` |
| Path | `/` |
| Issued by | `issue_session_cookie(tenant_id, &secret)` (call from signup/verify-email/login handlers) |

## 4. Constraints & Business Rules

1. **`feedbackmonk_session` is the ONLY admin session cookie name.** The
   P1 plan referenced `feedbackmonk_admin_session`; the P0 implementation
   ships `feedbackmonk_session` (Contract C11 reconciles). Worker A + B +
   e2e scripts + tests all use `feedbackmonk_session` exactly.
2. **HMAC input order is load-bearing.** `concat(tenant_uuid_16,
   issued_unix_be_8)` — never the other way. Reordering or
   re-encoding either half invalidates every cookie in flight.
3. **`subtle::ConstantTimeEq` for tag comparison.** Constant-time
   comparison protects against timing-based tag-discovery attacks.
   `==` byte comparison is forbidden; CI's clippy-pedantic config does
   not flag this directly, so reviewers must catch.
4. **Future-dated cookie tolerance: 60 seconds.** Wider tolerance
   accepts clock-skewed attackers minting cookies for the future;
   tighter tolerance breaks under normal NTP drift.
5. **Tenant existence is re-checked at every extraction.**
   `TenantRepo::scope_for` proves the tenant row still exists. Deleted
   tenants get 401 even with valid signatures — sessions don't outlive
   account deletion.
6. **Pending-verification tenants get 403, not 401.** 401 says "no
   identity"; 403 says "your identity exists but isn't authorized
   yet." Frontend redirects 403 to a "check your inbox" page, not the
   login page.

## 5. Relationships & Dependencies

- **Consumed by**: every admin endpoint in
  `crates/feedbackmonk-api/src/handlers/` (signup completes the loop by
  issuing the cookie; admin_feedback / projects / signing_keys all
  extract `AdminSession`).
- **Crate deps**: `argon2`, `axum`, `axum-extra` (CookieJar), `base64`,
  `chrono`, `hmac`, `sha2`, `subtle`, `uuid`, `feedbackmonk-repository`
  (TenantScope).
- **Secret source**: `state.session_secret: Arc<[u8; 32]>`. Loaded from
  `FEEDBACKMONK_SESSION_SECRET` env var (64 hex chars → 32 raw bytes) at
  binary startup. Rotation requires a process restart; in-flight
  sessions invalidate on rotation.
- **No DB access in `session.rs`** — every DB lookup goes through the
  `TenantRepo` trait passed via `AppState`. The
  `multi-tenant-isolation-check` oracle's raw-SQL ban is therefore N/A
  inside the module.

## 6. Decision Log

- **HMAC-SHA256, not stateful sessions.** Single signed-cookie keeps
  the admin path zero-DB-write on every request. Stateful sessions
  (DB-backed session-id table) would require eviction logic + cost a
  query-per-request. The 7-day TTL + immediate-revocation-via-key-rotation
  is acceptable for the P0 threat model.
- **Cookie format: three `.`-separated b64url-no-pad segments.** Not
  JWT (avoids carrying the `alg` header surface area + the
  algorithm-confusion attack surface that haunts JWT cookies). Custom
  format means custom parser, which means we DON'T accept any algorithm
  other than HMAC-SHA256 — there's no header to confuse.
- **`AdminSession: Copy`.** The struct is `tenant_id + nothing`; making
  it `Copy` lets handlers freely re-use the scope without `.clone()`
  noise. The underlying `TenantScope` is `Copy` too — both newtypes
  are pure identity carriers.
- **`AdminSession` not split into `Session` + `AdminSession`.** P0/P1
  do not have a non-admin authenticated session concept — tenant
  identity IS admin identity in the current model. If a future phase
  introduces tenant-member roles, the split happens then (the type-name
  AdminSession was chosen forward-looking to that eventual split).
