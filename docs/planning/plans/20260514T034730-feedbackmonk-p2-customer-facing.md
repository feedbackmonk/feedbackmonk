# Execution Plan — feedbackmonk P2 (Customer-Facing)
**Source**: /0-uldf-ldis-plan
**Generated**: 2026-05-14T03:47:30Z
**Task**: feedbackmonk P2 — Customer-Facing
**Strategy**: PARALLEL (3-worker PODS, same-branch, intra-phase staged with Worker B contract freeze gating Worker C)
**Intake Source**: `docs/planning/intakes/20260512T221154-extract-feedback-system-as-standalone-product.md`
**Arc Plan**: `docs/planning/plans/20260513T185711-feedbackmonk-v1-build-arc.md` (P2 section)
**Prior Phase Plan**: `docs/planning/plans/20260513T231115-feedbackmonk-p1-closes-the-loop.md` (CLOSED at `835fbf8` + rename `82a2e59`)
**Spec Source**: `docs/specs/SPECIFICATION.md` (FR-FBR-04 / 11 / 12 / 13) + `docs/specs/DECISIONS.md` (DEC-FBR-02 / 04 / 06 load-bearing)

---

## Phase summary

P2 ships **the customer-facing surface**: the embeddable widget that end-users see, the public roadmap they vote on, and the admin action that promotes a feature-request feedback into a public roadmap item. After P2, feedbackmonk has a complete two-sided product loop (submit → triage → ship-or-promote → publicly track), and the widget creates the distribution channel P3's commercial gate needs.

**Scope**: FR-FBR-04 (widget) + FR-FBR-11 (public roadmap) + FR-FBR-12 (promote-to-roadmap + Q24 invariant) + FR-FBR-13 (voting + aggregator).

**Time budget**: ~3 weeks FTE per arc plan. Widget is the long pole and gets generous UX iteration budget.

---

## Strategy Rationale

### Why PARALLEL (3-worker PODS), not SEQUENTIAL

The P2 FR set splits cleanly into three ownership-clear streams:

| Worker | FR(s) | Surface | Coupling to other workers |
|---|---|---|---|
| **Worker A — Widget** | FR-FBR-04 | greenfield `widget/` dir + `widget-bundle-size` Verification Oracle + a11y harness | consumes existing `POST /api/v1/projects/{id}/feedback` (P0) + adds `GET /api/v1/projects/{id}/widget-config` (Contract C12); independent of B/C |
| **Worker B — Roadmap Backend** | FR-FBR-11 + FR-FBR-13 | migrations 00006/00007, `feedbackmonk-core` roadmap types, `feedbackmonk-repository` roadmap repos, public + admin HTTP API, 60s in-process voting cache (port pattern from `gitcellar-cloud/src/feedback/roadmap_voting.rs`) | produces Contracts C13–C15 that Worker C consumes |
| **Worker C — Promote + UI** | FR-FBR-12 (backend) + roadmap UI (frontend) | `crates/feedbackmonk-api/src/handlers/promote.rs` with byte-for-byte `render_roadmap_body` port + Q24 inline test; admin-ui new pages (admin roadmap list + public roadmap viewer + promote button) | depends on Worker B's contract freeze (mid-phase sync point); independent of Worker A |

This shape mirrors P1 Stage 2's successful pattern (Worker A backend + Worker B frontend, same-branch, disjoint file touches), extended to three workers because P2 adds a third novel surface (the widget) that does not share a directory with anything else in the tree.

PARALLEL is strongly favored by the per-worker Collaboration Value Assessment:

| Factor | Score (1-5) | Notes |
|---|---|---|
| **Specialization** | 5 | Three distinct skill clusters — frontend bundler/a11y (A), Rust HTTP+cache (B), Rust port + React UI (C) |
| **Quality** | 4 | Q24 byte-for-byte invariant + widget a11y + voting double-vote prevention each benefit from focused review |
| **Discovery** | 4 | Widget is the most novel piece in the entire arc (no port reference); roadmap voting cache has subtle freshness/concurrency semantics |
| **Speed** | 4 | Three weeks calendar — running A+B fully parallel (and C partly parallel) cuts wall-clock ~40% |
| **Boundary Clarity** | 4 | File-touch boundaries clean: Worker A → `widget/`, Worker B → `crates/` + `migrations/`, Worker C → `crates/feedbackmonk-api/src/handlers/promote.rs` + `admin-ui/src/pages/roadmap/` |
| **Coupling** | 3 | Worker C depends on Worker B's contract; resolved via mid-phase freeze + channels handshake (same pattern as P1 Stage 1→2) |

**Value total**: 17/20. **Friction total**: 7/10. **Net**: 13.5 → PARALLEL strongly recommended.

### Why same-branch (not worktrees)

P1 Stage 2 ran 3-way PODS same-branch with `antiFitScore=0` from the `project-runtime-state` oracle and zero file conflicts (Worker A in `crates/`, Worker B in `admin-ui/`). P2's worker boundaries are equally disjoint: `widget/` is greenfield, Worker B's backend touches `crates/feedbackmonk-{core,repository,api}/src/roadmap.rs` (new module) + `migrations/00006_*.sql` + `migrations/00007_*.sql`, Worker C's promote handler is a new file. Same-branch is the default unless the user explicitly passes `--worktrees` at `/0-uldf-pods-parallelize` time.

### Why three workers, not two or four

- **Two workers** (collapsing Worker C into B): would saddle Worker B with the Q24 invariant *and* the admin-ui frontend, exhausting one agent's context on disjoint skills.
- **Four workers** (splitting C's backend + frontend): the promote backend is ~150 LOC and tightly coupled to the Q24 test. Splitting them across two agents would force a contract handshake for what is really one cohesive unit of work.

Three is the natural decomposition. Confirmed by per-worker context budget estimate (each worker projects <60% of effective capacity at fan-out; well within the 85% ceiling).

---

## Context Budget Assessment

| Worker | Assigned scope (LOC est.) | Sibling summaries needed | Interface contracts loaded | Reasoning reserve | Budget |
|---|---|---|---|---|---|
| Worker A (Widget) | ~1200 (greenfield JS/CSS + Playwright tests + oracle) | 0 (independent) | C12 (widget-config) only | generous (novel UX iteration) | ~45% |
| Worker B (Roadmap Backend) | ~1500 (2 migrations + 3 new modules + cache + HTTP) | C6 status workflow (consumed by promote — needs the `Duplicate` transition fact) | C13–C15 (authored) | normal | ~55% |
| Worker C (Promote + UI) | ~1100 (promote handler + Q24 test + 3 admin-ui pages) | C6 + C13–C15 | C16 (authored) | normal | ~60% |

All three under 85% ceiling. No further decomposition required.

---

## Oracle Pre-Build Plan

| Oracle | Question | Consumer(s) | Timing | Status |
|---|---|---|---|---|
| `widget-bundle-size` | Is the built widget JS+CSS bundle ≤ 30720 bytes (FR-FBR-04 cap)? | Worker A; CI gate on every commit touching `widget/` | **Task Zero of Worker A** — build before any widget source lands | not yet built |

**Why mandatory before Worker A writes widget code**: FR-FBR-04's 30KB cap is a contract, not aspiration (arc-plan Testability Gate Q3=4). Without the oracle as the deterministic verifier, the agent will accept "looks small enough" and ship over-budget. With the oracle, every `npm run build` in `widget/` re-evaluates the cap and the inner loop closes per iteration.

**Oracle shape** (specify at Task Zero time; this is the contract Worker A builds against):
- **Probe A — Size**: sum byte count of every file in `widget/dist/*.{js,css,mjs}` (post-minification, post-gzip-pre-state). Cap: `30720` bytes. Emits `FAIL` with current size vs cap if over.
- **Probe B — Tracker domains**: grep `widget/dist/*` for canonical third-party tracker hostnames (`segment.io`, `mixpanel.com`, `google-analytics.com`, `googletagmanager.com`, `intercom.io`, `fullstory.com`, `hotjar.com`, `amplitude.com`). Any hit = FAIL. **Defends DEC-FBR-02 brand promise as a code-level invariant**, not a code-review check.
- **Freshness contract**: trigger-invalidate on `widget/dist/` file hash change. Oracle runs on every commit touching `widget/**`. `.claude/oracles/widget-bundle-size/INDEX.md` registers it.
- **Drift detection (Q5)**: oracle reads built artifacts directly (not from a manifest), so bundler config changes are observed empirically. The tracker hostname list is hashed and re-printed in the oracle report — visible drift if the list shrinks silently.

**Deferrals** (evaluated but not scheduled):
- `roadmap-vote-cache-freshness` (candidate): voting cache misses are observable in app logs (a vote that lands during cache TTL doesn't appear until next tick). Not a security invariant; deferrable to runtime observability. **Defer**: cost (build + maintain) exceeds expected payoff for v1.
- `q24-render-roadmap-body-purity` (candidate): the byte-for-byte inline unit test for `render_roadmap_body` (Q24 invariant) already serves as the deterministic verifier. **Defer**: the test IS the oracle for this surface; an external oracle would be redundant. Already covered by the carry-over from gitcellar's `roadmap_promote.rs` test.

---

## Testability Gate Findings

### FR-FBR-04 — Embeddable widget

Per arc-plan Testability Gate (composite 11, borderline-flagged because Q3=4 + Q4 high-leverage):

| Q | Score | Reasoning |
|---|---|---|
| Q1 | Iteration cost | 3 (browser eval + bundler step) |
| Q2 | Fidelity risk | 3 (Playwright + axe-core has known fidelity bounds; size cap is byte-exact) |
| Q3 | Critical path | **4** (P2 ships when widget ships; widget is THE customer-facing artifact) |
| Q4 | Scaffolding leverage | yes — `widget-bundle-size` oracle + Playwright + axe-core harness halves iteration cycles |
| Q5 | Drift detection | size-budget oracle on every build; a11y harness on every PR commit |

**Recommendation (consumed)**: Both the bundle-size oracle AND a Playwright + axe-core a11y harness exist before main widget code lands. This is reified in **Worker A's Task Zero list** below. P1 Stage 2 already shipped `@axe-core/playwright` 4.10.0 + Playwright 1.48.2 in `admin-ui/`; Worker A reuses the same Playwright runner against the widget's test harness page rather than installing a parallel toolchain.

### FR-FBR-12 — Promote + Q24 invariant

| Q | Score | Reasoning |
|---|---|---|
| Q1 | Iteration cost | 1 (pure-function unit test, no I/O) |
| Q2 | Fidelity risk | 1 (byte-for-byte string assertion) |
| Q3 | Critical path | **4** (privacy regression here is brand-damage; load-bearing per DEC-FBR-02) |
| Q4 | Scaffolding leverage | already covered — the inline unit test IS the verifier |
| Q5 | Drift detection | test file lives next to `render_roadmap_body` — refactor that moves the function MUST move the test |

**Composite 7 — not flagged by score**, but **explicitly recorded as untouchable** in Worker C's task brief and in the eventual module README (`crates/feedbackmonk-api/src/handlers/promote.rs`). Test name and assertions ported **byte-for-byte** from gitcellar `gitcellar-cloud/src/feedback/roadmap_promote.rs` lines 340–368. If a future refactor "tidies" the test or merges it with adjacent assertions, that's a Q24 regression and must be reverted.

### FR-FBR-11 + FR-FBR-13 — Roadmap voting + aggregator

| Q | Score | Reasoning |
|---|---|---|
| Q1 | Iteration cost | 2 (Postgres queries; tokio + sqlx test fixtures exist from P0/P1) |
| Q2 | Fidelity risk | 2 (aggregator semantics testable via `tokio::time::pause`; vote double-prevention is a UNIQUE constraint) |
| Q3 | Critical path | 3 (promote depends on roadmap-items table existing; public roadmap depends on aggregator) |
| Q4 | Scaffolding leverage | partial — existing test fixtures suffice |
| Q5 | Drift detection | UNIQUE constraint on `(item_id, voter_id)` is the load-bearing drift defender |

**Composite 10 — not flagged.** Recorded recommendation for Worker B: a focused integration test that asserts "second vote with same `(item_id, voter_id)` returns `409 conflict` / does NOT increment `vote_count`" is sufficient. No further scaffolding required.

### Items NOT flagged

FR-FBR-04 widget-config endpoint, FR-FBR-11 public roadmap browse, FR-FBR-13 vote-retract: each scored composite < 9. The work surfaces inherit P0/P1 patterns (tenant-scoped repository layer, axum extractors, sqlx queries) where iteration cost is bounded by the existing toolchain.

---

## Ripple Analysis

P2 adds new surfaces; **the only modification to existing files is small**:

| Modified Interface | Consumers | Impact |
|---|---|---|
| `legal_transitions_from(Submitted)` already permits `→ Duplicate` (P1 Stage 1 frozen) | promote handler (Worker C) | Worker C transitions the source feedback to `Duplicate` after roadmap-item insert succeeds — Contract C6 already permits this, no spec change |
| `migrations/` next number | sqlx offline cache, schema-hash for `multi-tenant-isolation-check` oracle | migrations 00006 + 00007 land; oracle's schema-hash bumps; `cargo sqlx prepare` re-runs |
| `crates/feedbackmonk-api/src/router.rs` | binary composition in `main.rs` | Worker B adds `roadmap_router(state)`; Worker C adds `promote_router(state)`; `main.rs::build_app` merges both — same pattern as existing `admin_feedback_routes` merge |
| `.claude/oracles/multi-tenant-isolation-check/allowlist.toml` | the oracle itself | new `SqlxRoadmapItemRepo::new` / `SqlxRoadmapVoteRepo::new` constructor entries (structural mirror of existing `SqlxFeedbackRepo::new` entries); LD-ratifies at PODS convergence |
| `admin-ui/src/shared/router.tsx` | admin-ui routing | Worker C adds public `/roadmap/:project_id` route (no AdminSession required) + admin `/admin/roadmap/:project_id` routes |
| `crates/feedbackmonk-core/src/lib.rs` re-exports | downstream `feedbackmonk-api` consumers | Worker B adds `roadmap::{RoadmapItem, RoadmapItemStatus, RoadmapVote}` exports |

**No deletion or rename of existing requirement IDs.** No breaking change to any P0 or P1 contract. Q24 carryover is additive (a new test in a new file).

---

## Interface Contracts

P2 freezes **five new contracts** (C12–C16). Each MUST be written in detail in the `docs/planning/handoffs/p2-fanout-contracts.md` artifact Worker B authors **before fan-out from `/0-uldf-pods-parallelize`**. The summaries below are load-bearing skeletons.

### Contract C12 — Widget runtime config endpoint (Worker A authors backend; Worker A consumes)

**Endpoint**: `GET /api/v1/projects/{project_id}/widget-config`
**Auth**: none (public; the project_id is the public widget key)
**Repository surface used**: `ProjectRepo::open_for_submission(project_id)` (already allowlisted; pre-auth boundary) + new `TenantRepo::get_widget_brand(&TenantScope) -> WidgetBrand`

**Response shape**:
```json
{
  "project_id": "uuid",
  "tenant_id": "uuid",
  "display_name": "string (project name, customer-visible)",
  "brand": {
    "primary_color": "#3b82f6",
    "logo_url": "https://… or null",
    "footer_text": "powered by feedbackmonk (or null on paid tiers)"
  },
  "auth_modes": ["auth", "anonymous"],
  "submission_kinds": ["bug", "feature", "question", "other"],
  "max_body_chars": 16384
}
```

**Cache**: 60s HTTP `Cache-Control: public, max-age=60` (matches voting cache TTL; trades freshness for response budget — widget mounts re-fetch on TTL miss).
**CORS**: open `Access-Control-Allow-Origin: *` per DEC-FBR-04 (`Domain allowlist for widget embed` is enforced at the SUBMISSION endpoint, not at config; config is public-readable to enable cross-domain embed).

### Contract C13 — Roadmap item schema (Worker B authors; Worker C + future P4 marketing consume)

**Tables** (migration `00006_roadmap_items.sql`):
```sql
CREATE TABLE roadmap_items (
  id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id       UUID NOT NULL REFERENCES tenants(id),
  project_id      UUID NOT NULL REFERENCES projects(id),
  slug            TEXT NOT NULL,           -- URL component; unique per project
  title           TEXT NOT NULL CHECK (char_length(title) BETWEEN 1 AND 200),
  body            TEXT NOT NULL CHECK (char_length(body) BETWEEN 1 AND 16384),
  status          TEXT NOT NULL DEFAULT 'considering'
                   CHECK (status IN ('considering','planned','in-progress','shipped','wontfix')),
  origin_feedback_id UUID REFERENCES feedback(id), -- nullable: admin can create from scratch
  created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
  created_by      UUID NOT NULL,           -- admin user id (P1 admin session)
  updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (project_id, slug),
  UNIQUE (origin_feedback_id)              -- enforces "one roadmap item per source feedback" (idempotency)
);
CREATE INDEX roadmap_items_tenant_project_status_idx
  ON roadmap_items (tenant_id, project_id, status);
```

**Status state machine** (no audit-history table — admin can edit freely; the log lives in app-level activity later):
- `considering → planned → in-progress → shipped` (forward path)
- any → `wontfix` (close)
- `shipped` is terminal

### Contract C14 — Roadmap voting schema + double-vote prevention (Worker B authors)

**Tables** (migration `00007_roadmap_votes.sql`):
```sql
CREATE TABLE roadmap_votes (
  id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id     UUID NOT NULL REFERENCES tenants(id),
  project_id    UUID NOT NULL REFERENCES projects(id),
  item_id       UUID NOT NULL REFERENCES roadmap_items(id) ON DELETE CASCADE,
  voter_id      TEXT NOT NULL,             -- JWT `sub` (auth mode) OR hashed anon cookie (anon mode)
  voter_mode    TEXT NOT NULL CHECK (voter_mode IN ('jwt','anon')),
  cast_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (item_id, voter_id)               -- Q5 drift defender for "1 vote per (item, voter)" rule
);
CREATE INDEX roadmap_votes_item_id_idx ON roadmap_votes (item_id);
```

**Hard invariants** (worker tests assert):
1. INSERT with duplicate `(item_id, voter_id)` returns `Err(RepoError::UniqueViolation)` → handler maps to `409 Conflict`. NOT silent upsert.
2. `voter_id` in anon mode is the SAME hash function the submission endpoint uses (`feedbackmonk_anon::token_hash`) — re-using the canonical chokepoint, no parallel implementation.
3. Retracting a vote (`DELETE`) is permitted within 60s of `cast_at`; after that returns `403`. Defends against vote-brigading toggle abuse.

### Contract C15 — Public roadmap HTTP API + 60s aggregator cache (Worker B authors)

**Endpoints**:
```
GET  /api/v1/projects/{project_id}/roadmap?status=&limit=&offset=
GET  /api/v1/projects/{project_id}/roadmap/top-voted?limit=N      (N default 10, max 50)
GET  /api/v1/projects/{project_id}/roadmap/items/{slug}
POST /api/v1/projects/{project_id}/roadmap/items/{slug}/vote      (auth via JWT or anon cookie)
DELETE /api/v1/projects/{project_id}/roadmap/items/{slug}/vote    (retract; 60s window)

GET    /api/v1/admin/projects/{project_id}/roadmap                (admin list including drafts/wontfix)
POST   /api/v1/admin/projects/{project_id}/roadmap/items          (admin create from scratch)
PATCH  /api/v1/admin/projects/{project_id}/roadmap/items/{slug}   (admin edit title/body/status)
```

**Voting cache**:
- Pattern ported from gitcellar `roadmap_voting.rs` `GLOBAL_VOTING_CACHE: LazyLock<VotingCache>`.
- TTL: `60s` (constant `VOTING_CACHE_TTL_SECS`).
- Refresh tick: `tokio::spawn` task started from `main.rs` after `build_state` succeeds; fires immediately at startup then every 60s.
- Cache miss returns the previous tick's data + `cached_at` timestamp; empty cache returns `items: [], cached_at: null` (never error).
- Data source: SQL `SELECT … FROM roadmap_items JOIN roadmap_votes … GROUP BY item_id ORDER BY vote_count DESC LIMIT N` — replaces gitcellar's Forge HTTP roundtrips with native Postgres.

**Auth-mode resolution at vote time** (mirrors submission endpoint):
- `Authorization: Bearer` present → `jwt_verify_with_leeway` → `voter_id = claims.sub`, `voter_mode = 'jwt'`
- absent → read `X-Feedbackmonk-Anon-Cookie`, `feedbackmonk_anon::token_hash(ip, cookie, project_id)` → `voter_id = hex(hash)`, `voter_mode = 'anon'`
- rate-limit via existing `AnonGate` for anon mode

### Contract C16 — Promote-to-roadmap action (Worker C authors)

**Endpoint**: `POST /api/v1/admin/feedback/{feedback_id}/promote`
**Auth**: `AdminSession` (existing P1 auth extractor)
**Request body**: `{ "slug": "string (1..=80)", "title": "string (1..=200, optional — defaults to render_roadmap_title(feedback.body))" }`

**Response shape**:
```json
{
  "roadmap_item_id": "uuid",
  "roadmap_item_slug": "kebab-string",
  "source_feedback_id": "FB-XXXXXX",
  "source_status": "duplicate",     -- transition just applied
  "already_promoted": false         -- true on idempotent re-call
}
```

**Hard invariants** (Worker C asserts in `crates/feedbackmonk-api/src/handlers/promote.rs::tests`):

1. **Q24 — body**: `render_roadmap_body(message: &str) -> String` is byte-for-byte ported from gitcellar `gitcellar-cloud/src/feedback/roadmap_promote.rs::render_roadmap_body`. Test name: `q24_roadmap_body_excludes_fb_id_and_username`. Assertions: body MUST NOT contain `"FB-"`, MUST NOT contain framing strings `["Originally submitted by", "Submitter:", "Submitted by user_id", "From user:"]`, MUST contain the original message content (trimmed). **Untouchable** — documented in promote module README + assertions copied character-for-character.
2. **Q24 — title**: `render_roadmap_title(message: &str) -> String` byte-for-byte ported. Test `q24_roadmap_title_excludes_added_fb_framing`. Assertions: title MUST NOT start with `"[FB-"` or `"FB-"`. Title length cap matches gitcellar's `TITLE_MAX_CHARS` constant ported as-is.
3. **Category gate**: source feedback's `kind` MUST be `FeedbackKind::Feature` (matches gitcellar's `category != "feature_request"` check). Other kinds → `400 InvalidCategory`.
4. **Idempotency**: a second promote of an already-promoted feedback returns `200 OK` with `already_promoted: true` and the existing roadmap-item slug. Enforced by `roadmap_items.origin_feedback_id UNIQUE` constraint + repo-level "fetch-existing-on-violation" pattern (mirrors gitcellar's `get_existing_promotion`).
5. **Atomic status transition**: after roadmap-item INSERT succeeds, the source feedback's status transitions to `Duplicate` **in the same DB transaction** via the existing `FeedbackRepo::update_status_in_executor` + `FeedbackStatusHistoryRepo::append_in_executor` `_in_executor` overloads (Contract C6 Hard Invariant #4). Audit row reason = `"promoted to roadmap"`; `duplicate_of_feedback_id` is set to `NULL` (this is a roadmap promotion, not a duplicate-of-feedback merge — distinct sub-category captured in `feedback_status_history.transition_reason`).
6. **PII scrubbing chokepoint**: any structured log emitted from the promote handler inherits the workspace-wide `feedbackmonk_tracing` scrubber automatically — `pii-scrub-audit` oracle's Probe A enforces this at commit time (no `tracing_subscriber` setup outside `crates/feedbackmonk-tracing/`). No new chokepoint required.

---

## Per-Worker Tasks

### Worker A — Widget (FR-FBR-04)

**Owns**: `widget/` (new top-level directory) + `.claude/oracles/widget-bundle-size/` + `crates/feedbackmonk-api/src/handlers/widget_config.rs` (NEW) + tenant brand additions in `crates/feedbackmonk-repository/src/tenants.rs`.

**Task list**:
1. **Task Zero**: build `widget-bundle-size` Verification Oracle (Python canonical + sh shim + ps1 shim matching `pii-scrub-audit`/`multi-tenant-isolation-check` patterns). Register in `.claude/oracles/INDEX.md`. Probe A + Probe B per § Oracle Pre-Build Plan above. Oracle MUST be GREEN against an empty `widget/dist/` (initial state).
2. **Scaffold `widget/`**: `package.json` (vanilla JS+CSS, NO React, NO framework — vite as bundler only), `vite.config.ts` with minify+terser+CSS-minify, `tsconfig.json`. Bundle target: ES2020+, single output `widget/dist/widget.js` + `widget/dist/widget.css`. Strict CSP-compatibility: no inline scripts, no `eval`, no `Function()` constructor.
3. **Widget core** (`widget/src/widget.ts`): mount function `mountFeedbackMonk(config)` that:
   - Reads `data-project-id` from the embedding script tag.
   - Fetches `GET /api/v1/projects/{project_id}/widget-config` (Contract C12) once on mount; on TTL miss the next mount re-fetches.
   - Renders a launcher button (bottom-right, `position: fixed`, themed by `brand.primary_color`).
   - On click, opens a modal with subject + body + kind dropdown + optional email field (anon mode) + submit button.
   - On submit: POST to `/api/v1/projects/{project_id}/feedback` with `Authorization: Bearer ${jwt}` if the host page supplied one via `data-jwt` or `mountFeedbackMonk({ jwt })`, else anon mode (no Authorization header).
   - On success: success toast + close modal. On failure: surface `error.code` + retry button.
   - Renders the "powered by feedbackmonk" footer when `brand.footer_text` is non-null.
4. **A11y**: keyboard-trap inside modal, ESC to close, focus-return to launcher button, ARIA roles per WCAG AA. NO third-party trackers (DEC-FBR-02 brand promise — Probe B of the oracle enforces).
5. **Playwright + axe-core harness** in `widget/e2e/widget-a11y.spec.ts`: mount widget in a fixture HTML page, run `@axe-core/playwright` analyze on idle + after modal-open. Reuse `admin-ui/`'s existing `@axe-core/playwright@4.10.0` + `@playwright/test@1.48.2` toolchain (NO new top-level node deps; widget's `package.json` references admin-ui's devDeps via workspace pattern OR ships its own minimal lockfile — Worker A picks at fan-out time).
6. **`widget-config` HTTP endpoint** (`crates/feedbackmonk-api/src/handlers/widget_config.rs`): wire `GET /api/v1/projects/{project_id}/widget-config` into the router. Hardcoded `auth_modes: ["auth","anonymous"]` and `submission_kinds: [...]` for v1 (P3 tier flag enables/disables footer).
7. **Tenant brand surface extension**: add `TenantRepo::get_widget_brand(&TenantScope) -> WidgetBrand` (mirrors existing `TenantRepo::get_brand` from P1). `WidgetBrand` lives in `crates/feedbackmonk-core/src/models.rs` next to `EmailTenantBrand`.

**Exit gate for Worker A**:
- `widget-bundle-size` oracle GREEN (Probe A reports current size — must be <= 30KB; Probe B asserts no tracker hostnames).
- Playwright a11y harness GREEN (zero axe-core violations against modal-open + modal-closed states).
- `cargo build` + `cargo clippy --workspace --all-targets -- -D warnings` GREEN.
- `widget-config` endpoint returns valid Contract C12 JSON for a seeded project.
- E2E smoke (`scripts/e2e-p2-widget-curl.sh` or equivalent): submission via the widget's bundled JSON shape reaches `feedback` table.

### Worker B — Roadmap Backend (FR-FBR-11 + FR-FBR-13)

**Owns**: `migrations/0000{6,7}_*.sql` + `crates/feedbackmonk-core/src/roadmap.rs` (NEW) + `crates/feedbackmonk-repository/src/roadmap_items.rs` + `crates/feedbackmonk-repository/src/roadmap_votes.rs` (NEW) + `crates/feedbackmonk-api/src/handlers/roadmap.rs` (NEW, public + admin endpoints split into submodules) + `crates/feedbackmonk-api/src/roadmap_voting_cache.rs` (NEW) + allowlist updates.

**Task list**:
1. **Author `docs/planning/handoffs/p2-fanout-contracts.md`** with C12–C16 written in full **before** any code lands. This is Worker B's first task because Worker C is gated on the contract being frozen. Worker A's C12 can be authored by Worker A or by Worker B as a courtesy at fan-out.
2. **Migrations**: `00006_roadmap_items.sql` per Contract C13; `00007_roadmap_votes.sql` per Contract C14.
3. **Core types** (`crates/feedbackmonk-core/src/roadmap.rs`): `RoadmapItem`, `RoadmapItemStatus` (5-variant enum, kebab-case serde), `RoadmapVote`, `RoadmapVoterMode`. Re-export from `lib.rs`.
4. **Repository surfaces**: `RoadmapItemRepo` trait + `SqlxRoadmapItemRepo` impl (constructor allowlisted as structural mirror); `RoadmapVoteRepo` trait + `SqlxRoadmapVoteRepo` impl. All methods take `&ProjectScope` as their first non-self arg (Probe B compliance). `get_existing_promotion(project_scope, origin_feedback_id)` mirrors gitcellar's helper for idempotent promote.
5. **Voting cache** (`crates/feedbackmonk-api/src/roadmap_voting_cache.rs`): `VotingCache: Arc<RwLock<CacheInner>>` + `spawn_refresh_tick(state)` started from `main.rs::main`. TTL 60s. Cold-start tick runs immediately. Use `tokio::time::interval`. Failure mode: tick logs WARN, cache stays as last good value (or empty if cold).
6. **HTTP handlers** (`crates/feedbackmonk-api/src/handlers/roadmap.rs`):
   - Public `roadmap_router(state) -> Router`: list + top-voted + detail + vote + retract.
   - Admin `admin_roadmap_router(state) -> Router`: list (including drafts) + create + edit. `AdminSession` extractor.
   - Both routers merged into `build_app` in `main.rs`.
7. **Allowlist update**: add `[[inherent_methods]]` entries for `SqlxRoadmapItemRepo::new` and `SqlxRoadmapVoteRepo::new` to `.claude/oracles/multi-tenant-isolation-check/allowlist.toml` with the structural-mirror rationale.
8. **Tests**:
   - Unit: cache TTL semantics (use `tokio::time::pause`); `RoadmapItemStatus` round-trip; double-vote returns conflict.
   - Integration (sqlx): one happy-path vote + one duplicate-vote returning 409.
   - Witness: existing `multi-tenant-isolation-check` re-runs GREEN with the new repos.
9. **Sqlx offline cache regenerate**: `cargo sqlx prepare --workspace`; commit the new `.sqlx/` entries.

**Worker B contract-freeze handshake** (mid-phase sync point):
- Worker B writes `docs/planning/handoffs/p2-fanout-contracts.md` complete with C13/C14/C15/C16 (C16's signature is Worker C's to author the body of, but the endpoint+request+response shape is frozen here so Worker C can begin its admin-ui work in parallel).
- Worker B posts to `channels/messages.md` `[B → C] Contracts frozen at <commit>` once the file is committed.
- Worker C may scaffold pages and write tests against the contracts but does NOT integrate with backend until the C16 endpoint compiles.

**Exit gate for Worker B**:
- Both migrations apply cleanly + rollback cleanly.
- `multi-tenant-isolation-check` oracle GREEN.
- All new repo methods take `&ProjectScope`/`&TenantScope` first arg (or are allowlisted with rationale).
- Public + admin endpoints curl-able end-to-end against a seeded project.
- Voting cache observable via `/api/v1/projects/{id}/roadmap/top-voted` returns `cached_at` field; `cached_at` advances after 60s.
- Workspace tests pass (current count 218 → ~250+).

### Worker C — Promote + UI (FR-FBR-12 + admin/public roadmap UI)

**Owns**: `crates/feedbackmonk-api/src/handlers/promote.rs` (NEW) + `admin-ui/src/pages/roadmap/{PublicRoadmap.tsx,AdminRoadmap.tsx,PromoteButton.tsx}` (NEW) + admin-ui routing + admin-ui types refresh.

**Task list**:
1. **Promote handler** (`crates/feedbackmonk-api/src/handlers/promote.rs`):
   - `pub fn render_roadmap_title(message: &str) -> String` — byte-for-byte port from gitcellar.
   - `pub fn render_roadmap_body(message: &str) -> String` — byte-for-byte port from gitcellar.
   - `pub async fn promote_handler(...)` — full pipeline per Contract C16: validate kind, idempotency check via `get_existing_promotion`, render title+body, INSERT roadmap_item (re-use existing constraint UNIQUE-violation handling on `origin_feedback_id`), atomic transition source feedback → Duplicate in same txn, return outcome.
   - Tests (`#[cfg(test)] mod tests`): port gitcellar tests `q24_roadmap_body_excludes_fb_id_and_username`, `q24_roadmap_title_excludes_added_fb_framing`, `render_roadmap_title_truncates_long_messages`, `render_roadmap_title_collapses_newlines`, `render_roadmap_body_invites_voting`, `truncate_with_ellipsis_preserves_short_input` **byte-for-byte**. Test names + assertions unchanged.
   - Module README at `crates/feedbackmonk-api/src/handlers/promote.md` (or inline at top of file): ULADP Agent Context Header + Decision Log entry: *"Q24 byte-for-byte invariant — DO NOT refactor `render_roadmap_body` without porting the test verbatim. See DEC-FBR-02 brand promise."*
2. **Promote endpoint wiring**: add `POST /api/v1/admin/feedback/:feedback_id/promote` to admin router. AdminSession extractor.
3. **Admin UI roadmap pages** (`admin-ui/src/pages/roadmap/`):
   - `AdminRoadmap.tsx`: list view of all roadmap items for current project (admin sees drafts + wontfix); create-new + edit modals; status dropdown.
   - `PromoteButton.tsx`: button placed in `FeedbackDrawer.tsx` (existing P1 component) — visible only when `feedback.kind === 'feature'` and `feedback.status !== 'duplicate'`. On click: prompt for slug + optional title → `POST /promote` → toast → close drawer + redirect to admin roadmap.
   - `PublicRoadmap.tsx`: public unauthenticated route (renders for any visitor). Fetches `GET /roadmap` + `GET /roadmap/top-voted`. Vote button posts with JWT (if host page supplied) or anon-cookie (if not). Lists items grouped by status. Shows `cached_at` timestamp footer.
4. **Routing changes** (`admin-ui/src/shared/router.tsx`): add admin route `/admin/projects/:projectId/roadmap` (behind AdminSession) + public route `/public/projects/:projectId/roadmap` (no auth). Public route uses a separate layout (no admin chrome).
5. **Type generation**: regenerate `admin-ui/src/shared/types.gen.ts` to include the new C16 promote response + C13/C14/C15 types from Worker B's source-of-truth.
6. **Tests**:
   - Vitest: `PromoteButton` only renders when kind=feature + status≠duplicate; PromoteFlow happy path mocked against MSW; PublicRoadmap renders items grouped by status; AdminRoadmap status edit fires PATCH.
   - Playwright + axe-core: public roadmap page passes axe (extension of existing admin-ui Playwright harness).
7. **Justification artifact** (if any byte-for-byte port test is modified — including formatting): write `docs/test-modifications/<date>-<slug>.md` per Anti-Reward-Hacking Gate § 0.5. Worker C is on **autopilot read-only-tests** mode; the byte-for-byte ports themselves are net-new tests (creating tests is permitted; modifying existing tests is what's gated).

**Exit gate for Worker C**:
- `cargo test --workspace` includes the 6 ported tests, all PASS.
- Promote endpoint end-to-end: create feature-feedback → promote → source feedback status = `duplicate` + roadmap_items row exists + idempotent re-call returns `already_promoted: true`.
- Vitest pass (admin-ui tests).
- Playwright a11y smoke green for the new public roadmap page.
- Module README at `promote.md` (or inline header) documents the Q24 untouchable invariant.

---

## Coordination Requirements

| Sync point | Owner | Mechanism | Blocks |
|---|---|---|---|
| Contract C12 frozen | Worker A or Worker B (decide at fan-out) | written into `docs/planning/handoffs/p2-fanout-contracts.md` | nothing — A is independent |
| Contracts C13/C14/C15/C16 frozen | Worker B | written into `docs/planning/handoffs/p2-fanout-contracts.md` + `channels/messages.md` ping | Worker C's backend implementation work (admin-ui scaffolding against the typed contract may proceed in parallel) |
| `multi-tenant-isolation-check` allowlist update | Worker B | commit to `.claude/oracles/multi-tenant-isolation-check/allowlist.toml` | none directly; convergence-time LD ratifies |
| Sqlx offline cache regenerate | Worker B (primary); Worker C re-runs if their promote handler adds queries | `cargo sqlx prepare --workspace` | the merge commit (cache must be in lockstep with migrations) |
| Channels file ownership | LD (orchestrator) | `channels/messages.md`, `status.md`, `touches.json` per PODS protocol | three workers write status updates; LD reads + ratifies decisions |
| Convergence | LD via `/0-uldf-pods-converge` | critic verdict + reviewer agent (P1 Stage 2 pattern) | P2 close + P1→P2 finalize trigger |

---

## Deferred Decisions

| Decision | Deferred Until | Default if Unresolved | Why Defer |
|---|---|---|---|
| Should the public roadmap UI live in `admin-ui/` or a future separate `web/` workspace? | P4 planning round | Keep in `admin-ui/` for v1 — pragmatic given Vite/React toolchain reuse. P4 can extract if the marketing site (Astro) takes ownership of public surfaces. | Splitting toolchains pre-launch adds calendar cost; the admin-ui name is a slight misnomer but the routing already separates `/admin/*` (AdminSession) from `/public/*` (open). |
| Roadmap status iconography + branding | P4 brand pass (DEC-FBR-09) | Plain text status labels + colored chips (steal palette from gitcellar's roadmap UI patterns) | Real brand pass is P4; placeholders OK. |
| Per-tenant footer opt-out on paid tiers (FR-FBR-14 ties this to tier state) | P3 tier-enforcement work | Free-tier default `footer_text: "powered by feedbackmonk"` for v1 of the widget; P3 wires the flag-flip | P3 owns tier enforcement; threading tier state through C12 now would couple this phase to billing logic. |
| Custom slugs vs auto-generated slugs for promote | Worker C at handler-author time | Allow admin to specify; default auto-generate from `render_roadmap_title` lower-cased + dashed | Low-stakes UX decision; deferrable to Worker C's discretion. |
| Vote retraction window length (currently proposed 60s) | Worker B at handler-author time | 60s | Mid-phase decision; if Worker B finds 60s too short for UX or too long for abuse, can flex 30–120s in their final commit. |

---

## Risks and Mitigations

| Risk | Likelihood | Severity | Mitigation |
|---|---|---|---|
| Widget bundle creeps past 30KB during a11y polish | Medium | High | `widget-bundle-size` oracle as Task Zero. Every commit re-checks. Force the agent to choose between dropping a feature or shipping over-budget — never silent. |
| Q24 regression in a future P2 refactor (e.g. someone "tidies" `render_roadmap_body`) | Low | **High** | Byte-for-byte test ported from gitcellar; module README documents as untouchable; PR template note (P4 work) reinforces. The test failure message itself includes "Q24 violation". |
| Voting cache cold-start race (refresh tick not yet warm + first request) | Low | Low | Cache returns `items: [], cached_at: null` empty state — never an error. Gitcellar pattern proven. |
| Worker C blocked waiting for Worker B's contract freeze | Medium | Medium | Mid-phase handshake at `docs/planning/handoffs/p2-fanout-contracts.md`. Worker C scaffolds UI against the typed contract in parallel; backend implementation proceeds once Worker B's compiled handler signatures land. |
| Public roadmap exposes draft items to anon visitors | Low | High | Admin endpoints (`/api/v1/admin/projects/.../roadmap`) require AdminSession; public endpoints filter `status IN ('considering','planned','in-progress','shipped','wontfix')` AND there's no concept of "draft" — admin creates items directly into `considering`, no separate draft state in v1. |
| Vote double-prevention silently breaks under concurrent inserts | Low | High | UNIQUE constraint on `(item_id, voter_id)` is the load-bearing defender. Integration test asserts the 409 path. |
| `widget/` directory name conflicts with future workspace name | Low | Low | Cargo workspace doesn't see `widget/` (it's a node project). No path collision. |
| Same-branch merge conflict during PODS run | None expected | n/a | Worker file-touches disjoint (A→`widget/`, B→`crates/` + `migrations/`, C→`promote.rs` + `admin-ui/src/pages/roadmap/`). P1 Stage 2 same-branch precedent. |
| Anon-cookie vote replay attack across project boundaries | Low | High | `feedbackmonk_anon::token_hash` is `(ip, cookie, project_id)` — already project-scoped. Re-use of the canonical chokepoint enforces (Contract C14 invariant 2). |
| In-process voting cache loses state on restart | Medium | Low | Acceptable for P2 (matches anon-rate-limit in-memory pattern, DISCOVERY D-FBR-08 deferred to v1.1). Cache rebuilds from DB within 60s. |

---

## Execution Commands

**Recommended next step**: `/0-uldf-proceed`

The router will evaluate context budget + work shape and pick the topology. Likely outcome at autopilot:continuous:

1. **HANDOFF to a fresh session for `/0-uldf-pods-parallelize`** (this session has consumed plan-authoring + reference-survey context budget; PODS-LD work benefits from a fresh ~1M-context orchestrator).
2. The successor session runs `/0-uldf-pods-parallelize "feedbackmonk P2 — Customer-Facing"` with this plan as input.
3. PODS spawns 3 workers via `/0-uldf-pods-spawn-collaborator --all` (Worker A widget / Worker B roadmap-backend / Worker C promote+UI).
4. LD monitors via `/0-uldf-pods-collab-sync`, ratifies mid-phase contract freeze, runs `/0-uldf-pods-converge` at completion.
5. At convergence: `/0-uldf-finalize --skip-push` (per CLAUDE.md push gate — PF-REGISTER-01 not yet executed) → P2 exit → P3 plan via fresh `/0-uldf-ldis-plan`.

Direct invocation (if `/0-uldf-proceed` is unavailable):
```
/0-uldf-pods-parallelize "feedbackmonk P2 — Customer-Facing" --from-plan=docs/planning/plans/20260514T034730-feedbackmonk-p2-customer-facing.md
```

---

## Notes for Downstream Consumers

- **`/0-uldf-pods-parallelize`**: reads this plan as input (`--from-plan=` flag or auto-resolves latest plan). The 3-worker decomposition + per-worker task lists above are the canonical worker briefs. Worker spawn prompts cite Contract IDs (C12–C16) rather than re-paste.
- **`/0-uldf-pods-spawn-collaborator`**: each worker prompt cites:
  - the contracts the worker owns (C12 / C13–C15 / C16 respectively)
  - the cross-references to gitcellar reference files (for B and C)
  - the read-only-tests autopilot mode applies — net-new tests are permitted; modifying existing P0/P1 tests requires a justification artifact.
- **`/0-uldf-pods-converge`**: convergence criteria include:
  - all three workers' exit gates GREEN
  - `widget-bundle-size` oracle GREEN
  - `multi-tenant-isolation-check` oracle GREEN with allowlist additions ratified
  - `pii-scrub-audit` oracle GREEN
  - Q24 byte-for-byte tests present in `crates/feedbackmonk-api/src/handlers/promote.rs::tests` with identical names + assertions to gitcellar source
  - critic verdict PASS (will surface as `C-P2-XXX` carry-forwards if any)
- **`/0-uldf-finalize`**: P2 ends with `--skip-push` until PF-REGISTER-01 clears (CLAUDE.md "Constraints not in artifacts"). PF-RENAME-02 (working-dir rename) is user-action and does not block.

---
