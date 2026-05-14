# Execution Plan — Feedbackr P1 — Closes the Loop
**Source**: /0-uldf-ldis-plan
**Generated**: 2026-05-13T23:11:15Z
**Task**: Build the P1 (Closes the Loop) slice of Feedbackr v1 — status workflow state machine + audit history + admin UI (React+Vite) + status emails + PII scrubber with drift-detection oracle. Exit gate: "admin can list feedback, click drawer, transition status (with audit row), reply public + internal; submission → confirmation email → status-change email → public-reply email all observed via Mailpit; PII scrubber active on all server logs and oracle green."
**Strategy**: STAGED (3 stages; Stage 2 is the 2-worker PODS fan-out; Stages 1 and 3 are single-agent)
**Arc Plan**: `docs/planning/plans/20260513T185711-feedbackr-v1-build-arc.md`
**P0 Reference Plan**: `docs/planning/plans/20260513T210133-feedbackr-p0-foundation.md` (structural template)
**Spec Source**: `docs/specs/SPECIFICATION.md` (FR-FBR-07, FR-FBR-08, FR-FBR-09, FR-FBR-10) + `docs/specs/DECISIONS.md` (DEC-FBR-04 carry, DEC-FBR-IMPL-01..04, DEC-PODS-001/002)
**Carry-state**: `docs/specs/DISCOVERIES.md` D-FBR-01..09 (pre-auth-allowlist as repeatable mechanism; fixture-corpus-first; in-memory-rate-limiter v1.1 graduation; axum ConnectInfo gotcha)
**Autonomy**: `autopilot:continuous` arc grant — valid until 2026-05-14T21:06:21Z; BoundConsent inherits; spans P0→P4.

---

## Strategy Rationale

### Why STAGED with three stages (mirrors P0's proven shape)

P0's three-stage STAGED pattern delivered cleanly: Stage 1 froze contracts sequentially; Stage 2 forked into parallel workers consuming frozen surfaces; Stage 3 ran observability/integration in the converging session. P1 has the same structural shape — a small set of cross-worker contracts (status state machine signatures + admin API endpoint shapes + PII-scrubber tracing-layer integration), a substantial parallel branch (frontend React vs backend Rust), and a small convergence task.

| Stage | Scope | Topology | Why |
|---|---|---|---|
| **Stage 1** — Foundation contracts + PII oracle (Task Zero) | Port `pii-scrub-audit` Verification Oracle (FR-FBR-10 drift-detection) + port PII scrubber impl + author status workflow state machine contract (C6) + admin API endpoint contracts (C7/C8) + email template tenant-brand parameter contract (C10) + migration `00003_feedback_status_history.sql` | **SEQUENTIAL** (1 agent) | The status-workflow contract is what Stage 2 Workers A and B both consume (Worker A produces transitions; Worker B renders status labels and dispatches transition requests). The PII scrubber is small, self-contained, and Task Zero per arc plan + handoff constraints — folding it into Stage 1 means Stage 2 workers' new code emits through a scrubbed tracing layer for free. Freezing contracts sequentially in one head — with the oracle policing log drift from commit 1 — eliminates Stage 2 contract churn. |
| **Stage 2** — Parallel surfaces | Worker A: status workflow state machine + transitions + audit history + status emails (FR-FBR-08 + FR-FBR-09); Worker B: admin UI list + drawer + reply composer (FR-FBR-07) | **PODS, 2 workers** | Backend Rust and frontend React are distinct skill clusters with clean producer/consumer interface contracts. Worker A owns the entire backend triage surface (state machine + emails cohere — the state-transition function IS the email-send trigger point); Worker B owns the entire admin UI surface (consumes Worker A's frozen HTTP contracts as a library surface, like Stage 2 Worker B consumed Contract C1 in P0). |
| **Stage 3** — E2E witness + ULADP cleanup | E2E script (`scripts/e2e-p1-curl.sh` extension): signup → submit → admin login → list → drawer → transition → status email observed in Mailpit → reply → public-reply email observed; carry-forward critic items C-002 (axum Router-level submission handler integration tests) + missing module READMEs (feedbackr-anon, feedbackr-jwt, feedbackr-api/auth, feedbackr-api/email, feedbackr-api/handlers); `/0-uldf-finalize` Phase 4 ULADP pass | **SEQUENTIAL** (1 agent, in the converging session) | Small port-pattern work consuming both Stage 2 outputs; runs once Stage 2 converges so the witness exercises real wired-together paths. The ULADP cleanup is opportunistic — those crates are now touched by P1, so README authoring is in-scope per the handoff's "alongside any Phase 4 ULADP work in P1 that touches those modules anyway." |

PARALLEL across Stage 1 + Stage 2 would race contract definition against contract consumers — Worker B's React would have to mock the admin API shape that Worker A hasn't pinned yet, then re-thread when Worker A makes adjustments. The cost is asymmetric: each Worker-B revision in React (component prop changes, fetch-call shapes, TypeScript type drift) is far costlier than the matching backend signature changes.

### Collaboration Value Assessment (P1 scope)

| Factor | Score (1-5) | Notes |
|---|---|---|
| **Specialization** | **5** | Backend Rust (axum + sqlx + lettre + tracing) vs frontend React+Vite+TypeScript. Maximum skill-cluster divergence — peak specialization value. |
| **Quality** | 4 | Status workflow audit-trail correctness, email template parameterization tenant-bleed risk, PII drift, admin-UI ARIA/keyboard a11y baseline — each benefits from focused review. |
| **Discovery** | 3 | GitCellar reference impls exist for all four FRs (db.rs, email_templates.rs, scrubber.rs, admin-ui/), but admin UI is a fresh React port at a different repo with different design discipline (port logic, refresh styling); novelty is moderate. |
| **Speed** | 4 | Backend + frontend can fan out cleanly once Stage 1 contracts are frozen. ~1.5 calendar weeks of parallel work compresses to ~1 calendar week. |
| **Boundary Clarity** | **5** | Stage 1 freezes the contracts; Workers in Stage 2 consume them as a frozen library surface. Boundary is exactly the HTTP contract between admin UI and backend admin endpoints. |
| **Coupling** | 4 | Stage 2 workers share only the HTTP contracts (C7/C8) and the admin session cookie shape (carry-state from P0 — already exists). No in-flight inter-worker dependencies. |

**Value**: 16/20. **Friction (higher=less friction)**: 9/10. **Net**: 16 − (10 − 9)/2 = **15.5** → PARALLEL strongly recommended within Stage 2.

### Why 2 workers in Stage 2, not 3 (per handoff confirmation)

The arc plan's intra-phase note suggested 3-4 branches (status workflow / admin UI / emails / PII scrub separately). Refining at P1 plan time per the handoff hint "Worker boundary likely splits backend status-workflow + emails from frontend admin UI — confirm at plan time":

**PII scrubber + oracle fold into Stage 1** (rather than parallel Worker D):
- The scrubber is small (~150-200 LOC port from `gitcellar-service/src/feedback_logs/scrubber.rs`), oracle-paired, and consumed via a tracing-subscriber layer installed at binary init. Folding it into Stage 1 means Worker A's new status-transition logs and Worker B's frontend (irrelevant — backend logs only) all emit through the scrubbed formatter automatically.
- Task Zero ordering is preserved: the oracle is built first; the scrubber implementation lands behind it; Stage 2 workers inherit scrubbed logging gratis.
- A separate Worker D for PII would force a synthetic seam at the tracing-subscriber composition point and risk Worker D's tracing layer landing AFTER Worker A's handlers — leaving a window where new code emits unscrubbed logs. Cohesion at Stage 1 closes that window.

**Status workflow + emails fold into one backend worker** (Worker A):
- The status-transition function IS the email-send trigger point. `transition_status(scope, feedback_id, to_status, reason_note?, duplicate_of?)` synchronously produces both the audit row AND the queued email-send call. Splitting these across two workers forces a synthetic API between them (e.g. transition emits an event; email worker consumes); P0's experience (one worker owning FR-FBR-03 + 05 + 06 because they share a hot code path) showed this consolidation works.
- Calls per FR are correlated: every status transition produces at most one email; every email originates from a transition. One owner.

**Admin UI is the orthogonal surface** (Worker B):
- Different language (TypeScript vs Rust), different runtime (browser vs server), different test framework (Vitest + Playwright vs `cargo test`), different repo location (`admin-ui/` subdirectory). Maximum specialization payoff, clean library-surface consumption of Worker A's HTTP contracts.

This also lowers per-worker context pressure: each worker holds their own surface + frozen contracts + their language's reference port, not the other worker's domain.

---

## Context Budget Assessment

### Stage 1 (single agent, oracle + scrubber + contract-defining)

| Item | Tokens (estimate) |
|---|---|
| Spec sections (FR-FBR-07..10) + DEC-FBR-04 + DEC-PODS-001/002 + DISCOVERIES D-FBR-05..09 | ~8k |
| Arc plan §P1 + this plan's Stage 1 brief + P0 plan §C1/C2 (carry-state reads) | ~6k |
| GitCellar reference reads: `scrubber.rs` (canonical 20-pattern source) + `feedback/db.rs` (status state machine) + `email_templates.rs` + GitCellar's own pii-scrub-audit oracle | ~20-30k |
| Implementation + tests + oracle build + contract authoring + migration | ~40-55k |
| Reasoning reserve | ~25-30k |
| **Total** | **~100-130k → comfortably within 1M context (~10-13% utilization)** |

Pass.

### Stage 2 — Worker A (backend: status workflow + emails)

| Item | Tokens |
|---|---|
| Spec sections (FR-FBR-08 + FR-FBR-09) | ~3k |
| Stage 1 carry-state (frozen contracts C6/C7/C10 + migration 00003 in tree) | ~10k |
| P0 carry-state (admin session shape from CLAUDE-A's work; mailer module shape; repository surface for `feedback_status_history` writes) | ~10k |
| GitCellar reference reads (`feedback/db.rs` for state machine, `email_templates.rs` for templates, `feedback/email.rs` for send path) | ~15-20k |
| Implementation + tests (state machine + audit + admin endpoints + email templates + Mailpit integration tests) | ~50-65k |
| Reasoning reserve | ~25k |
| **Total** | **~115-135k → ~12-14% utilization. Pass.** |

### Stage 2 — Worker B (frontend: admin UI)

| Item | Tokens |
|---|---|
| Spec section (FR-FBR-07) | ~2k |
| Stage 1 carry-state (frozen contracts C7/C8 + admin session cookie shape) | ~8k |
| GitCellar admin-ui port reference (component files + styling + state mgmt patterns) | ~25-35k |
| React + Vite + TypeScript scaffolding + 4 component sets (List, Drawer, ReplyComposer, StatusControls) + Vitest + Playwright a11y harness baseline | ~60-85k |
| Reasoning reserve | ~30k |
| **Total** | **~125-160k → ~13-16% utilization. Pass.** |

Frontend has the heaviest budget due to React file proliferation (many small files = many Read tool calls = many output tokens), but still well under capacity.

### Stage 3 (single agent, e2e + ULADP cleanup)

E2E script extension + 5 missing module READMEs + carry-forward critic C-002 (Router-level integration tests) — surface is moderate. Estimate ~70-100k total. Pass.

**Per-agent budgets all pass with comfortable margin.** No decomposition needed beyond what Stage 2's two-worker split already provides.

---

## Oracle Pre-Build Plan

| Oracle | Question | Consumer(s) | Timing | Status |
|---|---|---|---|---|
| `pii-scrub-audit` | "Does every emitted log line (from `tracing` subscribers) pass through the canonical 20-pattern PII scrubber? Has the pattern set drifted from the canonical source?" | Stage 1 agent (builds + consumes); Stage 2 Worker A (consumes — new status-transition logs emit through scrubbed layer); CI gate from commit 1 | **Task Zero of Stage 1 — built before any new log emission lands** | not yet built (port from GitCellar's existing oracle if present, else author per arc plan) |

**Rationale**: FR-FBR-10 specifies "canonical 20-pattern set" — the entire defense rests on the assumption that the pattern set in code matches the canonical source. Without an oracle, drift is silent (a developer adds a new emit-path that doesn't go through the scrubber, OR a copy-paste accidentally drops a pattern). One oracle, invoked on every commit during P1+ by every backend worker, defends FR-FBR-10's promise to the privacy persona (DEC-FBR-01 Persona D). Three-leg defense per D-FBR-02:
- **Leg 1 — type system**: scrubber function is the SOLE public format-event entry point on the tracing layer; alternatives are `pub(crate)` or absent.
- **Leg 2 — oracle**: `pii-scrub-audit` greps for tracing subscriber `Layer` registrations outside the scrubber module + verifies the pattern-set hash matches the canonical reference.
- **Leg 3 — lint baseline**: clippy + a workspace `deny.toml` rule rejecting direct `tracing_subscriber::fmt()` builder calls outside the binary's main entrypoint.

**Implementation sketch** (Stage 1 agent authors, mirroring the P0 multi-tenant-isolation-check pattern):
- `kind: "verification"`
- `freshness: trigger-invalidate` on changes to `crates/feedbackr-tracing/` (new crate Stage 1 creates) or `migrations/`
- Two probes:
  1. **AST/grep probe over all crates**: any direct `tracing_subscriber::fmt()`, `tracing_subscriber::registry()`, or custom `Layer` impl OUTSIDE `crates/feedbackr-tracing/` fails the oracle.
  2. **Pattern-set hash probe**: SHA-256 of the canonical 20-pattern list (named regex array in `feedbackr-tracing::scrubber::CANONICAL_PATTERNS`) must match a hash recorded in `.claude/oracles/pii-scrub-audit/expected_hash.txt`. Changes require an explicit hash update commit, surfacing pattern-set evolution as a reviewable change.
- Canonical implementation: **Python 3.8+** per DEC-FBR-IMPL-03 (signature first-arg + pattern-set parsing both benefit from real string handling; bash shim forwards).
- Output: machine-parseable PASS / FAIL with file:line offenders.

**Deferrals** (evaluated, not scheduled for P1):
- `widget-bundle-size` — deferred to P2 per arc plan.
- `tier-enforcement-status` — deferred to P3.
- `admin-ui-a11y-baseline` (potential candidate) — **deferred**. Playwright + axe-core a11y harness is mandated for P2 widget per the arc plan's Testability Gate; replicating it for the admin UI in P1 has lower payoff (admin UI users are paying customers' employees, not end-users; A+D persona discipline). Re-evaluate at `/0-uldf-finalize` Phase 11 if Stage 2 Worker B's admin UI work surfaces a11y regressions.
- `email-template-snapshot-drift` (potential candidate) — **rejected as oracle**. Snapshot tests via Vitest/insta cover this at lower cost; oracle scope (drift across SDK ports) doesn't apply since P1 has only one Rust-side email template renderer.

---

## Component Decomposition (stage → worker → sub-task)

```
Stage 1 — Foundation Contracts + PII Oracle (SEQUENTIAL, 1 agent)
└── Task Zero: build pii-scrub-audit Verification Oracle skeleton + freshness contract
└── Sub-task 1: PII scrubber impl (FR-FBR-10)
    ├── New crate: crates/feedbackr-tracing/
    │   ├── src/lib.rs — re-exports
    │   ├── src/scrubber.rs — CANONICAL_PATTERNS [r"...", ...] (20 patterns ported byte-for-byte from gitcellar-service/src/feedback_logs/scrubber.rs)
    │   ├── src/layer.rs — tracing_subscriber::Layer impl applying scrubber to every emitted event's fields
    │   └── tests/ — pattern-by-pattern unit tests + 4-5 integration tests through real tracing emission
    ├── Wire into bin/feedbackr-api/src/main.rs replacing existing tracing setup from P0 Stage 3
    └── Oracle CI integration: workflow runs oracle on every commit; refuses build on red
└── Sub-task 2: Schema migration (FR-FBR-08 audit history)
    └── migrations/00003_feedback_status_history.sql
        ├── Table: feedback_status_history (id PK, feedback_id FK, from_status, to_status, reason_note nullable, duplicate_of_feedback_id nullable, transitioned_by tenant_user_id, transitioned_at)
        └── Index on (feedback_id, transitioned_at DESC) for drawer audit-row queries
└── Sub-task 3: Frozen contract authoring (C6, C7, C8, C9, C10 below) — written to docs/planning/handoffs/p1-stage1-to-stage2.md
└── Sub-task 4: Repository surface extensions (Contract C6 backing methods)
    ├── crates/feedbackr-repository/src/feedback.rs — add: list_for_admin(scope, status_filter?, limit, offset), get_with_history(scope, feedback_id) -> (Feedback, Vec<StatusHistoryRow>)
    └── crates/feedbackr-repository/src/feedback_status_history.rs — new module: FeedbackStatusHistoryRepo trait + SqlxFeedbackStatusHistoryRepo impl
        ├── append(scope, feedback_id, from, to, reason?, duplicate_of?, transitioned_by) -> Result<()>
        ├── list_for_feedback(scope, feedback_id) -> Result<Vec<StatusHistoryRow>>
        └── multi-tenant-isolation-check oracle stays GREEN (all methods take &ProjectScope first)

Stage 2 — Parallel Surfaces (PODS, 2 workers, fan-out after Stage 1 contract freeze)
├── Worker A — Backend: Status Workflow + Status Emails (FR-FBR-08 + FR-FBR-09)
│   ├── Status workflow state machine
│   │   ├── feedbackr_core::FeedbackStatus enum: Submitted | Triaged | InProgress | Shipped | WontFix | Duplicate (already in P0 schema; reuse)
│   │   ├── crates/feedbackr-api/src/handlers/admin_feedback.rs
│   │   │   ├── POST /api/v1/admin/feedback/{feedback_id}/transition — body { to_status, reason_note?, duplicate_of? }
│   │   │   ├── POST /api/v1/admin/feedback/{feedback_id}/reply — body { body, visibility: "public" | "internal" }
│   │   │   ├── GET /api/v1/admin/feedback — list with optional ?status=...&limit=...&offset=...
│   │   │   └── GET /api/v1/admin/feedback/{feedback_id} — single feedback with full status history + replies
│   │   ├── State-machine validation (only legal transitions accepted; illegal transitions return 409 with TransitionError)
│   │   └── Audit row append within same DB transaction as the status column update (atomic)
│   ├── Reply model
│   │   ├── migrations/00004_feedback_replies.sql — feedback_replies table (id PK, feedback_id FK, body, visibility public|internal, author_tenant_user_id, created_at)
│   │   └── FeedbackReplyRepo trait + SqlxFeedbackReplyRepo impl in feedbackr-repository
│   ├── Status emails (FR-FBR-09)
│   │   ├── crates/feedbackr-api/src/email/templates.rs — port from gitcellar-cloud/src/feedback/email_templates.rs
│   │   │   ├── ConfirmationEmail (sent on feedback submission, deferred from P0 — wire up now)
│   │   │   ├── StatusChangeEmail (sent on every status transition with visible-to-submitter target status)
│   │   │   └── PublicReplyEmail (sent on each `visibility: "public"` reply)
│   │   ├── Subject line: `[{tenant.email_subject_prefix} #FB-{display_id}] {short_subject}`
│   │   ├── Footer parameterization: tenant.brand_name, tenant.support_email, tenant.unsubscribe_url
│   │   ├── Sender display name: `{tenant.brand_name} via Feedbackr`
│   │   └── Snapshot tests via `insta` crate for each template (3 templates × locale-en × 2 tenant fixtures = 6 snapshots minimum)
│   └── Trigger discipline: each transition path / reply path queues the email via a single send_email(&ProjectScope, EmailKind, EmailContext) -> Result<()> chokepoint
│       └── Uses lettre's SMTP transport (already wired by CLAUDE-A in P0); Mailpit in dev (port 1025), env-var-driven SMTP in prod
├── Worker B — Frontend: Admin UI (FR-FBR-07)
│   ├── New directory: admin-ui/ (peer to crates/, scripts/, migrations/)
│   │   ├── package.json — react@18, vite@5, typescript@5, @tanstack/react-query, axios, vitest, @playwright/test, @axe-core/playwright
│   │   ├── vite.config.ts — strictPort: true, port: 14204, server.proxy '/api' → 'http://localhost:14304' (P0 backend port)
│   │   ├── tsconfig.json
│   │   └── index.html + src/main.tsx + src/App.tsx
│   ├── Component tree (port logic + structure from gitcellar-cloud/admin-ui/, refresh styling)
│   │   ├── pages/Login.tsx — admin session login (reuses P0 admin-session-cookie shape from CLAUDE-A's work)
│   │   ├── pages/FeedbackList.tsx — filtered list view (status filter pills + pagination)
│   │   ├── pages/FeedbackDrawer.tsx — single-feedback detail view
│   │   │   ├── Body display (PII-respecting — render as plain-text, not HTML)
│   │   │   ├── Status history list with timestamps + transitioner identities
│   │   │   ├── Reply list (public + internal tabs)
│   │   │   ├── ReplyComposer (textarea + visibility radio + send button)
│   │   │   └── StatusControls (state-machine-aware: only renders legal next-state buttons)
│   │   └── shared/ApiClient.ts — typed wrappers around C7/C8 endpoints (TypeScript types match Worker A's response shapes)
│   ├── Auth flow
│   │   ├── Login posts to /api/v1/auth/login (P0 endpoint from CLAUDE-A); receives Set-Cookie admin session
│   │   ├── Subsequent admin endpoint calls send the cookie automatically (same-site, secure-in-prod)
│   │   └── 401 responses redirect to /login
│   ├── Vitest unit tests for each component (state-machine button rendering, reply composer validation, list filtering)
│   └── Playwright + @axe-core/playwright a11y smoke (1 happy-path scenario: login → list → drawer → reply → transition; assert zero axe violations on each page)

Stage 3 — E2E Witness + ULADP Cleanup (SEQUENTIAL, 1 agent, in converging session)
└── Sub-task 1: E2E P1 witness script
    └── scripts/e2e-p1-curl.sh extending scripts/e2e-p0-curl.sh
        ├── 8 steps: signup → verify → project → key-register → JWT submit (P0 carry-forward) → admin login → list contains FB-id → transition to triaged → poll Mailpit for status-change email → reply public → poll Mailpit for public-reply email
        └── ALL PASS required; refuses to exit 0 on any subset
└── Sub-task 2: Carry-forward critic C-002
    └── crates/feedbackr-api/tests/router_submission_integration.rs (or similar location) — axum-Router-level integration tests for the P0 submission handler
        ├── Adjacent to admin-UI Router work (admin endpoints follow the same Router composition pattern; co-locating tests is cheap)
        └── ~3-5 tests covering JWT submit happy path, anon submit happy path, 401 path, 429 path, 400 body validation path
└── Sub-task 3: Carry-forward ULADP module READMEs
    └── Author README.md for each of: crates/feedbackr-anon/, crates/feedbackr-jwt/, crates/feedbackr-api/src/auth/, crates/feedbackr-api/src/email/, crates/feedbackr-api/src/handlers/
        ├── Per-module Synopsis + File Index + Public API + Constraints + Decision Log per ULADP
        └── /0-uldf-uladp-compliance should report 100% synopsis coverage on the crates touched by P1 after this sub-task
```

---

## Interface Contracts (MUST be authored in detail in Stage 1; carried as frozen state to Stage 2)

### Contract C6 — Status Workflow State Machine

**Owner**: Stage 1 agent. **Consumers**: Worker A (implements transitions); Worker B (renders state-machine-aware controls; never emits illegal transition UI).

```rust
// crates/feedbackr-core/src/status.rs — extends P0's FeedbackStatus enum

pub enum FeedbackStatus {
    Submitted,
    Triaged,
    InProgress,
    Shipped,
    WontFix,
    Duplicate,
}

/// Legal transitions. Returned ordering is stable for UI rendering.
/// Source: gitcellar-cloud/src/feedback/db.rs (state-machine ported).
pub fn legal_transitions_from(s: FeedbackStatus) -> &'static [FeedbackStatus] {
    match s {
        Submitted   => &[Triaged, WontFix, Duplicate],
        Triaged     => &[InProgress, WontFix, Duplicate, Submitted /* admin revert */],
        InProgress  => &[Shipped, WontFix, Duplicate, Triaged /* admin revert */],
        Shipped     => &[],  // terminal
        WontFix     => &[Submitted],  // re-open path
        Duplicate   => &[Submitted],  // un-merge path
    }
}

pub enum TransitionError {
    IllegalTransition { from: FeedbackStatus, to: FeedbackStatus },
    DuplicateRequiresTarget,  // to == Duplicate but no duplicate_of provided
    DuplicateTargetMissing,   // duplicate_of_feedback_id doesn't exist in scope
    DuplicateSelfReference,   // duplicate_of == feedback_id
}
```

**Worker A's transition function signature**:

```rust
// crates/feedbackr-api/src/handlers/admin_feedback.rs

pub async fn transition_status(
    scope: &ProjectScope,
    feedback_id: FeedbackId,
    to: FeedbackStatus,
    reason_note: Option<&str>,
    duplicate_of: Option<FeedbackId>,
    transitioned_by: TenantUserId,
) -> Result<TransitionOutcome, TransitionError>;
```

**Hard invariants (Worker A unit-tests with named tests)**:
1. Illegal transitions fail with `TransitionError::IllegalTransition` BEFORE any DB write.
2. `to == Duplicate` requires `duplicate_of: Some(_)` or fails with `DuplicateRequiresTarget`.
3. `duplicate_of` must reference a feedback row in the SAME `ProjectScope` (cross-tenant duplicate target rejection).
4. Audit row is appended in the SAME DB transaction as the status column update (atomicity — no half-applied transitions).
5. `from == to` (no-op transition) returns `Err(IllegalTransition)` rather than silently succeeding.

### Contract C7 — Admin Status Transition + Reply HTTP API

**Owner**: Stage 1 agent (shape) + Worker A (implementation). **Consumers**: Worker B (admin UI fetches).

```http
POST /api/v1/admin/feedback/{feedback_id}/transition
Cookie: feedbackr_admin_session=<value>  (P0 carry-state)
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

401 if admin session invalid; 404 if feedback_id not in scope.

POST /api/v1/admin/feedback/{feedback_id}/reply
Cookie: feedbackr_admin_session=<value>
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
  "email_queued": true  // false if visibility == "internal" or submitter has no email
}
```

### Contract C8 — Admin Feedback List + Get HTTP API

**Owner**: Stage 1 agent (shape) + Worker A (implementation). **Consumers**: Worker B.

```http
GET /api/v1/admin/feedback?status=triaged&limit=20&offset=0
Cookie: feedbackr_admin_session=<value>

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

GET /api/v1/admin/feedback/{feedback_id}
Cookie: feedbackr_admin_session=<value>

200 OK:
{
  "feedback_id": "FB-XXXXXX",
  "kind": "bug",
  "status": "triaged",
  "body": "full body string, PII-respecting; never log-scrubbed for admin view",
  "submitted_at": "...",
  "submitter": { "kind": "authenticated" | "anonymous",
                 "sub": "...?", "email": "...?", "name": "...?" },
  "external_metadata": {...}?,
  "status_history": [
    { "from_status": "submitted", "to_status": "triaged",
      "reason_note": "...?", "duplicate_of_feedback_id": "...?",
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

**Invariant**: admin feedback responses include the full body unredacted. The PII scrubber (Contract C9) applies to LOGS, not to admin-UI response bodies — admins must be able to read what users submitted (this is the entire product). Worker B's frontend renders the body as plain-text (no HTML interpretation, no link auto-conversion) to defend against stored-XSS from submitter content.

### Contract C9 — PII Scrubber Layer

**Owner**: Stage 1 agent. **Consumers**: every log emission across all crates (transparently via tracing-subscriber layer).

```rust
// crates/feedbackr-tracing/src/lib.rs

/// Install in main.rs before any tracing emission:
/// ```ignore
/// feedbackr_tracing::install_global_subscriber(
///     LogLevel::Info,
///     LogFormat::Json,
/// )?;
/// ```
pub fn install_global_subscriber(level: LogLevel, format: LogFormat) -> Result<(), TracingError>;

// crates/feedbackr-tracing/src/scrubber.rs

/// 20-pattern canonical set, byte-for-byte port from
/// gitcellar-service/src/feedback_logs/scrubber.rs.
/// SHA-256 of this slice is recorded in
/// .claude/oracles/pii-scrub-audit/expected_hash.txt
pub(crate) static CANONICAL_PATTERNS: &[(&str, &str)] = &[
    // (pattern_name, regex)
    ("email", r"..."),
    ("phone_us", r"..."),
    ("ssn", r"..."),
    // ... 17 more
];

/// Apply all 20 patterns to a string, replacing matches with "[REDACTED:{pattern_name}]".
pub fn scrub(input: &str) -> String;
```

**Hard invariants (Stage 1 agent unit-tests)**:
1. SHA-256 of `CANONICAL_PATTERNS` matches expected_hash.txt exactly.
2. Each pattern has at least one positive-match test fixture and one near-miss-no-match test fixture.
3. Direct `tracing_subscriber::fmt()` builder calls outside `crates/feedbackr-tracing/` fail oracle Probe A.
4. The scrubber is idempotent: `scrub(scrub(x)) == scrub(x)`.

### Contract C10 — Email Template Tenant-Brand Parameters

**Owner**: Stage 1 agent (parameter shape) + Worker A (template renderers). **Consumers**: Worker A's renderers; carry-state for P3 tier-display footer; P4 self-host docs (parameter list documentation).

```rust
// crates/feedbackr-api/src/email/context.rs

pub struct EmailTenantBrand {
    pub brand_name: String,         // tenants.brand_name, defaults to tenant email_local_part
    pub email_subject_prefix: String,  // tenants.email_subject_prefix, defaults to brand_name
    pub support_email: String,      // tenants.support_email, defaults to tenants.email
    pub unsubscribe_url: Option<String>,  // tenants.unsubscribe_url; None → no unsubscribe footer line
    pub footer_signature: String,   // tenants.footer_signature, defaults to "— The {brand_name} team"
    pub sender_display_name: String, // computed: "{brand_name} via Feedbackr"
}

impl EmailTenantBrand {
    pub fn from_scope(scope: &TenantScope, repo: &dyn TenantRepo) -> Result<Self>;
}
```

**Subject format**: `[{email_subject_prefix} #FB-{display_id}] {short_subject}` — display_id is the FB-NNNNNN form per FR-FBR-09.

**Footer template** (plain-text, per FR-FBR-09 "Status emails (plain-text)"):
```
{footer_signature}
---
You are receiving this because you submitted feedback to {brand_name}.
Reply to this email or contact {support_email}.
{unsubscribe_url_line_if_some}
```

Schema migration (Stage 1 owns): add `brand_name`, `email_subject_prefix`, `support_email`, `unsubscribe_url`, `footer_signature` columns to `tenants` table via `migrations/00005_tenant_email_brand.sql` (defaults non-null where sensible; unsubscribe_url nullable). Repository surface extension on `TenantRepo`: `update_brand(scope, EmailTenantBrand) -> Result<()>` + serialization to/from `EmailTenantBrand` value in the existing `find_by_email` + `scope_for` returns (consider whether to fold brand into Tenant struct or fetch on demand — Stage 1 agent decides at impl-time; both pass the oracle).

### Contract C11 — Admin Session Cookie (carry-state from P0)

**Owner**: P0 CLAUDE-A (already shipped). **Consumers**: Worker A (every admin endpoint), Worker B (every admin UI request).

Documented as carry-state, not re-authored:
- Cookie name: `feedbackr_admin_session` (verify in P0 code if exact name differs)
- Shape: HMAC-signed value `{tenant_id, tenant_user_id, issued_at, expires_at}`, base64url-encoded
- Verification: argon2-equivalent middleware extracts tenant_id, mints `TenantScope` via `TenantRepo::scope_for` (pre-auth allow-listed per DEC-FBR-IMPL-02)
- Worker A: every admin endpoint sits behind `axum::middleware::from_fn(require_admin_session)` extractor
- Worker B: every request includes the cookie via `axios.defaults.withCredentials = true`; same-site; secure-in-prod

**Stage 1 agent action**: read the existing P0 admin-session module (`crates/feedbackr-api/src/auth/` per P0 critic note), document the cookie shape verbatim in Stage 1's handoff doc, do NOT re-implement.

---

## Testability Gate Findings

Per `claude-template/segments/-ldis/plan-phase4-testability-gate.md`. Five questions scored per P1 FR.

### Flagged

#### FR-FBR-10 — PII scrubber with canonical 20-pattern regex set

| Q | Score | Reasoning |
|---|---|---|
| Q1 | Iteration cost | 2 (pattern-by-pattern unit tests are cheap; tracing-subscriber integration tests cost slightly more) |
| Q2 | Fidelity risk | **4** (pattern drift is silent — a developer adds a new emit path that bypasses the scrubber, or copy-paste drops one of the 20 patterns, and there's no symptom until a customer notices a log; the privacy persona's whole pitch rests on this not happening) |
| Q3 | Critical path | 3 (P1 exit gate depends on this for the privacy positioning; downstream P3 tier footer and P4 self-host docs both reference the canonical pattern set) |
| Q4 | Scaffolding leverage | **yes — `pii-scrub-audit` Verification Oracle halves iteration cost AND closes the fidelity gap** |
| Q5 | Drift detection | pattern-set hash + AST probe over all crates; oracle re-runs on any change to `feedbackr-tracing/` |

**Composite 11 (borderline); flagged because Q2=4 with scaffolding planned (Q5 mandatory) and Q3 ≥ 3.**

**Recommendation**: build the `pii-scrub-audit` oracle as Task Zero of Stage 1 BEFORE the scrubber implementation lands. Three-leg defense per D-FBR-02 (type system + AST oracle + lint baseline) — the same shape that made `multi-tenant-isolation-check` reliable. Pattern-by-pattern unit tests live in `crates/feedbackr-tracing/tests/`. Stage 2 Worker A's new log emissions automatically inherit the scrubbed layer (no per-worker integration cost).

### Items NOT flagged (composite <10 with no Q2=5 spike)

- **FR-FBR-07** (admin UI): composite 8. Q1=3 (React iteration overhead + Vitest cycle; Playwright cycle slightly higher), Q2=2 (visual + axe assertions catch most regressions; admin UI is internal — A+D persona discipline rules out aggressive a11y oracle), Q3=3 (P1 exit gate depends but downstream phases don't), Q4=yes (Vitest + Playwright + axe-core baseline), Q5=axe-violation count on every PR.
- **FR-FBR-08** (status workflow): composite 9. Q1=2 (DB-only unit tests via sqlx::test; deterministic state machine), Q2=3 (audit-row atomicity is the main risk; the same-transaction discipline in Contract C6 mitigates), Q3=4 (P1 exit gate depends + Worker B blocks on this contract), Q4=yes (named state-machine tests per transition; Contract C6's hard invariants 1-5 become 5+ named tests), Q5=clippy + cargo-deny.
- **FR-FBR-09** (status emails): composite 7. Q1=2 (Mailpit is cheap; snapshot tests via `insta` crate are near-zero cost), Q2=2 (template snapshot tests + tenant-brand parameterization tests; subject-line format is byte-exact testable), Q3=3, Q4=`insta`-snapshots are scaffolding, Q5=snapshot review on every change.

**FR-FBR-08 audit-row atomicity** deserves a note despite not being flagged: this is the highest-stakes invariant in Worker A's scope. The transition path MUST write the status column + the audit row in a single `BEGIN ... COMMIT` block. Document this in Worker A's brief as untouchable. Test name: `transition_writes_audit_atomically_or_fails`. The test should attempt to mock a failure between the two writes (sqlx test transaction rollback) and assert that the status column is unchanged.

---

## Ripple Analysis

P1 is additive to P0's frozen surfaces — no API contract breaking changes; no requirement renames.

### Modified Interfaces

| Interface | Change | Consumers updated | Migration cost |
|---|---|---|---|
| `feedbackr_repository::FeedbackRepo` | **EXTEND** — add `list_for_admin(scope, status_filter?, limit, offset)` and `get_with_history(scope, feedback_id)`. Existing methods unchanged. | Worker A's admin handlers; no P0 consumer breaks. | None — additive. |
| `feedbackr_repository::TenantRepo` | **EXTEND** — add `update_brand(scope, EmailTenantBrand)`; brand fields may be fetched via `find_by_email` widening or via a new `get_brand(scope)` method. Stage 1 agent picks. | Stage 1 (Contract C10 wiring); Worker A's email renderers. | None — additive. |
| `crates/feedbackr-api/src/main.rs` tracing setup | **REPLACE** — current P0 Stage 3 inline `tracing_subscriber::fmt()` builder is replaced by `feedbackr_tracing::install_global_subscriber(...)`. | One file change; verified by existing P0 e2e witness still passing. | Low — single binary file. |
| `feedbackr_core::FeedbackStatus` | **NO CHANGE** — enum variants are already in P0 schema; only the state-machine transition rules are NEW (in `feedbackr_core::status`). | Worker A consumes via `legal_transitions_from`. | None. |

### New Surfaces

- New crate: `crates/feedbackr-tracing/` (Stage 1)
- New DB tables: `feedback_status_history` (migration 00003, Stage 1), `feedback_replies` (migration 00004, Worker A) — both new, no foreign-key cascades into existing P0 tables beyond the existing `feedback.id` FK.
- New tenant columns: `brand_name`, `email_subject_prefix`, `support_email`, `unsubscribe_url`, `footer_signature` via migration 00005 (Stage 1). Backfill with sensible defaults from existing `tenants.email` row.
- New admin endpoints: 4 routes under `/api/v1/admin/feedback/...` (Worker A)
- New directory: `admin-ui/` (Worker B) — fully isolated; no impact on existing crates.

### Forward-looking ripples (consumers that don't yet exist — Stage 1/Stage 2 contracts must accommodate)

| Future consumer | What it will need from P1 | Captured in |
|---|---|---|
| P2 widget (FR-FBR-04) | Status labels rendered in widget for end-user status-display ("Your feedback is in progress") | C6 enum variants are stable; widget reads via a public status-name endpoint Worker A could expose (`GET /api/v1/projects/{id}/feedback/{fb_id}/status` — public, JWT-or-anon authorized matching the submission auth). **Decision deferred**: Worker A may add this endpoint in Stage 2 if cheap; otherwise P2's plan adds it. |
| P2 promote-to-roadmap (FR-FBR-12) | Status transition Submitted → Duplicate with `duplicate_of` pointing to a NEW roadmap-item row (cross-table). Q24 privacy invariant: byte-for-byte body forwarding with NO submitter attribution. | C6 already supports `to == Duplicate` with `duplicate_of`; P2 introduces a parallel `roadmap_promotes` table that bridges feedback → roadmap_item; the Q24 invariant lives in P2's roadmap-promote handler. P1 does not regress Q24 — P1 has no roadmap surface. |
| P3 tier display (FR-FBR-14) | "Powered by Feedbackr" widget footer (P2) + tier-cap counter reads in admin UI dashboard (P3) | C10's EmailTenantBrand + a future EmailTenantTier field can be additive in P3; P1 doesn't anticipate. |
| P4 self-host docs | All EmailTenantBrand parameter names; PII scrubber pattern-set documentation; env-var configuration surface (already 12-factor from P0) | C10 docstrings + `feedbackr-tracing::scrubber::CANONICAL_PATTERNS` doc comments + `docs/operations/EMAIL_TEMPLATES.md` (Stage 3 sub-task) |

**No source-level ripple into GitCellar.** Confirmed: P1 modifies zero lines in `gitcellar-*` working trees. GitCellar reference reads are read-only per DEC-FBR-07.

---

## Deferred Decisions

| Decision | Deferred Until | Default if Unresolved | Why Defer |
|---|---|---|---|
| Public end-user "what's the status of my feedback?" endpoint (FR-FBR-07 widget-side) | P2 widget plan round | Not in P1; admin-only views in P1 | P2 widget plan finalizes the public-facing status-display contract — premature to commit now |
| Email digest cadences (port from GitCellar's `digest_worker`) | Post-v1 | Out (DEC-FBR-08 OUT list) | Net new feature beyond v1 MVP |
| Admin RBAC / multi-admin-per-tenant | Post-v1 | Single-admin-per-tenant (effectively: the signup tenant_user_id) | DEC-FBR-08 rules this out for v1 |
| Internal-reply visibility on the public reply path (a public reply with internal notes) | Worker A plan-out | Two separate replies, never merged in transit | Worker A finalizes; simplest shape |
| Admin UI dark mode / themes | P4 brand pass | Default light theme | DEC-FBR-09 schedules brand work at P4 |
| Admin UI state management library (React Query alone? Add Zustand? Redux?) | Worker B plan-out | React Query + React Context for cross-cutting | Worker B picks; not arc-level concern |
| Outbound email queue persistence (in-memory vs DB-backed) | Worker A plan-out | In-memory `tokio::spawn` send tasks for P1 (acceptable per single-instance dogfood scale per D-FBR-08 graduation criterion); document Redis/DB backend swap as v1.1 non-breaking | Worker A finalizes |
| Reply rich-text vs plain-text | Stage 1 / Worker A | **Plain-text only** (DEC-FBR-09 mandates plain-text emails; reply composer matches for byte-exact email-body rendering) | Decided here |
| `display_id` format `FB-NNNNNN` vs `FB-XXXXXX` (numeric vs alphanumeric) | Worker A | **`FB-NNNNNN` zero-padded sequential per project** (matches GitCellar precedent; numeric is more memorable for support emails) | Decided here |

---

## Risks and Mitigations

| Risk | Likelihood | Severity | Mitigation |
|---|---|---|---|
| PII scrubber pattern drift across SDK ports or in-tree edits | Low (after oracle) | **High** | Three-leg defense: type-system chokepoint (single `install_global_subscriber` entry), `pii-scrub-audit` oracle (AST probe + hash probe), clippy + cargo-deny rules. Wired in Stage 1 commit 1. Pattern set ported byte-for-byte from GitCellar with hash recorded. |
| Audit-row atomicity violated (status flipped but audit row not appended) | Low (with same-transaction discipline) | **High** | Worker A's transition function MUST use a single `BEGIN ... COMMIT` block. Test: `transition_writes_audit_atomically_or_fails` simulates rollback between updates and asserts no status drift. |
| Worker B (frontend) needs Worker A's admin endpoints before Worker A finishes them | Medium | Medium | Worker B's brief includes mock-mode: typed `ApiClient.ts` matches Stage 1's frozen Contract C7/C8 JSON shapes verbatim; Worker B can implement against an in-memory mock for component dev and switch to real fetch once Worker A's first endpoint is live. PODS `channels/messages.md` for coordination on which endpoints are wired vs mocked. |
| Email send failure mode (Mailpit unreachable in dev; SMTP down in prod) | Medium | Low | Worker A's `send_email` chokepoint logs + emits a tracing span on failure but does NOT block the HTTP response (status transition succeeds; email retry deferred to v1.1 outbox pattern). Documented as a known property. Email queue persistence deferred to v1.1 (see Deferred Decisions). |
| State-machine transition rules surprise users (e.g. revert paths from `Triaged → Submitted` confuse new admins) | Low | Low | Worker B's UI renders ONLY `legal_transitions_from(current_status)` — illegal transitions are never visible. Revert paths are explicit buttons labeled "Re-open" or "Revert to Submitted" rather than generic "Change status". |
| Admin session cookie compromised by stored-XSS in feedback body | Low | High | Worker B renders feedback body as plain-text (`textContent` / React's default escaping; never `dangerouslySetInnerHTML`). HttpOnly + Secure + SameSite=Strict on the session cookie (verify CLAUDE-A set this in P0). |
| Frontend dev port 14204 collides with sibling project | None | n/a | Reserved in `~/.claude/MACHINE_CONFIG.md` Dev Port Registry; Vite `strictPort: true` enforced per CLAUDE.md. |
| Stage 2 Worker A and Worker B converge with incompatible TypeScript types vs Rust response shapes | Low (with Contract C7/C8 frozen) | Medium | Stage 1's handoff doc includes both Rust type signatures AND a hand-rolled TypeScript type-mirror file (`admin-ui/src/shared/types.gen.ts.example`) Worker B uses as the authoritative typing. Worker A's response struct is the source of truth; Worker B mirrors. Convergence test: Stage 3 e2e script asserts admin-UI fetch responses parse against the TypeScript types via Vitest. |
| ULADP module READMEs (Stage 3 sub-task) inflate Stage 3 scope beyond reasonable | Medium | Low | If Stage 3 runs long, defer ULADP READMEs to a follow-up `/0-uldf-uladp-compliance` invocation. The carry-forward critic items are "low severity, non-blocking" per handoff — they can move to a P1.1 cleanup pass if necessary. |
| In-memory email send tasks lost on restart (per D-FBR-08 graduation pattern) | Medium | Low | Acceptable per single-instance P1 dogfood scale. Documented as known property. v1.1 graduation: durable outbox table (`outgoing_emails`) + retry worker; non-breaking swap because `send_email` chokepoint API doesn't change. |

---

## PODS Coordination Protocol (Stage 2)

Reapplying lessons from D-FBR-06 (PODS LD-in-monitor coordination latency):

- **Lead Developer mode**: this orchestrating session becomes LD via `/0-uldf-pods-parallelize`; spawns CLAUDE-A (backend) and CLAUDE-B (frontend) via `/0-uldf-pods-spawn-collaborator --all`.
- **Self-mediation authority** (per D-FBR-05): workers MAY self-mediate if a needed contract widening matches a plan-time pre-specified API signature in Stage 1's handoff doc. Procedure:
  1. Worker recognizes a needed widening (e.g. an additional repository method, an additional optional field on a JSON shape).
  2. Worker checks Stage 1's handoff doc for a pre-specified signature.
  3. If signature-match: worker proceeds, tags `channels/decisions.md` with `self_mediated=true; ratification_pending=true; matches_spec_at=<doc_path>`.
  4. If no signature-match: worker writes to `channels/alerts.md` with `**LD-state**: script-monitor / **Self-mediation**: NOT AUTHORIZED — non-pre-specified change. Awaiting LD ratification.`
- **Pre-authorized widenings** (Stage 1 agent authors these into the handoff doc):
  - `FeedbackRepo::list_for_admin` may extend with additional filter parameters (`?author=...`, `?since=...`) if Worker A discovers a UX-driven need — pre-spec'd as "additional optional query params, defaulting to no-filter".
  - `EmailTenantBrand` may extend with additional optional brand fields if Worker A discovers parameterization gaps — pre-spec'd as "additional `Option<String>` columns, schema-backwards-compatible".
  - Admin UI component prop shapes may extend without LD ratification (purely Worker B internal).
- **NOT pre-authorized** (require LD halt):
  - Adding a new entry to `.claude/oracles/multi-tenant-isolation-check/allowlist.toml` (oracle invariant; same discipline as P0).
  - Changing the JSON shape of an existing endpoint in a non-backwards-compatible way.
  - Cross-contract widenings (e.g. C6 state-machine new state variants).

**Worker A and Worker B share files via PODS `touches.json`**:
- Worker A touches: `crates/feedbackr-api/src/handlers/admin_feedback.rs`, `crates/feedbackr-api/src/email/`, `crates/feedbackr-repository/src/feedback.rs` (extension), `migrations/00004_feedback_replies.sql`, new repo modules.
- Worker B touches: `admin-ui/` entirely; no Rust crate touches.
- **Zero file conflicts expected.** Worker B's only Rust touches would be `crates/feedbackr-api/src/main.rs` if a CORS / static-file-serving change is needed for the admin UI; defer this to Stage 3 e2e integration (orchestrator owns).

---

## Execution Commands

The recommended progression:

1. **Now**: this plan is being authored under `autopilot:continuous`. No user review interrupt — `/0-uldf-proceed` will route from here per AUTOCHAIN.
2. **Stage 1 trigger** (single sequential session in this repo):
   - LTADS S001 is `PAUSED` per `ltads/sessions/current-session.md`; `/0-uldf-proceed` Phase 3 will likely choose HANDOFF to a fresh orchestrated session for full-context Stage 1 implementation OR HERE if context budget allows.
   - Stage 1 exit criteria: `pii-scrub-audit` oracle GREEN + Contracts C6/C7/C8/C9/C10/C11 frozen + migrations 00003/00005 + scrubber crate + repository extensions + `multi-tenant-isolation-check` oracle STILL GREEN + Stage 2 carry-state docs at `docs/planning/handoffs/p1-stage1-to-stage2.md`.
3. **Stage 2 trigger** (after Stage 1 commit lands):
   - `/0-uldf-pods-parallelize` (this plan as input) → become Lead Developer of 2 Stage 2 workers.
   - `/0-uldf-pods-spawn-collaborator --all` → spawn Worker A (backend) + Worker B (frontend) in separate Claude CLI sessions.
   - Each worker reads their contract section from this plan plus their FRs from `docs/specs/SPECIFICATION.md` plus Stage 1's handoff doc.
4. **Stage 2 convergence**:
   - `/0-uldf-pods-converge` once both workers report exit-gate-met.
5. **Stage 3 trigger** (single session in converging Lead Developer's tree):
   - E2E witness extension + carry-forward critic C-002 + module READMEs (ULADP) run sequentially in the same session that ran convergence.
   - `/0-uldf-finalize --skip-push` once P1 exit gate (admin list/drawer/transition/reply with all four email types observed in Mailpit + `pii-scrub-audit` GREEN + `multi-tenant-isolation-check` GREEN + e2e script PASS) is reached.
   - **`--skip-push` is mandatory per handoff constraints** — AGPL LICENSE remains stub; no remote pushes until user replaces with full text per DEC-FBR-05 ratification gate.
6. **After P1 close**: fresh `/0-uldf-ldis-plan "Feedbackr P2 — Customer-Facing"` round consuming this plan as the predecessor + the arc plan's §P2 brief.

---

## Notes for Downstream Consumers

- **`/0-uldf-pods-parallelize`**: consume this plan; the two Stage 2 worker briefs are encoded in the Component Decomposition + Interface Contracts sections. Worker A's brief is FR-FBR-08 + FR-FBR-09 + Contracts C6/C7/C8/C10. Worker B's brief is FR-FBR-07 + Contracts C7/C8/C11 (consumer of all).
- **`/0-uldf-ltads-start`**: resume LTADS S001 (currently PAUSED). The arc-plan, P0 plan, and this plan together are the cross-session carry-state through P1. Mid-arc Checkpoint will update post-Stage 1 commit.
- **Stage 1 agent**: your deliverable is **(a)** the `pii-scrub-audit` Verification Oracle GREEN, **(b)** the `feedbackr-tracing` crate with 20 canonical patterns, **(c)** the `feedback_status_history` migration, **(d)** Contracts C6/C7/C8/C10 frozen and written to `docs/planning/handoffs/p1-stage1-to-stage2.md`, **(e)** repository extensions to `FeedbackRepo` + `TenantRepo` (no new pre-auth allowlist entries needed — these are scope-clean additions), and **(f)** carry-state read of P0's admin session module documented for Worker A/B reference. Resist scope creep into Stage 2 (status transitions, email rendering, admin UI). Contract-first.
- **Workers in Stage 2**: read Contracts C6/C7/C8/C10/C11 + the existing P0 carry-state (admin session, mailer, repository surface) as frozen library surfaces. If you discover an inadequacy in a contract mid-Stage-2, **check the self-mediation pre-spec list first** (PODS Coordination Protocol § Pre-authorized widenings); if signature-match → proceed and tag for ratification; if no match → halt via `channels/alerts.md`.
- **P1 exit gate verification**: a single curl + admin-UI-via-Playwright pipeline through signup → submit → admin login → list → transition → email-observed → public-reply → public-reply-email-observed must be runnable from a clean `docker compose up`. This is the durable witness that P1 closed.
- **AGPL LICENSE ratification gate**: not addressed in this plan; user-action. `/0-uldf-finalize` continues to invoke with `--skip-push` until the LICENSE file is replaced with full AGPL-3.0 text and the user authorizes the first public push.
