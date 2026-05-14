# P2 Fan-Out Contracts — Frozen for collab-20260514-035703

**Predecessor (P1)**: `docs/planning/handoffs/p1-stage1-to-stage2.md` (P1 carry-state — still authoritative for everything P1 froze; this doc layers P2 deltas)
**Plan**: `docs/planning/plans/20260514T034730-feedbackmonk-p2-customer-facing.md` (P2 execution plan — §Interface Contracts is the source of truth this file extracts/refines)
**Authored by**: CLAUDE-B (PODS Roadmap Backend worker, collab-20260514-035703)
**Authored at**: 2026-05-14T04:05:00Z
**Audience**: CLAUDE-A (reads C12 to align its widget code), CLAUDE-C (reads C13/C14/C15/C16 to scaffold admin-ui + write the C16 handler body), LD (ratifies in `channels/decisions.md`)

> Read this BEFORE touching backend integration code. CLAUDE-C may scaffold admin-ui pages against the TypeScript starter kit at the bottom of this file even before CLAUDE-B's handler signature compiles, but backend integration (e.g. wiring `fetch('/api/v1/admin/feedback/.../promote')` to a real type) is gated on C16's handler-signature commit.

---

## What P2 inherits as frozen (from P0/P1 — DO NOT MODIFY in P2)

| Surface | Where it lives | Stable for P2 |
|---|---|---|
| 20-pattern PII scrubber + `install_global_subscriber` | `crates/feedbackmonk-tracing/` | Yes. Hash-locked. New emit paths inherit scrubbing automatically. |
| `feedbackmonk_anon::AnonGate::token_hash(client_ip, cookie, project_id)` | `crates/feedbackmonk-anon/src/lib.rs` | Yes. **Canonical chokepoint**. CLAUDE-B's anon-mode `voter_id` MUST call this; NEVER parallel-implement. |
| `feedbackmonk_jwt::verify_with_leeway(token, expected_aud_project_id, active_keys, now_unix, iat_leeway_seconds)` | `crates/feedbackmonk-jwt/src/lib.rs` | Yes. **Canonical chokepoint**. CLAUDE-B's auth-mode vote resolution MUST call this; NEVER write a parallel verifier. (Convention: existing submission handler imports as `use feedbackmonk_jwt::{verify_with_leeway as jwt_verify_with_leeway, …};`. Roadmap handler mirrors that aliasing.) |
| `ANON_COOKIE_HEADER = "X-Feedbackmonk-Anon-Cookie"` | `feedbackmonk_anon` | Yes. CLAUDE-B's anon-vote path reads it from the request headers, mints a fresh one (via `AnonGate::mint_cookie`) if absent, and emits `Set-Cookie` mirroring the submission handler. |
| `SESSION_COOKIE_NAME = "feedbackmonk_session"` + `AdminSession` extractor | `crates/feedbackmonk-api/src/auth/session.rs` | Yes. CLAUDE-B's admin endpoints + CLAUDE-C's promote endpoint sit behind `AdminSession`. |
| `FeedbackStatus` state machine + `legal_transitions_from` | `crates/feedbackmonk-core/src/status.rs` | Yes. CLAUDE-C's promote handler transitions source feedback → `Duplicate` via the existing `Submitted/Triaged/InProgress → Duplicate` legal transition. |
| `feedback_status_history` table + `FeedbackStatusHistoryRepo::append_in_executor` | migration 00003 + `crates/feedbackmonk-repository/src/feedback_status_history.rs` | Yes. CLAUDE-C composes a same-txn pair via `update_status_in_executor` + `append_in_executor` for Contract C6 Hard Invariant #4 atomicity. |
| `FeedbackRepo::update_status_in_executor` | `crates/feedbackmonk-repository/src/feedback.rs` | Yes. Pre-authorized widening shipped in P1. CLAUDE-C re-uses for the same-txn status flip. |
| `ProjectScope` / `TenantScope` newtypes + `ProjectRepo::open` / `open_for_submission` | `crates/feedbackmonk-repository/src/scope.rs` + `projects.rs` | Yes. Every CLAUDE-B repo method takes `&ProjectScope` first; admin endpoints derive scope from `AdminSession`. Public read endpoints derive scope via `open_for_submission` (already allowlisted as a pre-auth boundary). |
| `ApiError` + `RepoError::{Conflict, NotFound, TenantProjectMismatch}` | `crates/feedbackmonk-api/src/error.rs` + repo `error.rs` | Yes. CLAUDE-B maps `RepoError::Conflict` from a `cast` unique-violation to HTTP 409. |
| `multi-tenant-isolation-check` oracle + allowlist shape | `.claude/oracles/multi-tenant-isolation-check/` | Yes. CLAUDE-B adds two STRUCTURAL-MIRROR `[[inherent_methods]]` constructor entries (`SqlxRoadmapItemRepo::new` + `SqlxRoadmapVoteRepo::new`) — LD-pre-authorized per GUIDE.md §8. NON-constructor allowlist additions remain `channels/alerts.md` halts. |
| `pii-scrub-audit` oracle | `.claude/oracles/pii-scrub-audit/` | Yes. CLAUDE-B's new modules inherit the workspace-wide scrubber automatically. No `tracing_subscriber` setup outside `crates/feedbackmonk-tracing/`. |
| migrations 00001..00005 | `migrations/` | Frozen. P2 appends 00006 + 00007. |

---

## Pre-authorized self-mediation widenings (PODS Coordination Protocol)

(Verbatim cross-link to GUIDE.md §8 — repeated here so workers don't have to flip files.)

| Surface | Allowed widening shape | Tag in `channels/decisions.md` |
|---|---|---|
| `RoadmapItem` / `RoadmapVote` Rust types | Additional **optional read-only** fields beyond the C13/C14 spec (e.g., `vote_count_cached: i64`) as long as the table schema supports them. | `self_mediated=true; ratification_pending=true; matches_spec_at=docs/planning/plans/20260514T034730-feedbackmonk-p2-customer-facing.md#contract-c13` |
| `WidgetConfig` JSON response shape | Additional **optional** fields; never remove/rename. | same with `#contract-c12` anchor |
| Voting cache internal types | `VotingCache` struct shape, `CacheInner` fields — internal to CLAUDE-B's crate; freely choose. | (n/a — internal) |
| `widget-bundle-size` oracle tracker-domain list | CLAUDE-A may ADD canonical tracker hostnames; never remove. | `self_mediated=true; …; matches_spec_at=…#oracle-pre-build-plan` |
| Vote retraction window | CLAUDE-B may flex 30–120s if UX/abuse concerns surface during impl. Default 60s. | `self_mediated=true; …; matches_spec_at=…#deferred-decisions` |

**NOT pre-authorized** (halt via `channels/alerts.md`):

- Q24 byte-for-byte invariant — `render_roadmap_body` / `render_roadmap_title` modifications beyond the verbatim gitcellar port. Modifying the Q24 assertions is a halt.
- New entries in `multi-tenant-isolation-check/allowlist.toml` beyond the two structural-mirror constructor entries.
- New `RoadmapItemStatus` variants or new `FeedbackStatus` variants.
- Non-backwards-compatible JSON shape changes on any C12–C16 endpoint after this freeze.
- Pattern-set changes in `crates/feedbackmonk-tracing/src/scrubber.rs`.
- Modification of existing P0/P1 tests (Read-Only-Tests autopilot mode). Net-new tests (including the 6 Q24 ports + CLAUDE-B's roadmap-cache + repo tests) are permitted.

---

## Contract C12 — Widget runtime config endpoint

> **Author**: CLAUDE-A. **Consumer**: CLAUDE-A only on the Rust side; widget JS code mirrors the JSON shape. This block mirrors the plan §C12 verbatim so this file is a single-stop reference for the TypeScript type-gen.

**Endpoint**: `GET /api/v1/projects/{project_id}/widget-config`
**Auth**: none (public — the project_id is the public widget key)
**Repository surface used**: `ProjectRepo::open_for_submission(project_id)` (already allowlisted; pre-auth boundary per DEC-PODS-001) + new `TenantRepo::get_widget_brand(&TenantScope) -> WidgetBrand`
**CORS**: open `Access-Control-Allow-Origin: *` per DEC-FBR-04 (domain-allowlist enforcement happens at the SUBMISSION endpoint, not at config).
**Cache**: HTTP `Cache-Control: public, max-age=60` (matches voting-cache TTL; trades freshness for response budget).

**Response shape** (Rust → JSON):

```rust
// crates/feedbackmonk-api/src/handlers/widget_config.rs (CLAUDE-A authors)

pub struct WidgetConfigResponse {
    pub project_id: Uuid,
    pub tenant_id: Uuid,
    pub display_name: String,           // project's customer-visible name
    pub brand: WidgetBrand,
    pub auth_modes: Vec<&'static str>,  // ["auth", "anonymous"] for v1
    pub submission_kinds: Vec<&'static str>, // ["bug","feature","question","other"]
    pub max_body_chars: usize,          // 16384 (mirrors FeedbackRequest::MAX_BODY_CHARS)
}

pub struct WidgetBrand {
    pub primary_color: String,          // hex "#3b82f6"
    pub logo_url: Option<String>,
    pub footer_text: Option<String>,    // "powered by feedbackmonk" on free tier; None on paid (P3 flips)
}
```

```json
{
  "project_id": "uuid",
  "tenant_id": "uuid",
  "display_name": "Acme Customer Feedback",
  "brand": {
    "primary_color": "#3b82f6",
    "logo_url": null,
    "footer_text": "powered by feedbackmonk"
  },
  "auth_modes": ["auth", "anonymous"],
  "submission_kinds": ["bug", "feature", "question", "other"],
  "max_body_chars": 16384
}
```

**Hard invariants**:
1. Endpoint is unauthenticated — anyone with a `project_id` can fetch widget config. There is no PII in the response.
2. `auth_modes` is hardcoded for v1 (`["auth","anonymous"]`); P3 may filter based on tier state.
3. `submission_kinds` mirrors the existing `FeedbackKind` enum (bug/feature/question/other).
4. `max_body_chars = 16384` is sourced from `feedbackmonk_api::handlers::feedback::MAX_BODY_CHARS` — mismatch would silently break the widget's pre-flight validation.

---

## Contract C13 — Roadmap item schema (CLAUDE-B authors; CLAUDE-C + P4 marketing consume)

**Migration** (`migrations/00006_roadmap_items.sql`):

```sql
CREATE TABLE roadmap_items (
  id                 UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id          UUID NOT NULL REFERENCES tenants(id),
  project_id         UUID NOT NULL REFERENCES projects(id),
  slug               TEXT NOT NULL,           -- URL component, unique per project
  title              TEXT NOT NULL CHECK (char_length(title) BETWEEN 1 AND 200),
  body               TEXT NOT NULL CHECK (char_length(body) BETWEEN 1 AND 16384),
  status             TEXT NOT NULL DEFAULT 'considering'
                       CHECK (status IN ('considering','planned','in-progress','shipped','wontfix')),
  origin_feedback_id UUID REFERENCES feedback(id), -- nullable: admin can create from scratch
  created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
  created_by         UUID NOT NULL,           -- admin user id (P1 admin session)
  updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (project_id, slug),
  UNIQUE (origin_feedback_id)                 -- "one roadmap item per source feedback" idempotency
);

CREATE INDEX roadmap_items_tenant_project_status_idx
  ON roadmap_items (tenant_id, project_id, status);
```

**`updated_at` policy**: feedbackmonk doesn't currently ship a generic UPDATE trigger. CLAUDE-B updates `updated_at` explicitly in the `update()` repo method via `SET updated_at = now()` inside the UPDATE SQL. Documented in module README so a future generic trigger doesn't double-bump. (Pre-authorized self-mediation: either approach works; CLAUDE-B picks the explicit-SQL form because feedbackmonk-repository's existing convention is to keep all timestamp mutation visible in Rust call sites.)

**Status state machine** (no audit-history table — admin can edit freely; activity log is P3 work):

- `considering → planned → in-progress → shipped` (forward path)
- any → `wontfix` (close)
- `shipped` is terminal
- `wontfix` can be re-opened to `considering` via admin edit

**Rust types** (`crates/feedbackmonk-core/src/roadmap.rs`):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RoadmapItemStatus {
    #[default]
    Considering,
    Planned,
    InProgress,   // "in-progress"
    Shipped,
    WontFix,      // "wontfix"
}

impl RoadmapItemStatus {
    pub fn as_db_str(self) -> &'static str { /* kebab-case */ }
    pub fn from_db_str(s: &str) -> Self    { /* lenient; unknown → Considering */ }
    /// Public listing filters by these in v1; admin sees all.
    pub fn is_public_visible(self) -> bool { true /* all 5 — drafts don't exist as a separate state */ }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RoadmapItem {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub slug: String,
    pub title: String,
    pub body: String,
    pub status: RoadmapItemStatus,
    pub origin_feedback_id: Option<Uuid>,
    pub created_at: chrono::DateTime<Utc>,
    pub created_by: Uuid,
    pub updated_at: chrono::DateTime<Utc>,
}
```

**Hard invariants**:
1. `(project_id, slug)` is unique per project — slugs are URL components in the public roadmap.
2. `origin_feedback_id` is unique workspace-wide — one roadmap item per source feedback. Idempotent promote uses this constraint.
3. Status is one of 5 kebab-case strings — DB CHECK + Rust `from_db_str` are both lenient (unknown maps to `Considering`) so an old admin-ui pinned to a stale type-gen never 500s.
4. There is NO separate "draft" state — admin creates items directly into `Considering`. Public visibility is governed by the status enum, not a separate column. Public endpoints filter `status IN (...)` (in v1, all 5; left as a server-side knob if `WontFix` should be hidden later — controlled by `RoadmapItemStatus::is_public_visible`).

---

## Contract C14 — Roadmap voting schema + double-vote prevention

**Migration** (`migrations/00007_roadmap_votes.sql`):

```sql
CREATE TABLE roadmap_votes (
  id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id   UUID NOT NULL REFERENCES tenants(id),
  project_id  UUID NOT NULL REFERENCES projects(id),
  item_id     UUID NOT NULL REFERENCES roadmap_items(id) ON DELETE CASCADE,
  voter_id    TEXT NOT NULL,            -- JWT `sub` (auth mode) OR hex(token_hash) (anon mode)
  voter_mode  TEXT NOT NULL CHECK (voter_mode IN ('jwt','anon')),
  cast_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (item_id, voter_id)            -- Q5 drift defender for "1 vote per (item, voter)" rule
);

CREATE INDEX roadmap_votes_item_id_idx ON roadmap_votes (item_id);
```

**Rust types**:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoadmapVoterMode { Jwt, Anon }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoadmapVote {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub item_id: Uuid,
    pub voter_id: String,
    pub voter_mode: RoadmapVoterMode,
    pub cast_at: chrono::DateTime<Utc>,
}
```

**Hard invariants** (each backed by a named test):

1. **Double-vote returns 409**: A second INSERT with the same `(item_id, voter_id)` returns `Err(RepoError::Conflict)` from `RoadmapVoteRepo::cast`; the handler maps to `409 Conflict` with body `{"error": "AlreadyVoted"}`. **NOT silent upsert** — the user MUST see a distinct outcome.
2. **Anon `voter_id` re-uses the canonical chokepoint**: `voter_id = hex(AnonGate::token_hash(client_ip, cookie, project_id))`. Same hash function the submission endpoint uses — no parallel implementation. Per-project hash domain (the function already mixes `project_id` into the BLAKE3) prevents cross-project vote-replay.
3. **JWT `voter_id` is `claims.sub`** from `feedbackmonk_jwt::verify_with_leeway` — same chokepoint as submission's auth-mode path. Audience check ensures the JWT was minted for THIS project.
4. **Retraction window**: `DELETE` on a vote is permitted within 60s of `cast_at`; after that returns `403 Forbidden` with body `{"error": "RetractionWindowExpired"}`. Default `RETRACTION_WINDOW_SECS = 60` — CLAUDE-B may flex 30–120s during impl per pre-authorized widening; final value documented in module README.
5. **Vote rows cascade-delete with their `roadmap_items`**: `ON DELETE CASCADE` on `item_id` — admin-delete of a roadmap item (deferred to P3) drops votes too. No orphan rows.

---

## Contract C15 — Public + admin roadmap HTTP API + 60s aggregator cache

### Public endpoints (no auth required except vote endpoints, which accept JWT bearer OR anon cookie):

```
GET    /api/v1/projects/{project_id}/roadmap?status=<s>&limit=N&offset=K
GET    /api/v1/projects/{project_id}/roadmap/top-voted?limit=N         (N default 10, max 50)
GET    /api/v1/projects/{project_id}/roadmap/items/{slug}
POST   /api/v1/projects/{project_id}/roadmap/items/{slug}/vote          (auth or anon)
DELETE /api/v1/projects/{project_id}/roadmap/items/{slug}/vote          (retract; 60s window)
```

### Admin endpoints (behind `AdminSession`):

```
GET   /api/v1/admin/projects/{project_id}/roadmap                       (list including wontfix)
POST  /api/v1/admin/projects/{project_id}/roadmap/items                 (create from scratch)
PATCH /api/v1/admin/projects/{project_id}/roadmap/items/{slug}          (edit title/body/status)
```

### Response shapes

**List** (`GET /roadmap`):

```json
{
  "items": [
    {
      "slug": "dark-mode",
      "title": "Add dark mode",
      "body": "Many users have asked for ...",
      "status": "planned",
      "vote_count": 42,
      "created_at": "2026-04-01T12:00:00Z",
      "updated_at": "2026-05-01T12:00:00Z"
    }
  ],
  "total": 13,
  "limit": 20,
  "offset": 0,
  "cached_at": "2026-05-14T04:00:00Z"     // null on cold cache
}
```

**Top-voted** (`GET /roadmap/top-voted`):

```json
{
  "items": [
    { "slug": "dark-mode", "title": "...", "status": "planned", "vote_count": 42 }
  ],
  "cached_at": "2026-05-14T04:00:00Z"
}
```

**Detail** (`GET /roadmap/items/{slug}`):

```json
{
  "slug": "dark-mode",
  "title": "Add dark mode",
  "body": "...",
  "status": "planned",
  "vote_count": 42,
  "created_at": "...",
  "updated_at": "..."
}
```

**Vote** (`POST /roadmap/items/{slug}/vote`):

```json
// 200 OK
{ "item_slug": "dark-mode", "voter_mode": "jwt", "cast_at": "..." }

// 409 Conflict (already voted)
{ "error": "AlreadyVoted" }

// 429 Too Many Requests (anon-mode rate limit)
{ "error": "RateLimitExceeded", "retry_after_seconds": 60 }
```

**Retract** (`DELETE /roadmap/items/{slug}/vote`):

```json
// 200 OK
{ "item_slug": "dark-mode", "retracted_at": "..." }

// 403 Forbidden (window expired)
{ "error": "RetractionWindowExpired" }

// 404 if no prior vote from this voter
{ "error": "VoteNotFound" }
```

**Admin create** (`POST /admin/projects/{id}/roadmap/items`):

```json
// request
{ "slug": "...", "title": "...", "body": "...", "status": "considering" }

// 200 OK
{ "slug": "...", "title": "...", "body": "...", "status": "considering",
  "vote_count": 0, "created_at": "...", "updated_at": "..." }
```

**Admin edit** (`PATCH /admin/projects/{id}/roadmap/items/{slug}`):

```json
// request — any subset of fields
{ "title": "...", "body": "...", "status": "planned" }

// 200 OK — full updated record
{ "slug": "...", "title": "...", "body": "...", "status": "planned",
  "vote_count": 42, "created_at": "...", "updated_at": "..." }
```

### Auth-mode resolution at vote time (mirrors submission endpoint)

- `Authorization: Bearer <token>` header present → call `feedbackmonk_jwt::verify_with_leeway(token, project_id, &active_keys, now_unix, state.jwt_iat_leeway_seconds)`. Active keys via `state.signing_keys.list_active(&project_scope)`. On success, `voter_id = claims.sub`, `voter_mode = "jwt"`.
- Header absent → read `X-Feedbackmonk-Anon-Cookie` from request headers (mint via `AnonGate::mint_cookie` if absent + emit `Set-Cookie` response header). Call `AnonGate::token_hash(client_ip, cookie, project_id)`. `voter_id = hex(hash)`, `voter_mode = "anon"`. Pre-check `state.anon_gate.check(...)` for rate-limit; 429 with `Retry-After` if exceeded.

### Voting cache semantics

- **TTL**: 60 seconds (constant `VOTING_CACHE_TTL_SECS = 60`). Configurable later via env if needed.
- **Shape**: `VotingCache { inner: Arc<RwLock<CacheInner>> }`; `CacheInner { per_project: HashMap<Uuid, ProjectCacheEntry>, cached_at: Option<DateTime<Utc>> }`. Per-project entry holds `top_voted: Vec<TopVotedItem>` + `vote_counts: HashMap<RoadmapItemId, i64>`.
- **Refresh tick**: `spawn_refresh_tick(state) -> JoinHandle<()>` spawned from `main.rs::main` after `build_state` succeeds. Fires immediately at startup, then every 60s via `tokio::time::interval`. Aggregates via one SQL query joining `roadmap_items` × `roadmap_votes` grouped by `(project_id, item_id)`.
- **Failure mode**: tick logs WARN via the workspace `feedbackmonk_tracing` scrubber (inherited automatically); cache keeps the prior payload. If cold-start tick fails, public endpoints return `items: [], cached_at: null` — NEVER an error.
- **Read paths**: list + top-voted go through the cache first (fresh-if-cached, else fall through to live DB query for the per-call slice — cache stores TOP-VOTED aggregate; full listing reads `roadmap_items` directly with `vote_count` joined from cache or live). Detail endpoint reads `roadmap_items` directly (single-row, no aggregate).
- **Write paths**: `cast` + `retract` write to DB directly; cache catches up on next 60s tick. Acceptable staleness for a beta-scale voting product (matches gitcellar reference design).

### Listing-filter behavior

- Public `GET /roadmap?status=<s>`: when `status` omitted, returns ALL public-visible statuses (5 in v1). When `status` present, filters to that single status. Multiple-status filtering is deferred (UX picks per phase).
- Public listing returns `vote_count: i64` per item, joined live from `roadmap_votes` (or read from cache for the top-voted endpoint). `cached_at` field reflects when the cache aggregate was last refreshed.
- Admin list mirrors public shape; admin sees all 5 statuses by default (the `is_public_visible` filter is a SQL `WHERE` on the public endpoint only).

---

## Contract C16 — Promote-to-roadmap action

> **Author**: CLAUDE-C (handler body + Q24 byte-for-byte port). **CLAUDE-B authors this signature** so admin-ui type-gen can proceed in parallel. Once CLAUDE-C commits the handler body, the request/response shapes here become byte-equivalent to the live wire contract.

**Endpoint**: `POST /api/v1/admin/feedback/{feedback_id}/promote`
**Auth**: `AdminSession` (existing P1 extractor — `feedbackmonk_session` cookie)

**Request body**:

```json
{
  "slug": "string (1..=80 chars; admin-supplied URL component)",
  "title": "string (1..=200 chars; OPTIONAL — defaults to render_roadmap_title(feedback.body))"
}
```

**Response** (200 OK):

```json
{
  "roadmap_item_id": "uuid",
  "roadmap_item_slug": "kebab-string",
  "source_feedback_id": "FB-XXXXXX",
  "source_status": "duplicate",
  "already_promoted": false
}
```

**Error responses**:

| Status | Body | Cause |
|---|---|---|
| 400 | `{"error": "InvalidCategory", "kind": "<actual kind>"}` | source feedback's `kind` ≠ `feature` |
| 400 | `{"error": "InvalidSlug", "slug": "..."}` | slug fails format check (1..80, kebab-case, ASCII alphanumeric + hyphens) |
| 404 | `{"error": "FeedbackNotFound"}` | feedback_id not in admin's tenant scope |
| 409 | `{"error": "SlugTaken", "slug": "..."}` | `(project_id, slug)` unique-violation on a DIFFERENT origin_feedback_id (i.e. admin tried to re-use a slug from a hand-created roadmap item) |
| 500 | `{"error": "InternalError"}` | DB / transaction failure |

**Hard invariants** (CLAUDE-C asserts in `crates/feedbackmonk-api/src/handlers/promote.rs::tests`):

1. **Q24 — body byte-for-byte**. `render_roadmap_body(message: &str) -> String` is character-for-character ported from `gitcellar-cloud/src/feedback/roadmap_promote.rs::render_roadmap_body` (lines 136–141). Test `q24_roadmap_body_excludes_fb_id_and_username` is byte-for-byte ported from gitcellar lines 340–368 — test name identical, assertions identical. The output framing string `"Posted from a feedback submission.\n\n{}\n\n---\n\nReact with 👍 if you'd like to see this prioritized."` is reproduced verbatim. **Untouchable** — documented in module README. Any future "tidy" refactor of this function or its test is a Q24 regression.

2. **Q24 — title byte-for-byte**. `render_roadmap_title(message: &str) -> String` byte-for-byte ported. Test `q24_roadmap_title_excludes_added_fb_framing` byte-for-byte ported. `TITLE_MAX_CHARS` constant ported as-is from gitcellar.

3. **Category gate**. Source feedback's `kind` MUST be `FeedbackKind::Feature`. Other kinds → `400 InvalidCategory`. (Mirrors gitcellar's `category != "feature_request"` check.)

4. **Idempotency**. Second promote of an already-promoted feedback returns `200 OK` with `already_promoted: true` and the existing roadmap-item slug. Enforced by `roadmap_items.origin_feedback_id UNIQUE` + `RoadmapItemRepo::get_existing_promotion(&scope, origin_feedback_id)` returning `Option<RoadmapItem>` (CLAUDE-B authors this repo method as part of Phase 2).

5. **Atomic status transition**. After roadmap-item INSERT succeeds, source feedback's status transitions to `Duplicate` **in the same DB transaction** via existing `FeedbackRepo::update_status_in_executor` + `FeedbackStatusHistoryRepo::append_in_executor` (Contract C6 Hard Invariant #4). Audit row `reason_note = "promoted to roadmap"`; `duplicate_of_feedback_id = NULL` (this is a roadmap promotion, not a feedback↔feedback dup). Transition origin is captured in the audit `reason_note` text.

6. **PII scrubbing**. Any structured log emitted from the promote handler inherits the workspace-wide `feedbackmonk_tracing` scrubber automatically. `pii-scrub-audit` Probe A enforces no `tracing_subscriber` setup outside `crates/feedbackmonk-tracing/`.

7. **Slug format**. ASCII alphanumeric + hyphens, length 1..=80, no leading/trailing hyphen, no consecutive hyphens. CLAUDE-C may use a small inline regex validator or `validator` crate (already in workspace).

**Handler signature**:

```rust
// crates/feedbackmonk-api/src/handlers/promote.rs (CLAUDE-C authors body)

#[derive(Debug, Clone, Deserialize)]
pub struct PromoteRequest {
    pub slug: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PromoteResponse {
    pub roadmap_item_id: Uuid,
    pub roadmap_item_slug: String,
    pub source_feedback_id: String,      // "FB-XXXXXX"
    pub source_status: FeedbackStatus,   // serializes to "duplicate"
    pub already_promoted: bool,
}

pub async fn promote(
    State(state): State<AppState>,
    session: AdminSession,
    Path(feedback_id): Path<String>,     // short_code "FB-XXXXXX"
    Json(req): Json<PromoteRequest>,
) -> Result<Json<PromoteResponse>, ApiError>;

pub fn promote_router(state: AppState) -> axum::Router {
    axum::Router::new()
        .route(
            "/api/v1/admin/feedback/:feedback_id/promote",
            axum::routing::post(promote),
        )
        .with_state(state)
}

/// Q24 byte-for-byte port from gitcellar `roadmap_promote.rs`. UNTOUCHABLE.
pub fn render_roadmap_title(message: &str) -> String { /* … */ }
pub fn render_roadmap_body(message: &str) -> String { /* … */ }
```

**Pure-function ports** (CLAUDE-C copies verbatim, with no formatting changes):
- `render_roadmap_title(message)` — gitcellar lines 119–129 (port verbatim)
- `render_roadmap_body(message)` — gitcellar lines 136–141 (port verbatim)
- `truncate_with_ellipsis(s, max_chars)` — gitcellar lines 143–150 (port verbatim)
- `TITLE_MAX_CHARS` constant — same value as gitcellar's

---

## TypeScript type mirror (admin-ui starter kit)

> **CLAUDE-C** copies this into `admin-ui/src/shared/types.gen.ts` as the P2 delta atop the P1 mirror. Hand-rolled (no codegen tool); keep in sync with the Rust sources by re-applying when handler signatures change.

```ts
// ────────────────────────────────────────────────────────────────────────────
// P2 — Customer-Facing (Contracts C12–C16). Mirrors `crates/feedbackmonk-core::roadmap`
// + `crates/feedbackmonk-api::handlers::{roadmap, promote, widget_config}`.
// ────────────────────────────────────────────────────────────────────────────

// --- C12: widget config ----------------------------------------------------

export interface WidgetBrand {
  primary_color: string;          // hex, e.g. "#3b82f6"
  logo_url: string | null;
  footer_text: string | null;     // "powered by feedbackmonk" on free tier
}
export interface WidgetConfigResponse {
  project_id: string;
  tenant_id: string;
  display_name: string;
  brand: WidgetBrand;
  auth_modes: Array<"auth" | "anonymous">;
  submission_kinds: Array<"bug" | "feature" | "question" | "other">;
  max_body_chars: number;
}

// --- C13: roadmap item -----------------------------------------------------

export type RoadmapItemStatus =
  | "considering"
  | "planned"
  | "in-progress"
  | "shipped"
  | "wontfix";

export interface RoadmapItem {
  slug: string;
  title: string;
  body: string;
  status: RoadmapItemStatus;
  vote_count: number;
  created_at: string;             // RFC 3339
  updated_at: string;
}

// --- C14: voting ------------------------------------------------------------

export type RoadmapVoterMode = "jwt" | "anon";

export interface VoteResponse {
  item_slug: string;
  voter_mode: RoadmapVoterMode;
  cast_at: string;
}
export interface VoteErrorBody {
  error: "AlreadyVoted" | "RateLimitExceeded" | "VoteNotFound" | "RetractionWindowExpired";
  retry_after_seconds?: number;   // only on RateLimitExceeded
}
export interface RetractResponse {
  item_slug: string;
  retracted_at: string;
}

// --- C15: list + admin ------------------------------------------------------

export interface RoadmapListResponse {
  items: RoadmapItem[];
  total: number;
  limit: number;
  offset: number;
  cached_at: string | null;
}
export interface TopVotedItem {
  slug: string;
  title: string;
  status: RoadmapItemStatus;
  vote_count: number;
}
export interface TopVotedResponse {
  items: TopVotedItem[];
  cached_at: string | null;
}

export interface AdminCreateRoadmapItemRequest {
  slug: string;
  title: string;
  body: string;
  status?: RoadmapItemStatus;     // defaults to "considering"
}
export interface AdminPatchRoadmapItemRequest {
  title?: string;
  body?: string;
  status?: RoadmapItemStatus;
}

// --- C16: promote -----------------------------------------------------------

export interface PromoteRequest {
  slug: string;                   // 1..=80 chars; kebab-case ASCII
  title?: string;                 // defaults to render_roadmap_title(feedback.body)
}
export interface PromoteResponse {
  roadmap_item_id: string;
  roadmap_item_slug: string;
  source_feedback_id: string;     // "FB-XXXXXX"
  source_status: "duplicate";     // always "duplicate" after a successful promote
  already_promoted: boolean;
}
export interface PromoteErrorBody {
  error:
    | "InvalidCategory"
    | "InvalidSlug"
    | "FeedbackNotFound"
    | "SlugTaken"
    | "InternalError";
  kind?: "bug" | "feature" | "question" | "other";  // on InvalidCategory
  slug?: string;                  // on InvalidSlug / SlugTaken
}
```

---

## Worker dependency summary

| Worker | Reads | Writes |
|---|---|---|
| CLAUDE-A | C12 (own) | C12 endpoint + widget JS |
| CLAUDE-B | C13/C14/C15 (own), uses canonical `AnonGate::token_hash` + `feedbackmonk_jwt::verify_with_leeway`, consumes frozen `AdminSession` + `ProjectScope` | migrations 00006/00007, `roadmap.rs`, `roadmap_{items,votes}.rs`, `handlers/roadmap.rs`, `roadmap_voting_cache.rs`, allowlist additions, `.sqlx/` regen |
| CLAUDE-C | C13/C14/C15 (CLAUDE-B authored), C16 (own — body), C6 (P1 frozen state machine) + gitcellar `roadmap_promote.rs` (READ-ONLY reference for Q24 byte-for-byte port) | `handlers/promote.rs` + Q24 tests + module README, `admin-ui/src/pages/roadmap/*`, admin-ui routing, `types.gen.ts` regenerate |

---

## Open / forward-looking notes

- **Voting cache cold-start**: returns `cached_at: null` + empty arrays. Gitcellar pattern proven.
- **Migration numbering**: P2 takes 00006 (roadmap_items) + 00007 (roadmap_votes). P3 (tier enforcement) reserves 00008. CLAUDE-B authors both atomically.
- **`origin_feedback_id` referential integrity**: REFERENCES `feedback(id)` with no ON DELETE — feedback is never hard-deleted in feedbackmonk (only status-transitioned to wontfix/duplicate), so cascade isn't needed.
- **Vote retraction window default**: 60 seconds. CLAUDE-B may flex 30–120s during impl; the final value lives in `crates/feedbackmonk-api/src/handlers/roadmap.rs` as `RETRACTION_WINDOW_SECS` and is documented in the module README.
- **Workspace test count**: P1 closed at 218. Expected P2 close ~250+ (CLAUDE-B adds enum round-trip + cache TTL + vote 409 + retract window + sqlx integration; CLAUDE-C adds 6 Q24 ports + Vitest cases).
- **JoinHandle holding for the refresh tick**: CLAUDE-B's `spawn_refresh_tick` returns a `JoinHandle<()>` so the binary can `.abort()` it during shutdown — main.rs may drop it deliberately if the binary doesn't need graceful shutdown of the tick (which it doesn't — process exit is fine).

---

## Stage 1 deliverable checklist (CLAUDE-B's own — fills in as Phase 0..6 progress)

- [ ] Contract-freeze doc committed (this file) — ping `[B → A,C] Contracts frozen at <sha>` to `channels/messages.md`
- [ ] migrations 00006 + 00007 apply + rollback cleanly
- [ ] `crates/feedbackmonk-core/src/roadmap.rs` + lib.rs re-exports + enum round-trip tests GREEN
- [ ] `crates/feedbackmonk-repository/src/roadmap_items.rs` + `roadmap_votes.rs` + lib.rs re-exports
- [ ] `crates/feedbackmonk-api/src/roadmap_voting_cache.rs` with TTL semantics tests GREEN
- [ ] `crates/feedbackmonk-api/src/handlers/roadmap.rs` public + admin routers, wired into `main.rs::build_app`
- [ ] `state.rs` extended with `voting_cache` + `roadmap_items` + `roadmap_votes` Arc<dyn> fields (pre-authorized AppState widening per P1 precedent)
- [ ] Allowlist: `SqlxRoadmapItemRepo::new` + `SqlxRoadmapVoteRepo::new` structural-mirror entries
- [ ] `cargo sqlx prepare --workspace` regen + commit `.sqlx/`
- [ ] `cargo build` + `cargo clippy --workspace --all-targets -- -D warnings` GREEN
- [ ] `cargo test --workspace` GREEN; new tests: enum round-trip, cache TTL via `tokio::time::pause`, cold-start empty, tick failure preserves last value, happy-path vote, duplicate-vote 409, retract within 60s, retract after 60s 403
- [ ] `multi-tenant-isolation-check` oracle GREEN with allowlist additions
- [ ] `pii-scrub-audit` oracle GREEN

---

*Authored by CLAUDE-B, collab-20260514-035703, 2026-05-14T04:05Z. Ratify in `channels/decisions.md` once LD has reviewed.*
