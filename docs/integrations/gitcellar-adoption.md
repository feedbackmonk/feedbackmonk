# GitCellar ⇄ feedbackmonk — Integration Contract (Customer #1)

**Status**: ACTIVE — deployed 2026-06-02 on GitCellar Railway (`feedbackmonk-api` service). `project_id` = `a1350be8-3ff5-4744-9e1d-e35c97cc8aad`; tenant `triage@gitcellar.com`; signing key registered. API base `https://feedback.gitcellar.com` (cert provisioning) + `https://feedbackmonk-api-production.up.railway.app` (live). Originally drafted DRAFT — pre-deploy 2026-06-02 by the feedbackmonk side in response to
GitCellar's adoption intake (`../GitCellar/docs/planning/intakes/20260602T104026-adopt-feedbackmonk-as-feedback-system.md`).

> **DEPLOY DECISION (2026-06-02, resolved with user)**: **GitCellar self-hosts feedbackmonk on its
> existing Railway, reusing GitCellar's existing Postgres** (feedbackmonk gets its own database on
> that server; it is multi-tenant on a single Postgres by design). Chosen for lowest incremental
> cost (~$0–few/mo: one small API container, no second DB server). The `feedbackmonk.com` /
> `api.feedbackmonk.com` SaaS standup is deferred to a later product decision — NOT required for
> GitCellar's integration. Base URL below is therefore GitCellar's chosen host (e.g.
> `https://feedback.gitcellar.com`), not `api.feedbackmonk.com`. Runbook: `docs/operations/RAILWAY_GITCELLAR.md`.
**Owner (this doc)**: feedbackmonk repo. GitCellar consumes it; feedbackmonk keeps it authoritative.
**Companion runbooks**: `docs/operations/SELFHOST.md`, `docs/operations/SELFHOST_ENV.md` (Contract C21),
`CLAUDE.md` § PF-DEPLOY-01.

> This contract is the single meeting point between the two repos. GitCellar Desktop and
> gitcellar.com integrate against the surfaces frozen here. The one thing below that is **not yet
> live** is called out explicitly: the running deployment + the real `project_id` (PF-DEPLOY-01).
> The end-user read/list API GitCellar Desktop's "My Feedback" view needs (parity gap #4) is now
> **built and frozen** in §6. Everything else is built and code-verified.

---

## 0. TL;DR for the GitCellar integrator

| You need | Value | State |
|---|---|---|
| API base URL | `https://feedback.gitcellar.com` (GitCellar-self-hosted on its Railway — per deploy decision) | **live ✅** (`/health/ready` → 200, verified 2026-06-03) |
| `project_id` (the `aud` claim + URL path segment) | `a1350be8-3ff5-4744-9e1d-e35c97cc8aad` | **live ✅** (project provisioned; anon submit verified `FB-4R3VS8`) |
| Widget JS URL | vendored same-origin into gitcellar.com `public/feedback/` (the API does not serve `widget.js`) | **built ✅** — publish reverted to placeholder; re-publish at launch (see §1) |
| Anonymous widget embed (gitcellar.com) | see §4 | built ✅ |
| JWT minting spec (Desktop authenticated mode) | see §5 | verifier built ✅ |
| Signing-key registration | see §3.3 | built ✅ (`POST …/signing-keys`, Contract C4) |
| End-user "my feedback" list + reply-thread read API | see §6 | built ✅ (parity gap #4 — `GET …/me/feedback` + `…/me/feedback/<FB>/thread`) |

---

## 1. Deployment — what's required to stand feedbackmonk up

feedbackmonk's self-host stack and SaaS are the **same artifact** (DEC-FBR-05). The full
`docker compose up` stack is built and smoke-tested to `/health/ready` (FR-FBR-17,
`selfhost-compose-smoke` oracle GREEN). What does **not** yet exist is a *running, reachable*
instance — that is the whole of PF-DEPLOY-01.

### 1.1 Recommended host: Railway (match GitCellar)

GitCellar runs on Railway; matching it minimizes new ops surface and keeps both products in one
billing/observability plane. The stack maps cleanly onto Railway services:

| feedbackmonk component | Railway service | Domain |
|---|---|---|
| `feedbackmonk-api` (Rust/axum, `deploy/docker/Dockerfile.api`) | web service | `api.feedbackmonk.com` |
| admin UI (React/Vite → nginx, `Dockerfile.admin-ui`) | web service | `app.feedbackmonk.com` (or `feedbackmonk.com`) |
| widget `widget.js` + `widget.css` (static, 16.8 KB) | static / CDN | `cdn.feedbackmonk.com` |
| Postgres | Railway managed Postgres plugin | internal |
| `migrate` one-shot (`deploy/docker/migrate.sh`) | run-once / release command | — |
| marketing site (Astro, optional, FR-FBR-16) | static | `www.feedbackmonk.com` |

Notes:
- Railway's managed Postgres replaces the compose `db` service — point `DATABASE_URL` at it.
- Set `FEEDBACKMONK_BIND_ADDR=0.0.0.0` and `FEEDBACKMONK_PORT` to Railway's injected `$PORT`
  (or map it). Default bind is `127.0.0.1` for local dev (DEC-FBR-IMPL-07).
- `FEEDBACKMONK_PUBLIC_URL=https://api.feedbackmonk.com` (no trailing slash) — used in
  verify-email links and customer-facing URLs.
- `FEEDBACKMONK_CORS_ORIGINS=https://gitcellar.com` — **required** for the cross-origin widget
  embed (DEC-FBR-IMPL-09; see §4). Unset ⇒ the browser blocks every submission from gitcellar.com.
- Mailer: set `FEEDBACKMONK_MAILER=smtp` + the `FEEDBACKMONK_SMTP_*` vars (Mailpit is dev-only).
- Generate `FEEDBACKMONK_SESSION_SECRET` with `openssl rand -hex 32` (🔒, never commit).

Full env catalog: `docs/operations/SELFHOST_ENV.md`. The minimum-required set is `DATABASE_URL`,
`FEEDBACKMONK_PUBLIC_URL`, `FEEDBACKMONK_SESSION_SECRET`.

### 1.2 Faster alternative: GitCellar self-hosts feedbackmonk

If GitCellar wants the integration ASAP and doesn't need feedbackmonk.com live, GitCellar can run
the same compose stack on its own infra at e.g. `feedback.gitcellar.com`. This does **not** depend
on `feedbackmonk.com` DNS. Same contract below; only the base URL changes. (This is PF-DEPLOY-01's
"self-host" option.)

---

## 2. API base + transport

- **Base**: `https://api.feedbackmonk.com` (prod) — substitute your deployment's host.
- All endpoints are under `/api/v1/`.
- Health: `GET /health` (liveness), `GET /health/ready` (readiness; checks DB).
- Admin endpoints (`/api/v1/admin/*`, `POST/GET /api/v1/projects`, `…/signing-keys`) are gated by
  an HMAC-signed admin-session cookie (`feedbackmonk_session`) obtained via signup → verify-email.
  These are **operator** actions, not Desktop actions.
- End-user endpoints (submit; widget-config; public roadmap) are **public** — JWT bearer optional.

---

## 3. Provisioning the "GitCellar" tenant + project + signing key

All three steps run **once**, by a feedbackmonk operator, after deploy. Until then the values
below are placeholders.

### 3.1 Create the tenant (operator signup)

```
POST https://api.feedbackmonk.com/api/v1/signup
Content-Type: application/json
{ "email": "ops@gitcellar.com", "password": "<strong-pass>" }
→ 202  { "tenant_id": "<TENANT_ID>" }
```
Then redeem the verify-email token (`POST /api/v1/verify-email`); on success the response sets the
`feedbackmonk_session` cookie used for the admin calls below.

### 3.2 Create the GitCellar project

```
POST https://api.feedbackmonk.com/api/v1/projects        (admin-session cookie)
Content-Type: application/json
{ "name": "GitCellar", "slug": "gitcellar" }
→ 200 {
    "project_id": "<PROJECT_ID>",        ← this UUID is the JWT `aud` + URL path segment
    "name": "GitCellar",
    "slug": "gitcellar",
    "created_at": "...",
    "embed_snippet": "<script src=\".../widget.js\" data-project=\"gitcellar\"></script>"
  }
```
⚠️ **Use `project_id` (the UUID), not `slug`, everywhere downstream.** See §7 discrepancy note —
the server-emitted `embed_snippet` is currently stale and should not be copied verbatim.

> **DONE (2026-06-02): `project_id = a1350be8-3ff5-4744-9e1d-e35c97cc8aad`. Tenant `triage@gitcellar.com`; signing key_id `4704a9b4-4798-4d2c-a2ed-ba49f887fe6e`. Deployed on GitCellar Railway; anonymous submit verified end-to-end (FB-4R3VS8).**

### 3.3 Register GitCellar's Ed25519 signing key (Contract C4)

GitCellar generates the keypair itself and registers **only the public key** (DEC-FBR-04 — the
private key never leaves GitCellar). The public key is exactly 32 raw bytes, standard-base64-encoded.

Generate (example):
```bash
# Ed25519 keypair; export the 32-byte raw public key as base64.
openssl genpkey -algorithm ed25519 -out gitcellar_fbm_priv.pem
openssl pkey -in gitcellar_fbm_priv.pem -pubout -outform DER \
  | tail -c 32 | base64       # → <PUBLIC_KEY_BASE64>
```

Register:
```
POST https://api.feedbackmonk.com/api/v1/projects/<PROJECT_ID>/signing-keys   (admin-session cookie)
Content-Type: application/json
{ "public_key_base64": "<PUBLIC_KEY_BASE64>", "label": "gitcellar-desktop-2026" }
→ 200 { "key_id": "<KEY_ID>", "label": "...", "registered_at": "..." }
```
- Field name of record is `public_key_base64` (alias `public_key_b64` also accepted).
- Must decode to **exactly 32 bytes**; all-zero key rejected (400).
- Multiple active keys are allowed (key rotation). Deactivate with
  `DELETE /api/v1/projects/<PROJECT_ID>/signing-keys/<KEY_ID>`.

---

## 4. Widget embed — anonymous mode (gitcellar.com website feedback)

For anonymous website feedback on gitcellar.com (no end-user identity), embed **without** `data-jwt`:

```html
<script
  type="module"
  src="https://cdn.feedbackmonk.com/widget.js"
  data-project-id="<PROJECT_ID>"
  data-api-base="https://api.feedbackmonk.com"
></script>
```

- Anonymous mode: the widget uses `credentials: "include"` so the `X-Feedbackmonk-Anon-Cookie`
  travels for per-cookie/IP dedup + rate limiting (FR-FBR-06; default 10 submissions/hour per
  `(anon_hash, project)`, tunable via `FEEDBACKMONK_ANON_RATE_LIMIT_PER_HOUR`).
- Bundle: 16.8 KB total (well under the 30 KB cap). Zero third-party trackers (DEC-FBR-02,
  oracle-enforced). CSP-safe — no `unsafe-inline`/`unsafe-eval` required.
- a11y: WCAG 2.1 AA (role=dialog, focus trap, ESC + focus return) — axe-core clean.
- `data-api-base` is optional but **recommended** here since gitcellar.com is a different origin
  from the API.

> **⚠️ CORS — required for the cross-origin embed (DEC-FBR-IMPL-09).** Because gitcellar.com and the
> API host are different *origins*, the browser sends a preflight `OPTIONS` for the submit `POST` and
> requires `Access-Control-*` headers on the response. The API gates this on an env allowlist:
> set **`FEEDBACKMONK_CORS_ORIGINS=https://gitcellar.com`** on the deployed `feedbackmonk-api`
> service (comma-separate additional origins, e.g. `…,https://www.gitcellar.com`; no trailing
> slash). **Unset ⇒ the browser blocks every submission** (CORS error in the console; the API
> returns the preflight without an `Access-Control-Allow-Origin`). The anonymous path uses
> `credentials: include` (the dedup cookie), so the response echoes the *specific* origin (never `*`)
> and sets `Access-Control-Allow-Credentials: true`; the anon cookie is `SameSite=None; Secure`,
> which requires the API to be served over **HTTPS**. `widget-config` is unaffected (it stays
> `*`-public). GitCellar Desktop's native (non-browser) client needs no CORS. See
> `docs/operations/SELFHOST_ENV.md` → `FEEDBACKMONK_CORS_ORIGINS`.

---

## 5. JWT minting — authenticated mode (GitCellar Desktop per-user)

When GitCellar Desktop submits feedback on behalf of a signed-in user, it mints a short-lived
EdDSA JWT and passes it as `Authorization: Bearer <jwt>` (the widget does this automatically when
`data-jwt` is present; Desktop's native client sets the header directly). The feedbackmonk verifier
is `crates/feedbackmonk-jwt` (Contract C2) and is **strict**:

### 5.1 Header
```json
{ "alg": "EdDSA", "typ": "JWT" }
```
- **`alg` MUST be `EdDSA`** (Ed25519). `none`, `HS256`, `RS256`, etc. are rejected
  (`AlgorithmNotAllowed`) — no algorithm confusion.

### 5.2 Required claims (all four mandatory)
| Claim | Type | Meaning | Rule |
|---|---|---|---|
| `sub` | string | Stable per-user identifier (GitCellar's user id). **This is the ONLY identity feedbackmonk ever stores** for the end-user (DEC-FBR-04). | required; persisted as `feedback.end_user_sub` |
| `aud` | string | **The `<PROJECT_ID>` UUID** (string form) | must equal the project in the URL path, else `WrongAudience` |
| `iat` | number | issued-at, epoch seconds | leeway ±5s default (`FEEDBACKMONK_JWT_LEEWAY_SECONDS`); future-`iat` beyond leeway → `NotYetValid` |
| `exp` | number | expiry, epoch seconds | **STRICT** — `now > exp` → `Expired`. No leeway on `exp`. |

### 5.3 Optional claims (read into `VerifiedClaims`, stored on the feedback row)
| Claim | Type | Notes |
|---|---|---|
| `email` | string | optional; surfaced for status emails to the user |
| `name` | string | optional display name |
| `external_metadata` | JSON object | optional; **≤ 4096 bytes** or `ExternalMetadataTooLarge`. Good place for GitCellar app version, OS, etc. |

### 5.4 TTL convention
- Mint with a **short TTL (~5 minutes)**: `exp = iat + 300`. The 5-minute window is a
  customer-side minting convention (the verifier enforces whatever `exp` you set, strictly).
  Desktop should mint a fresh token per submission/poll rather than caching long-lived tokens.

### 5.5 Submit with the token
```
POST https://api.feedbackmonk.com/api/v1/projects/<PROJECT_ID>/feedback
Authorization: Bearer <jwt>
Content-Type: application/json
{ "body": "...", "kind": "bug" }          # kind ∈ bug|feature|question|other (default other)
→ 200 { "feedback_id": "FB-XXXXXX", "accepted_at": "...", "echo": { "body": "...", "kind": "bug" } }
```
JWT failures return **401** with `{ "error": "<variant>" }` where variant ∈
`BadSignature | Expired | NotYetValid | WrongAudience | AlgorithmNotAllowed | MissingRequiredClaim
| ExternalMetadataTooLarge | MalformedToken` — Desktop can disambiguate (e.g. re-mint on `Expired`).
Body cap 16384 chars (413 if exceeded). Tier cap → 402.

### 5.6 Crash-event linking + crash-link banner (parity gap #2 — BUILT)

When Desktop submits feedback right after a crash, it can link the feedback to the **Glitchtip**
(Sentry-API-compatible) crash event so triagers — and the user — see the crash inline.

**Submit-time:** add an optional `crash_event_id` to the **auth-mode** submit body. It is stored as a
first-class `feedback.crash_event_id` column (migration `00010`), **not** inside `external_metadata`.

```
POST https://api.feedbackmonk.com/api/v1/projects/<PROJECT_ID>/feedback
Authorization: Bearer <jwt>
Content-Type: application/json
{ "body": "App panicked on save", "kind": "bug",
  "crash_event_id": "a1b2c3d4e5f60718293a4b5c6d7e8f90" }   # ← Glitchtip event id, ≤128 chars
→ 200 { "feedback_id": "FB-XXXXXX", "accepted_at": "...", "echo": { ... } }
```

- **Auth-mode only.** `crash_event_id` is read from the body **only** when a verified JWT is present
  (it comes from the signed-in Desktop context). On the anonymous path it is ignored.
- Storing the link is **never** a failure point: an unreachable Glitchtip does **not** affect submit —
  submit only persists the id; resolving it to detail happens later, off the hot path.

**Render-time (banner shape):** feedbackmonk resolves `crash_event_id` to crash detail by **pulling**
it from Glitchtip on demand (`GET {glitchtip}/api/0/projects/{org}/{project}/events/{id}/`, read-only
token) — feedbackmonk is **not** a Glitchtip webhook target. The resolved shape Desktop renders as a
crash-link banner:

```json
{
  "crash_event_id": "a1b2c3d4e5f60718293a4b5c6d7e8f90",
  "title": "TypeError: cannot read 'id' of undefined",
  "culprit": "renderBanner (app/banner.tsx)",   // nullable
  "level": "fatal",                               // nullable: error|warning|fatal|…
  "permalink": "https://glitchtip…/events/…/",    // nullable — deep link to open
  "last_seen": "2026-06-02T11:59:00Z"             // nullable, RFC3339
}
```

Correlation is **best-effort**: when Glitchtip is unreachable or the id is unknown, the resolver
returns an "unavailable"/"not found" outcome and the banner degrades gracefully (no error to the
user; `title` is the only field guaranteed when an event *is* found). Desktop should treat every field
except `crash_event_id`/`title` as optional.

> **Deploy note (PF-DEPLOY-01):** the correlation *resolver* (`crash_correlation` worker) is built and
> unit-tested against a mock Glitchtip; pointing it at GitCellar's live Glitchtip needs the four
> `FEEDBACKMONK_GLITCHTIP_{URL,ORG,PROJECT,TOKEN}` env vars set at deploy. Until then, `crash_event_id`
> is still captured + stored; only the resolved banner detail is "unavailable".

---

## 6. ✅ BUILT & FROZEN — end-user read/list API (parity gap #4)

> **Status: BUILT** (parity gap #4 closed; PODS collab-20260602-123000, CLAUDE-DELTA). No schema
> change. Paths below are **frozen** — GitCellar Desktop (`fetch_my_feedback`,
> `fetch_feedback_thread`, `poll_for_updates`) integrates against these. Privacy invariants are
> enforced by `crates/feedbackmonk-api/tests/me_feedback_isolation.rs` (7 named tests) and the
> `feedback-parity-status` oracle reports Gap #4 CLOSED.

Two **JWT-`sub`-scoped** read routes complement the existing `POST …/feedback` submit route. Auth is
identical to submit (§5): `Authorization: Bearer <EdDSA-JWT>`, `aud` MUST equal `<PROJECT_ID>`. The
verified `sub` is the only identity used; every query is scoped to it.

### 6.1 List my feedback

```
GET /api/v1/projects/<PROJECT_ID>/me/feedback?limit=<1..100>&offset=<n>
Authorization: Bearer <jwt>
→ 200 {
    "items": [
      {
        "feedback_id": "FB-XXXXXX",
        "kind": "bug" | "feature" | "question" | "other",
        "status": "submitted" | "triaged" | "in-progress" | "shipped" | "wont-fix" | "duplicate",
        "body": "...",
        "submitted_at": "2026-06-02T15:00:00Z"
      }, …
    ],
    "total": 12,        // total rows for this sub (not the page size)
    "limit": 20,        // echoed; default 20, capped at 100
    "offset": 0
  }
```
- Returns ONLY rows where `end_user_sub == jwt.sub` (caller's own), newest-first. Another user's
  feedback and **anonymous** submissions are never returned.
- `limit` default 20, max 100 (values above 100 are clamped). `offset` default 0.

### 6.2 Feedback thread (status + public replies)

```
GET /api/v1/projects/<PROJECT_ID>/me/feedback/<FB-ID>/thread
Authorization: Bearer <jwt>
→ 200 {
    "feedback_id": "FB-XXXXXX",
    "kind": "bug",
    "status": "in-progress",
    "body": "...",
    "submitted_at": "2026-06-02T15:00:00Z",
    "replies": [
      { "reply_id": "<uuid>", "body": "...", "created_at": "2026-06-02T16:00:00Z" }, …
    ]
  }
```
- `replies` contains **PUBLIC replies only**, chronological (oldest-first). `internal` admin triage
  notes are NEVER exposed. No admin authorship/visibility is surfaced.
- `<FB-ID>` is the `feedback_id` from §6.1 (the `FB-XXXXXX` short code).

### 6.3 Errors

| Condition | Status | Body |
|---|---|---|
| JWT verification failure (`Expired`, `WrongAudience`, `AlgorithmNotAllowed`, `BadSignature`, …) | `401` | `{"error":"<JwtError variant>"}` (same variants as §5.5 — Desktop can re-mint on `Expired`) |
| Missing / empty `Authorization` Bearer | `401` | `{"error":"unauthorized"}` |
| `<FB-ID>` not owned by the caller's `sub` (or anonymous, or another tenant/project) | `404` | `{"error":"not found"}` |
| Unknown `<PROJECT_ID>` | `404` | `{"error":"not found"}` |

> **Polling note**: Desktop's tray `poll_for_updates` should call §6.1 (cheap, paginated) and open a
> thread (§6.2) on demand. Mint a fresh short-TTL JWT per poll (§5.4) rather than caching tokens.

---

## 7. Known discrepancy to fix (discovered during this intake)

The server-emitted `embed_snippet` (from `POST /api/v1/projects`) is **stale**:
`build_embed_snippet` in `crates/feedbackmonk-api/src/handlers/projects.rs` emits
`<script src="<public_url>/widget.js" data-project="<slug>">`, but the shipped widget reads
`data-project-id` (the **UUID**, not slug) and `data-api-base` (`widget/src/widget.ts`). A customer
copying the emitted snippet verbatim would get a non-functional embed. **Use §4 above as the
canonical embed.** Tracked as a discovery to fix on the feedbackmonk side (small).

---

## 8. Parity gap summary (feedbackmonk side)

| # | Parity item | Verified state | Action |
|---|---|---|---|
| 1 | Attachments (≤4 screenshots ≤5MB, canvas redaction, service-log capture w/ PII scrub, console-log) | **MISSING** — no attachment table/columns, no multipart handling | build |
| 2 | Crash-event correlation (`crash_event_id` ↔ Glitchtip + worker) | **BUILT** — `crash_event_id` column (migration 00010) + auth-mode submit accept + pull-mode `crash_correlation` worker (see §5.6). Live Glitchtip wiring pending deploy env (PF-DEPLOY-01) | done (deploy env pending) |
| 3 | Admin full-text search across feedback | **MISSING** — no search route, no tsvector/index | build |
| 4 | End-user JWT my-feedback list + reply-thread read API | **BUILT** ✅ — `GET …/me/feedback` + `…/me/feedback/<FB>/thread`, JWT-`sub`-scoped (see §6) | done |
| 5 | Forge issue bridge | N/A — GitCellar is DROPPING it (DEC-FBR-06 already drops Forge) | none |

---

## Change log
- 2026-06-02 — Initial draft authored from code verification at `78aca1e`. `project_id`, deploy host,
  and gap-#4 endpoints are placeholders pending PF-DEPLOY-01 and gap-closing build.
- 2026-06-02 (BRAVO, PODS collab-20260602-123000) — Parity gap #2 BUILT: added §5.6 crash-event linking
  + crash-link banner contract (pull-mode, best-effort); flipped §8 gap-#2 row to BUILT. Live Glitchtip
  resolution gated on `FEEDBACKMONK_GLITCHTIP_*` env at deploy (PF-DEPLOY-01).
- 2026-06-02 (DELTA, PODS collab-20260602-123000) — Parity gap #4 BUILT & FROZEN: §6 flipped from
  "NOT YET BUILT/proposed" to the final `GET …/me/feedback` (paginated) + `…/me/feedback/<FB>/thread`
  (status + public-replies-only) JWT-`sub`-scoped contract incl. error table; flipped §0 TL;DR + §8
  gap-#4 rows to built. No schema change. Isolation invariants frozen by
  `crates/feedbackmonk-api/tests/me_feedback_isolation.rs`.
- 2026-06-02 (CORS fix, DEC-FBR-IMPL-09) — Added a credentialed CORS layer to the public widget
  endpoints (submit + attachments), gated by `FEEDBACKMONK_CORS_ORIGINS`; anon cookie →
  `SameSite=None; Secure`. Fixes the gitcellar.com embed blocker (preflight `OPTIONS` was `405`).
  Surfaced from the GitCellar side. Deploy action: set `FEEDBACKMONK_CORS_ORIGINS=https://gitcellar.com`
  on the `feedbackmonk-api` service (see §1.1 + §4).
