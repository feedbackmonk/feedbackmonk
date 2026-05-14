# P1 Stage 1 → Stage 2 Handoff — Frozen Contracts

**Predecessor**: `docs/planning/plans/20260513T231115-feedbackmonk-p1-closes-the-loop.md` (Stage 1 plan)
**Predecessor (P0)**: `docs/planning/handoffs/stage1-to-stage2.md` (P0 handoff — still authoritative for everything P0 froze; this doc adds P1 deltas only)
**Stage 1 commit**: `b9a672a` + the upcoming P1-Stage-1 commit
**Generated**: 2026-05-13T23:55:00Z
**Authored by**: Stage 1 orchestrated worker (P1 Foundation)

> Distinct from `docs/planning/handoffs/stage1-to-stage2.md` (P0-era). This
> file documents the P1-Stage-1 surface that Stage 2 Worker A (backend) and
> Worker B (frontend) consume as a frozen library surface.

## What Stage 2 inherits as frozen

| Surface | Where it lives | Stable for Stage 2 |
|---|---|---|
| 20-pattern PII scrubber + `install_global_subscriber` | `crates/feedbackmonk-tracing/` | Yes. Pattern set is hash-locked by `.claude/oracles/pii-scrub-audit/expected_hash.txt`. New emit paths inherit scrubbing automatically. |
| `FeedbackStatus` enum + `legal_transitions_from` + `TransitionError` | `crates/feedbackmonk-core/src/status.rs` | Yes. Variants frozen; transition table frozen. Worker A's `transition_status` consumes; Worker B's UI renders `legal_transitions_from(current_status)`. |
| `feedback_status_history` table + `feedback.status` column | `migrations/00003_feedback_status_history.sql` | Yes. Column added with CHECK constraint on the six canonical values. |
| Tenant brand columns + backfilled defaults | `migrations/00005_tenant_email_brand.sql` | Yes. `unsubscribe_url` nullable; others NOT NULL with sensible defaults from `tenants.email` local-part. |
| `FeedbackRepo::list_for_admin` + `get_with_history` | `crates/feedbackmonk-repository/src/feedback.rs` | Yes. Pre-authorized widenings spelled out below. |
| `FeedbackStatusHistoryRepo` (`append`, `list_for_feedback`) | `crates/feedbackmonk-repository/src/feedback_status_history.rs` | Yes. Stage 2 Worker A composes a same-transaction variant atop `append` for atomicity. |
| `TenantRepo::get_brand` / `update_brand` | `crates/feedbackmonk-repository/src/tenants.rs` | Yes. Pre-authorized brand-field additions: optional `Option<String>` columns. |
| `EmailTenantBrand` value type | `crates/feedbackmonk-repository/src/tenants.rs` | Yes. `sender_display_name` is computed (`"{brand_name} via feedbackmonk"`); never read from a DB column. |
| `pii-scrub-audit` oracle (CI gate) | `.claude/oracles/pii-scrub-audit/` | Yes. Probe A + Probe B; pattern-set hash refresh requires a deliberate `expected_hash.txt` commit. |

## Stage 1 deviations from the brief (documented)

1. **Feedback `status` column added in migration 00003.** The brief enumerated
   only the `feedback_status_history` table for migration 00003. Adding the
   `status` column to `feedback` in the same migration keeps the
   "status workflow" feature cohesive in one migration and removes the need
   for Stage 2 Worker A to widen Stage 1's repository surface mid-flight.
   Default `'submitted'`; CHECK constraint on the six canonical values.
2. **`CANONICAL_PATTERNS` is a 3-tuple `(name, regex, replacement)`, not
   2-tuple as the brief illustrated.** GitCellar's source uses a `Rule
   { re, replacement }` struct with the name only in comments; promoting
   `name` into the slice gives the oracle a stable diagnostic label and
   makes re-orderings detectable in the SHA-256 even if pattern + replacement
   are unchanged.
3. **Layer chokepoint at the WRITE boundary, not a custom `Layer` impl.**
   The brief calls for a custom `tracing_subscriber::Layer` applying scrub
   to event field values. We chokepoint at the bytes a formatter has
   chosen to emit (`MakeWriter`-backed `ScrubbingMakeWriter`); same
   end-user property (every line passes through scrub), more stable seam
   (no field-visitor brittleness). The `pii-scrub-audit` oracle's Probe A
   pattern still forbids `impl Layer<...> for ...` outside the crate.
4. **`manifest.toml` and `manifest.json` both ship.** The brief specifies
   `manifest.toml`; the existing `multi-tenant-isolation-check` oracle
   uses `manifest.json`. Both files exist; `manifest.json` is the runtime
   metadata; `manifest.toml` is a TOML mirror so the brief's literal naming
   is satisfied. Drift between the two is a docs bug.
5. **`pii-scrub-audit` Probe A regex tightened.** The brief's loose
   `impl.*Layer.*for` would false-positive on `TraceLayer::new_for_http`
   in tower-http usage; the tightened pattern requires
   `impl ... Layer<...> for ...` block opener syntax.

## Pre-authorized self-mediation widenings (PODS Coordination Protocol)

Per the P1 plan's §PODS Coordination Protocol → Pre-authorized widenings:

| Surface | Allowed widening shape | Tag in `channels/decisions.md` |
|---|---|---|
| `FeedbackRepo::list_for_admin` | Additional **optional** filter parameters (e.g., `?author=...`, `?since=...`), defaulting to no-filter. Method signature gains additional `Option<T>` args. | `self_mediated=true; ratification_pending=true; matches_spec_at=docs/planning/handoffs/p1-stage1-to-stage2.md#pre-authorized` |
| `EmailTenantBrand` | Additional `Option<String>` brand fields (e.g., a future logo URL). Schema-backwards-compatible (new ALTER TABLE migration adding nullable columns). | same |
| `FeedbackListItem` | Additional read-only fields (e.g., `reply_count` becomes meaningful when Stage 2 Worker A's `feedback_replies` table lands; new fields beyond that are also allowed). | same |
| `FeedbackStatusHistoryRepo::append` | New **method overload** taking an executor (`&mut PgConnection` / `&mut Transaction`) so the transition handler can compose same-transaction writes with `feedback.status` UPDATE. Original `&self.pool` shape stays for non-transactional callers. | same |
| Admin UI component prop shapes | Worker B internal; freely widen. | (n/a — Worker B-only) |

**NOT pre-authorized** (require LD halt via `channels/alerts.md`):
- New entries in `.claude/oracles/multi-tenant-isolation-check/allowlist.toml`.
- Non-backwards-compatible JSON shape changes on any C7/C8 endpoint.
- New `FeedbackStatus` variants (would break Contract C6's frozen state machine).
- Pattern-set changes in `crates/feedbackmonk-tracing/src/scrubber.rs` (the
  `pii-scrub-audit` oracle is the gate; a deliberate `expected_hash.txt`
  refresh commit IS the ratification — but it's a per-change LD decision).

---

## Contract C6 — Status Workflow State Machine

Verbatim from the P1 plan §Interface Contracts, supplemented with the actual
Stage 1 Rust API.

```rust
// crates/feedbackmonk-core/src/status.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FeedbackStatus {
    #[default]
    Submitted,
    Triaged,
    InProgress,   // serialises as "in-progress"
    Shipped,
    WontFix,      // serialises as "wontfix"
    Duplicate,
}

impl FeedbackStatus {
    pub fn as_db_str(self) -> &'static str { /* kebab-case */ }
    pub fn from_db_str(s: &str) -> Self    { /* lenient; unknown -> Submitted */ }
}

/// Legal transitions. Returned ordering is stable for UI rendering.
/// Source: gitcellar-cloud/src/feedback/db.rs (state-machine ported).
pub fn legal_transitions_from(s: FeedbackStatus) -> &'static [FeedbackStatus] {
    use FeedbackStatus::*;
    match s {
        Submitted  => &[Triaged, WontFix, Duplicate],
        Triaged    => &[InProgress, WontFix, Duplicate, Submitted /* admin revert */],
        InProgress => &[Shipped, WontFix, Duplicate, Triaged /* admin revert */],
        Shipped    => &[],                  // terminal
        WontFix    => &[Submitted],         // re-open
        Duplicate  => &[Submitted],         // un-merge
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransitionError {
    IllegalTransition { from: FeedbackStatus, to: FeedbackStatus },
    DuplicateRequiresTarget,
    DuplicateTargetMissing,
    DuplicateSelfReference { feedback_id: FeedbackId },
}
```

**Worker A's transition function signature** (NOT IMPLEMENTED in Stage 1; freeze only):

```rust
// crates/feedbackmonk-api/src/handlers/admin_feedback.rs  (Worker A writes this)

pub async fn transition_status(
    scope: &ProjectScope,
    feedback_id: &FeedbackId,
    to: FeedbackStatus,
    reason_note: Option<&str>,
    duplicate_of: Option<&FeedbackId>,
    transitioned_by: uuid::Uuid,
) -> Result<TransitionOutcome, TransitionError>;
```

**Hard invariants (Worker A unit-tests with named tests)**:

1. Illegal transitions fail with `TransitionError::IllegalTransition` BEFORE any DB write.
2. `to == Duplicate` requires `duplicate_of: Some(_)` or fails with `DuplicateRequiresTarget`.
3. `duplicate_of` must reference a feedback row in the SAME `ProjectScope` (Stage 1's
   `FeedbackStatusHistoryRepo::append` ALREADY enforces this; reuse that gate).
4. Audit row is appended in the SAME DB transaction as the status column update.
   Stage 1 ships `append(&self, scope, ..., transitioned_by)` against `&self.pool`;
   Worker A's transition handler composes a same-transaction variant via the
   pre-authorized `Executor`-aware overload (see Pre-authorized widenings above).
5. `from == to` (no-op transition) returns `Err(IllegalTransition)` rather than
   silently succeeding.

The DB-side CHECK constraint
`feedback_status_history_no_self_duplicate` (`duplicate_of_feedback_id <> feedback_id`)
is belt-and-braces for invariant #3.

---

## Contract C7 — Admin Status Transition + Reply HTTP API

```http
POST /api/v1/admin/feedback/{feedback_id}/transition
Cookie: feedbackmonk_session=<value>           # NOT feedbackmonk_admin_session — see Contract C11 below
Content-Type: application/json

Request:
{
  "to_status": "triaged" | "in-progress" | "shipped" | "wontfix" | "duplicate" | "submitted",
  "reason_note": "string?",
  "duplicate_of": "FB-XXXXXX"?
}

200 OK:
{
  "feedback_id": "FB-XXXXXX",
  "from_status": "submitted",
  "to_status": "triaged",
  "transitioned_at": "2026-05-13T22:00:00Z",
  "audit_id": "uuid",
  "email_queued": true
}

409 Conflict (illegal transition or missing/cross-tenant duplicate target):
{ "error": "IllegalTransition" | "DuplicateRequiresTarget" | "DuplicateTargetMissing" | "DuplicateSelfReference",
  "from_status": "...", "to_status": "..." }

401 if session invalid; 403 if tenant not verified; 404 if feedback_id not in scope.
```

```http
POST /api/v1/admin/feedback/{feedback_id}/reply
Cookie: feedbackmonk_session=<value>
Content-Type: application/json

Request:
{
  "body": "string, 1..16384 chars",
  "visibility": "public" | "internal"
}

200 OK:
{
  "reply_id": "uuid",
  "feedback_id": "FB-XXXXXX",
  "visibility": "public",
  "created_at": "...",
  "email_queued": true   // false if visibility == "internal" or submitter has no email
}
```

Body length: same 1..16384 char range as the submission body (mirrors P0 schema).
Stage 2 Worker A's migration 00004 (the `feedback_replies` table) owns the
storage shape.

---

## Contract C8 — Admin Feedback List + Get HTTP API

```http
GET /api/v1/admin/feedback?status=triaged&limit=20&offset=0
Cookie: feedbackmonk_session=<value>

200 OK:
{
  "items": [
    {
      "feedback_id": "FB-XXXXXX",
      "kind": "bug" | "feature" | "question" | "other",
      "status": "triaged",
      "body_excerpt": "first 200 chars of body",
      "submitted_at": "...",
      "submitter_label": "alice@example.com" | "anonymous" | "anonymous (email: bob@example.com)",
      "reply_count": 0
    },
    ...
  ],
  "total": 47,
  "limit": 20,
  "offset": 0
}
```

Backing repository call (Stage 1):
`FeedbackRepo::list_for_admin(&scope, status_filter: Option<FeedbackStatus>, limit: u32, offset: u32)`
returns `(Vec<FeedbackListItem>, total: u32)`.

`FeedbackListItem` (Stage 1 shape — Worker A may widen per Pre-authorized widenings):

```rust
pub struct FeedbackListItem {
    pub feedback_id: FeedbackId,
    pub kind: FeedbackKind,
    pub status: FeedbackStatus,
    pub body_excerpt: String,
    pub submitted_at: chrono::DateTime<chrono::Utc>,
    pub submitter_email: Option<String>,   // None for fully-anonymous submissions
    pub is_anonymous: bool,
    pub reply_count: i64,                  // hard-zero in Stage 1; real once Worker A wires feedback_replies
}
```

Worker A's HTTP layer maps `submitter_email` + `is_anonymous` → `submitter_label`
(the response JSON's `submitter_label` is a string formatted by the handler;
not a column on the row).

```http
GET /api/v1/admin/feedback/{feedback_id}
Cookie: feedbackmonk_session=<value>

200 OK:
{
  "feedback_id": "FB-XXXXXX",
  "kind": "bug",
  "status": "triaged",
  "body": "full body string; admin sees the unredacted text",
  "submitted_at": "...",
  "submitter": { "kind": "authenticated" | "anonymous",
                 "sub": "...?", "email": "...?", "name": "...?" },
  "external_metadata": {...}?,
  "status_history": [
    { "from_status": "submitted", "to_status": "triaged",
      "reason_note": "...?", "duplicate_of_feedback_id": "FB-XXXXXX?",
      "transitioned_by": "admin@tenant.com", "transitioned_at": "..." },
    ...
  ],
  "replies": [
    { "reply_id": "uuid", "body": "...", "visibility": "public",
      "author": "admin@tenant.com", "created_at": "..." },
    ...
  ]
}
```

Backing repository calls:
- `FeedbackRepo::get_with_history(&scope, &feedback_id) -> (Feedback, Vec<StatusHistoryRow>)` (Stage 1)
- Stage 2 Worker A adds reply enumeration (via `FeedbackReplyRepo::list_for_feedback`) and the
  `transitioned_by` UUID → email-label lookup.

`status_history` returned by `get_with_history` is newest-first (matches the
DB index `feedback_status_history_feedback_idx (feedback_id, transitioned_at DESC)`).

**Invariant**: admin feedback responses include the full body unredacted.
The PII scrubber (Contract C9) applies to **logs**, not to admin-UI response
bodies — admins must be able to read what users submitted. Worker B's
frontend renders the body as plain-text (no HTML interpretation; no link
auto-conversion) to defend against stored-XSS from submitter content.

---

## Contract C9 — PII Scrubber Layer

**Public API**:

```rust
// crates/feedbackmonk-tracing/src/lib.rs

pub fn install_global_subscriber(level: LogLevel, format: LogFormat) -> Result<(), TracingError>;

pub fn scrub(input: &str) -> String;   // re-exported from scrubber.rs

pub use SharedBufferScrubbing;          // test-only MakeWriter factory for per-test subscribers
```

**LogLevel**: `Trace | Debug | Info | Warn | Error` (default `Info`).
**LogFormat**: `Plain | Json` (default `Json`).

**`RUST_LOG` overrides** `level` when set — parsed as a full `EnvFilter`
directive (matches the P0 baseline).

**Internal pattern set**:

```rust
// crates/feedbackmonk-tracing/src/scrubber.rs

pub(crate) static CANONICAL_PATTERNS: &[(&str, &str, &str)] = &[
    ("dsn",                  r"https?://[a-f0-9]{32,}@[a-zA-Z0-9.\-]+/\d+", "[dsn]"),
    ("bearer_token",         r"(?i)bearer\s+[A-Za-z0-9_\-\.=:+/]{20,}",     "Bearer [token]"),
    ("jwt",                  r"\beyJ[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{5,}\.[A-Za-z0-9_\-]{5,}\b", "[jwt]"),
    ("email",                r"\b[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}\b", "[email]"),
    ("windows_user_path",    r"[A-Za-z]:\\Users\\[^\\\s]+",                 "[user-path]"),
    ("mac_user_path",        r"/Users/[^/\s]+",                              "/Users/[user]"),
    ("linux_user_path",      r"/home/[^/\s]+",                               "/home/[user]"),
    ("windows_drive_path",   r"[A-Za-z]:\\[A-Za-z][\w]*\\[^\\\s]+",          "[drive-path]"),
    ("user_id_uuid",         r"user_id[=:]\s*[a-f0-9]{8}-...",               "user_id=[uuid]"),
    ("uuid",                 r"\b[a-f0-9]{8}-[a-f0-9]{4}-...",               "[uuid]"),
    ("machine_id",           r"machine_id[=:]\s*\S+",                        "machine_id=[redacted]"),
    ("forge_username",       r"\busername[=:]\s*\S+",                        "username=[redacted]"),
    ("repo_path",            r"\brepo[=:]\s*[\w\-]+/[\w\-]+\b",              "repo=[redacted]"),
    ("hash64",               r"\b[A-Fa-f0-9]{64}\b",                         "[hash64]"),
    ("hash40",               r"\b[a-f0-9]{40}\b",                            "[hash40]"),
    ("s3_access_key",        r"\bAKIA[0-9A-Z]{16}\b",                        "[s3-access-key]"),
    ("b2_app_key",           r"\bK\d{3}_[a-zA-Z0-9_\-]{30,}\b",              "[b2-app-key]"),
    ("b2_key_id",            r"\bK[0-9a-fA-F]{27,}\b",                       "[b2-key-id]"),
    ("ipv4",                 r"\b(?:25[0-5]|...)\b",                         "[ip]"),
    ("ipv6",                 r"(?i)(?:[0-9a-f]{1,4}:){7}...",                "[ipv6]"),
];
```

Full source: `crates/feedbackmonk-tracing/src/scrubber.rs`. ORDER MATTERS (see
module docs). SHA-256 of the canonical serialisation
`name\tregex\treplacement\n` per row is locked in
`.claude/oracles/pii-scrub-audit/expected_hash.txt`.

**Hard invariants** (Stage 1 oracle + tests):

1. SHA-256 of `CANONICAL_PATTERNS` matches `expected_hash.txt`.
2. Each pattern has at least one positive-match test + one near-miss-no-match test.
3. No `tracing_subscriber::fmt(`, `tracing_subscriber::registry(`, or
   `impl Layer<...> for ...` outside `crates/feedbackmonk-tracing/`.
4. `scrub(scrub(x)) == scrub(x)` (idempotent — replacement sigils never
   match any pattern).

---

## Contract C10 — Email Template Tenant-Brand Parameters

```rust
// crates/feedbackmonk-repository/src/tenants.rs

pub struct EmailTenantBrand {
    pub brand_name: String,
    pub email_subject_prefix: String,
    pub support_email: String,
    pub unsubscribe_url: Option<String>,
    pub footer_signature: String,
    pub sender_display_name: String,   // computed: "{brand_name} via feedbackmonk"
}

impl EmailTenantBrand {
    pub fn from_db(
        brand_name: String,
        email_subject_prefix: String,
        support_email: String,
        unsubscribe_url: Option<String>,
        footer_signature: String,
    ) -> Self;   // derives sender_display_name
}

// On TenantRepo:
async fn get_brand(&self, scope: &TenantScope) -> Result<EmailTenantBrand>;
async fn update_brand(&self, scope: &TenantScope, brand: &EmailTenantBrand) -> Result<()>;
```

**Decision documented**: brand fields are reached via `get_brand(&scope)`, not by
widening the existing pre-auth `find_by_email` surface. Rationale: `find_by_email`
is allow-listed as a pre-authentication exception (no scope at lookup time); exposing
brand columns there would unnecessarily widen the pre-auth surface. `get_brand`
requires a `&TenantScope`, matching the rest of the post-auth surface and keeping
the `multi-tenant-isolation-check` oracle's Probe B clean.

**Subject format** (Worker A's `email/templates.rs` renders this):

```
[{email_subject_prefix} #FB-{display_id}] {short_subject}
```

**Footer template** (plain-text per FR-FBR-09 "Status emails (plain-text)"):

```
{footer_signature}
---
You are receiving this because you submitted feedback to {brand_name}.
Reply to this email or contact {support_email}.
{unsubscribe_url_line_if_some}
```

Schema (migration 00005, already applied):

- `brand_name` NOT NULL — defaults to email local-part on backfill + new signup.
- `email_subject_prefix` NOT NULL — same default.
- `support_email` NOT NULL — defaults to `tenants.email`.
- `unsubscribe_url` NULLABLE — None ⇒ no footer line.
- `footer_signature` NOT NULL — defaults to `"— The {local-part} team"`.

`TenantRepo::create` (the signup path) has been updated to populate these
columns inline so new tenants land with the same default shape as backfilled
rows.

---

## Contract C11 — Admin Session Cookie (carry-state from P0)

> **The P1 plan referenced this cookie as `feedbackmonk_admin_session`. The
> actual P0 cookie name is `feedbackmonk_session`.** Worker A and Worker B
> use `feedbackmonk_session` exactly. The cookie has admin privileges (all
> admin endpoints sit behind the `AdminSession` extractor); there is no
> separate non-admin session cookie in P0/P1.

Source of truth: `crates/feedbackmonk-api/src/auth/session.rs`.

| Property | Value |
|---|---|
| Cookie name | `feedbackmonk_session` |
| Constant | `feedbackmonk_api::auth::session::SESSION_COOKIE_NAME` |
| Format | `<b64url(tenant_uuid_bytes_16)>.<b64url(issued_unix_be_8)>.<b64url(hmac_sha256_32)>` (URL-safe base64, no padding, `.` separators) |
| HMAC input | concat(tenant_uuid_16, issued_unix_be_8) |
| HMAC key | `FEEDBACKMONK_SESSION_SECRET` (64 hex chars → 32 raw bytes) |
| Max-Age | 7 × 24 × 60 × 60 = 604800 seconds (7 days) |
| HttpOnly | `true` |
| Secure | `true` (HTTPS-only on the wire — dev runs over HTTP but browsers tolerate Secure cookies on `localhost`) |
| SameSite | `Lax` |
| Path | `/` |
| Issued by | `issue_session_cookie(tenant_id, &secret)` (call this after a successful login or after verify-email) |

**Extractor**: `AdminSession` (an `axum::extract::FromRequestParts<AppState>` impl).
On every admin endpoint, declare an `AdminSession` extractor parameter and the
extractor will:

1. Reject (401) if the cookie is missing or HMAC-invalid.
2. Reject (401) if the cookie is expired or more than 60 seconds future-dated.
3. Resolve `tenant_id` → `TenantScope` via `TenantRepo::scope_for` (rejects 401
   if the tenant row has been deleted).
4. Reject (403) if the tenant is still in pending-verification state
   (`verified_at IS NULL`).
5. Return `AdminSession { scope: TenantScope }` on success.

```rust
// Worker A — admin endpoint shape
pub async fn list_admin_feedback(
    session: AdminSession,                                // ← extractor
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Result<Json<ListResponse>, ApiError> { ... }
```

**Worker B** (frontend): every admin-page request includes the cookie via
`axios.defaults.withCredentials = true` (or `fetch(..., { credentials: "include" })`).
401 responses redirect to `/login`. The cookie is set by Worker A's existing
verify-email handler (P0 carry-state) and by Worker A's future `/api/v1/admin/login`
handler (Stage 2 work — login endpoint may already exist in P0; verify before
writing a duplicate).

**Hard invariants** (already shipped by P0; not Stage 1's authority to change):

- Tampering any byte of the cookie value rejects (HMAC mismatch).
- Wrong secret rejects.
- Expired cookies reject.
- Future-dated cookies (>60s) reject.
- Malformed cookies (missing parts, wrong base64, wrong byte lengths) reject.

---

## TypeScript type mirror (Worker B starting kit)

Worker B should create `admin-ui/src/shared/types.gen.ts` (the file name in the
brief was `types.gen.ts.example`; Worker B may rename to drop the `.example`
suffix). The Rust-side response struct shapes for Contracts C7 + C8 map to:

```ts
// admin-ui/src/shared/types.gen.ts
//
// Hand-rolled mirror of the backend response shapes. KEEP IN SYNC with
// `crates/feedbackmonk-api/src/handlers/admin_feedback.rs` (Stage 2 Worker A).
//
// Stage 3 e2e includes a Vitest test asserting an admin-feedback fetch
// response parses against these types. Drift between Rust + TS surfaces
// here.

// Status workflow — Contract C6
export type FeedbackStatus =
  | "submitted"
  | "triaged"
  | "in-progress"
  | "shipped"
  | "wontfix"
  | "duplicate";

export const LEGAL_TRANSITIONS: Record<FeedbackStatus, FeedbackStatus[]> = {
  submitted:    ["triaged", "wontfix", "duplicate"],
  triaged:      ["in-progress", "wontfix", "duplicate", "submitted"],
  "in-progress":["shipped", "wontfix", "duplicate", "triaged"],
  shipped:      [],                       // terminal
  wontfix:      ["submitted"],            // re-open
  duplicate:    ["submitted"],            // un-merge
};

// Contract C8 — list response
export interface FeedbackListItem {
  feedback_id: string;          // "FB-XXXXXX"
  kind: "bug" | "feature" | "question" | "other";
  status: FeedbackStatus;
  body_excerpt: string;         // first 200 chars
  submitted_at: string;         // RFC 3339
  submitter_label: string;      // formatted server-side; never raw email-only
  reply_count: number;
}
export interface FeedbackListResponse {
  items: FeedbackListItem[];
  total: number;
  limit: number;
  offset: number;
}

// Contract C8 — get-with-history response
export interface StatusHistoryEntry {
  from_status: FeedbackStatus;
  to_status: FeedbackStatus;
  reason_note: string | null;
  duplicate_of_feedback_id: string | null;   // "FB-XXXXXX" or null
  transitioned_by: string;                   // server formats UUID → email-label
  transitioned_at: string;                   // RFC 3339
}
export interface ReplyEntry {
  reply_id: string;
  body: string;
  visibility: "public" | "internal";
  author: string;
  created_at: string;
}
export interface FeedbackSubmitter {
  kind: "authenticated" | "anonymous";
  sub?: string;
  email?: string;
  name?: string;
}
export interface FeedbackDetail {
  feedback_id: string;
  kind: "bug" | "feature" | "question" | "other";
  status: FeedbackStatus;
  body: string;                              // full body, unredacted (Contract C8 invariant)
  submitted_at: string;
  submitter: FeedbackSubmitter;
  external_metadata?: Record<string, unknown>;
  status_history: StatusHistoryEntry[];
  replies: ReplyEntry[];
}

// Contract C7 — transition request/response
export interface TransitionRequest {
  to_status: FeedbackStatus;
  reason_note?: string;
  duplicate_of?: string;                     // "FB-XXXXXX"
}
export interface TransitionResponse {
  feedback_id: string;
  from_status: FeedbackStatus;
  to_status: FeedbackStatus;
  transitioned_at: string;
  audit_id: string;
  email_queued: boolean;
}
export type TransitionErrorCode =
  | "IllegalTransition"
  | "DuplicateRequiresTarget"
  | "DuplicateTargetMissing"
  | "DuplicateSelfReference";
export interface TransitionErrorBody {
  error: TransitionErrorCode;
  from_status?: FeedbackStatus;
  to_status?: FeedbackStatus;
}

// Contract C7 — reply request/response
export interface ReplyRequest {
  body: string;                              // 1..16384 chars
  visibility: "public" | "internal";
}
export interface ReplyResponse {
  reply_id: string;
  feedback_id: string;
  visibility: "public" | "internal";
  created_at: string;
  email_queued: boolean;
}
```

Render `legal_transitions_from` UI choices via:

```ts
const choices = LEGAL_TRANSITIONS[currentStatus];   // never offer outside this list
```

The state machine's UI rendering invariant (Worker B's responsibility):
**only buttons for `LEGAL_TRANSITIONS[currentStatus]` are visible**.
Illegal transitions are never possible from the UI; the backend's 409 fallback
is belt-and-braces.

---

## Open / forward-looking notes for Worker A and Worker B

- **Migration numbering**: Stage 1 took 00003 (history + status column) and
  00005 (tenant brand). 00004 is **reserved for Worker A's
  `feedback_replies` table** so commits stack without renumbering merges.
- **`tenant_users` table**: Stage 1 stores `transitioned_by` as a bare UUID
  with NO foreign key. Worker A introduces a `tenant_users` table in a future
  migration; the FK can be added later via `ALTER TABLE … ADD CONSTRAINT …
  NOT VALID; VALIDATE CONSTRAINT …` for online safety. Worker A's
  HTTP-layer label lookup joins through this table when it exists; until
  then, the JSON exposes a raw UUID (Worker B can render `(unknown admin)`
  as fallback).
- **`FEEDBACKMONK_LOG_FORMAT=text` vs `=json`**: unchanged from P0; defaults to
  `json`. Worker A's tests should not call `install_global_subscriber` —
  they should compose per-test subscribers (or use `SharedBufferScrubbing`
  if asserting scrubbed output) so the global subscriber doesn't conflict.
- **e2e regression**: Stage 1 did NOT re-run `scripts/e2e-p0-curl.sh`
  (out of scope for an orchestrated-worker exit; orchestrator owns final
  regression check). Build, unit tests, integration tests, and both oracles
  all PASS — and the only main.rs change is the tracing-init swap which
  is exercised by the integration-test suite that boots a router.
- **Reply rich-text vs plain-text**: plain-text only, per the P1 plan's
  Deferred Decisions resolution. Stage 2 Worker B's `ReplyComposer` is a
  `<textarea>` with no rich-text toolbar.
- **Display ID**: `FB-NNNNNN` zero-padded sequential per project (P1 plan
  Deferred Decisions). Worker A may need to add a `display_id` BIGINT
  column + per-project sequence; the current P0 `short_code` is the
  random alphanumeric form `FB-XXXXXX`. Whether to add a numeric form is
  Worker A's call — the brief's "`FB-NNNNNN`" wording in C7/C8 refers to
  the existing `short_code` literal, not a new numeric form. If Worker A
  decides to switch, mirror the change in `admin-ui/src/shared/types.gen.ts`
  comments.
- **CORS / static-file serving for the admin-ui**: Stage 2 Worker B's Vite
  dev server proxies `/api` → `http://localhost:14304`; same-origin in
  prod (admin-ui builds to `admin-ui/dist/` and is served by `feedbackmonk-api`
  via a `tower-http::services::ServeDir`). The CORS layer is deferred to
  Stage 3 e2e integration per the P1 plan §PODS Coordination Protocol.

---

## Stage 1 deliverable checklist (state at handoff)

- [x] `.claude/oracles/pii-scrub-audit/` exists with `oracle.py`,
      `oracle.sh`, `manifest.json` (+ TOML mirror), `expected_hash.txt`,
      `README.md`. Oracle returns PASS against the scrubber crate.
- [x] `crates/feedbackmonk-tracing/` ships clean (`cargo build --workspace` GREEN).
- [x] `cargo clippy --workspace --all-targets -- -D warnings` GREEN.
- [x] 20 canonical patterns match GitCellar byte-for-byte; SHA-256 locked.
- [x] `migrations/00003_feedback_status_history.sql` applies cleanly.
- [x] `migrations/00005_tenant_email_brand.sql` applies cleanly with
      backfilled defaults.
- [x] `FeedbackRepo::list_for_admin` + `get_with_history` implemented +
      cross-tenant negative tests.
- [x] `FeedbackStatusHistoryRepo` trait + `SqlxFeedbackStatusHistoryRepo`
      impl with `append` + `list_for_feedback`.
- [x] `TenantRepo::get_brand` + `update_brand` + `EmailTenantBrand`.
- [x] `bin/feedbackmonk-api/src/main.rs` wired to
      `feedbackmonk_tracing::install_global_subscriber`.
- [x] `cargo test --workspace` GREEN (Stage 1 commit's snapshot:
      185 tests passing — see `ltads/execution/development-complete.md`).
- [x] `multi-tenant-isolation-check` oracle STILL GREEN.
- [x] `docs/planning/handoffs/p1-stage1-to-stage2.md` (this file).
- [x] `crates/feedbackmonk-tracing/README.md` follows ULADP module standard.

Items the brief flagged that this Stage explicitly DOES NOT do (Stage 2 scope):

- [ ] Status-transition handler logic (Worker A)
- [ ] Email templates (Worker A)
- [ ] Admin UI (Worker B)
- [ ] Reply endpoints + `feedback_replies` table (Worker A, migration 00004)
- [ ] `scripts/e2e-p0-curl.sh` regression run (orchestrator owns at arc level)
