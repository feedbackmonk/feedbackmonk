# Execution Plan
**Source**: /0-uldf-ldis-plan
**Generated**: 2026-07-01T16:12:00
**Task**: feedbackmonk Phase A — GitCellar contract build-out (5 additive contract surfaces + capability flags + deploy/verify gate)
**Strategy**: STAGED
**Intake Source**: docs/planning/intakes/20260701T160735-phase-a-gitcellar-contract-buildout.md

---

═══════════════════════════════════════════════════════════════
       LDIS EXECUTION PLAN
═══════════════════════════════════════════════════════════════

Task: Add Phase-A end-user contract surfaces to FeedbackMonk so GitCellar
(tenant #1) can delete its internal feedback backend. Every new capability is
advertised via `GET /api/v1/capabilities`. Deploy + verify to
`feedback.gitcellar.com` is the exit GATE for GitCellar's Phases B/C.

Strategy: **STAGED** (shared foundation → parallelizable streams → deploy gate)

───────────────────────────────────────────────────────────────
✅ CONFIRMATION GATE — CLEARED (user confirmed 2026-07-01)
───────────────────────────────────────────────────────────────

**User confirmed all three recommendations** ("go with your recommendations")
and set **autonomy = autopilot** for the arc. Development execution routed to the
**Fable** model (per user direction), with this Opus 4.8 session coordinating +
owning the shared seams (capabilities array, contract doc, integration, review).
D-A1/D-A4/D-A5 below are now CONFIRMED, not assumptions.

Three contract-freezing decisions (originally adopted as AFK defaults, now
CONFIRMED):

- **D-A1** Erasure = **hard-delete row + full FK cascade + explicit object-store
  byte purge**. (Alt: tombstone/redact.)
- **D-A4** `severity` = **optional** field, values **`low|medium|high|blocker`**,
  accepted on submit in auth + anon modes, first-class column. (Alt: `critical`
  instead of `blocker`.)
- **D-A5** `GET …/me/feedback/export` = **IN scope** for Phase A. (Alt: defer.)

Stage 0 (oracles, trait scaffolding, migration reservation) is decision-agnostic
and may begin immediately; Stage 1 handler/migration work waits on confirmation.

───────────────────────────────────────────────────────────────
STRATEGY RATIONALE + COLLABORATION VALUE
───────────────────────────────────────────────────────────────

Scope: MEDIUM. Value factors (intake): Specialization 3 / Quality 3 / Discovery 2
/ Speed 3 = 11/20. Friction: Boundary Clarity 4 / Coupling 3 = 7/10.
Net = 11 − 3.5 = **7.5 → STAGED** (parallel within the middle stage, careful
boundaries). NOT hierarchical (single LD holds the whole surface easily); NOT
enterprise.

Why STAGED and not flat-parallel-from-t0: two shared foundations must exist
before the streams can land cleanly —
1. **`ObjectStore` trait extension** (`delete` + `get`): confirmed the trait is
   `put`-only today (`crates/feedbackmonk-api/src/storage.rs`). A1 (byte purge)
   needs `delete`; A2 (tenant-scoped download) needs `get`. Both LocalFs + S3
   backends must implement them (S3 = two NEW SigV4-signed verbs — the module's
   "exactly one S3 verb" assumption is deliberately broken here). If A1 and A2
   each add their own half independently, they collide on the same trait + both
   backends. Build the trait extension ONCE in Stage 0.
2. **Migration numbering** (00020+): sequential; must be assigned by the LD, not
   raced by parallel streams.

After Stage 0, the three streams are cleanly separable by touched file, so Stage
1 parallelizes (flat PODS) OR runs HERE sequentially — `/0-uldf-proceed` picks by
context budget. Stage 2 (deploy/verify) is a single serialized gate.

Topology note: this is comfortably a **single-session HERE** job (the whole
surface is additive and well-specified, ~10-14 files). Flat PODS is available if
speed matters, but the coordination overhead (shared capabilities.rs + contract
doc + migration numbers) makes HERE-sequential-by-stream the low-friction default.
Recommend `/0-uldf-proceed` evaluate at the Stage 1 boundary.

───────────────────────────────────────────────────────────────
CTD PLAN
───────────────────────────────────────────────────────────────

Model tiering: **OFF (uniform default model)** — `ctd.modelTiering` not enabled.
CTD-01 would be TRUE (5 genuinely-independent units with verifiable boundaries),
so the partition + assembly plan below stands, but every block runs at the
default (frontier) model. Decomposition, capability-array assembly, and the
deploy/verify gate are frontier by definition (the seams).

───────────────────────────────────────────────────────────────
CONTEXT BUDGET
───────────────────────────────────────────────────────────────

Every stream fits one context with wide margin (each ≈ 1 migration + 1 repo
method-set + 1 handler + tests + doc-delta ≈ 15–35k tokens incl. reasoning). No
budget pressure; no decomposition beyond the streams below. Pass.

───────────────────────────────────────────────────────────────
COMPONENT BREAKDOWN
───────────────────────────────────────────────────────────────

### STAGE 0 — Shared foundation (decision-agnostic; can start now)

**S0.1 — `ObjectStore` trait extension** (`storage.rs`)
- Add `async fn delete(&self, key: &str) -> Result<(), StorageError>` and
  `async fn get(&self, key: &str) -> Result<Vec<u8>, StorageError>` to the trait.
- LocalFs: `tokio::fs::remove_file` (idempotent — treat NotFound as Ok for
  delete) + `tokio::fs::read`; keep the traversal guard.
- S3: two new SigV4-signed requests (DELETE object; GET object). Reuse the
  existing `SigV4` signer + `canonical_path`/`object_url`. Add unit coverage
  mirroring the existing PUT/round-trip tests (LocalFs delete+get round trip;
  S3 signer path exercised by the existing vector — the new verbs reuse it).
- Update the module doc (it currently says "exactly one S3 verb (PUT)").

**S0.2 — Verification Oracle: `feedback-erasure-completeness`** (Task Zero of A1)
- `.claude/oracles/feedback-erasure-completeness/` — after a delete, assert NO
  residual PII: body gone, cascade children gone (replies/status-history/
  attachments/moderation/board-votes), and NO object remains under the feedback's
  attachment storage prefix. Detection-from-code + `--full` behavioral leg
  (mirrors the project's approval-gate / moderation-gate / translation-egress
  oracle pattern). This is the anti-reward-hacking leg for the P0 privacy item.

**S0.3 — Migration number reservation**
- Assign: `00020` severity (A4), `00021` submit-idempotency (A4). A1/A2/A3/A5 need
  NO new tables (A1 = delete path; A2 = read existing `attachments`; A3 = derived
  columns in the query OR one `feedback.updated_at` column → if materialized,
  reserve `00022`; A5 = read path). Decide A3's column-vs-derived approach at
  Stage 1 start (see A3). Reserve conservatively; collapse unused numbers before
  commit.

### STAGE 1 — Parallelizable streams (after CONFIRMATION GATE)

**Stream 1 — me_feedback surface** (A1 + A3 + A5; owner of `me_feedback.rs`)
Cohesive: all three touch `me_feedback.rs` + the end-user repository reads.

- **A1 · DELETE `…/me/feedback/{id}` (P0 erasure)** — fully-scoped leaf
  - Handler: `delete_my_feedback` — same JWT-sub auth as list/thread; resolve the
    row scoped to `claims.sub` (404 if not owned/anonymous/other project — no
    existence oracle, mirror `get_for_end_user`).
  - Repo: `FeedbackRepo::list_attachment_keys_for_end_user(scope, sub, fb)` then
    `delete_for_end_user(scope, sub, fb)`. Order: **fetch attachment storage_keys
    → delete each object via `ObjectStore::delete` → DELETE feedback row** (FK
    cascade removes attachment rows + replies + status-history + moderation +
    board-votes; roadmap origin + duplicate-of set NULL). Byte purge BEFORE row
    delete so a mid-failure leaves rows (retryable) rather than orphaned bytes.
  - Idempotent: second delete → 404 (already gone) or 204; pick 204-on-first,
    404-on-absent. Response: `204 No Content` on success.
  - Capability: `feedback.delete`. Contract doc: new §12.
  - Tests: `tests/me_feedback_delete.rs` — deletes own row (gone from list +
    thread 404), cannot delete another sub's row (404, row survives), byte purge
    verified, cascade verified. Wire the S0.2 oracle `--full` leg.

- **A3 · list `updated_at` + `reply_count` (P1)** — fully-scoped leaf
  - Approach decision (Stage 1 start): **derived-in-query** (recommended — no
    migration): `reply_count` = COUNT of PUBLIC replies; `updated_at` =
    `greatest(accepted_at, max(public reply created_at), max(status_history
    changed_at))`. Avoids a materialized column + write-path bumps. Add optional
    `?since=<rfc3339>` filtering rows with `updated_at > since`.
  - Extend `MeFeedbackItem` (+ thread if useful) with `updated_at` + `reply_count`.
    Additive to the frozen §6 shape — safe (clients ignore unknown fields).
  - Capability: `feedback.reply_state`. Contract doc: update §6.1 + §11.
  - Tests: extend `me_feedback_isolation.rs` / new `me_feedback_reply_state.rs` —
    reply bumps updated_at + count; internal replies do NOT count (privacy);
    `?since=` filters correctly.

- **A5 · GET `…/me/feedback/export` (P1, D-A5)** — fully-scoped leaf
  - Handler `export_my_feedback` — same JWT-sub auth; returns the caller's full
    feedback (all rows, bodies, sentiments, kinds, statuses, public replies,
    attachment metadata/urls) as a single JSON document (portability companion to
    A1). No pagination (it's an export); reuse list + thread repo reads.
  - Capability: `feedback.export`. Contract doc: new §13.
  - Tests: `tests/me_feedback_export.rs` — exports only own rows; shape stable;
    no other-user leakage; internal replies excluded.

**Stream 2 — submit surface** (A4; owner of `feedback.rs`)
- **A4a · first-class `severity`** — fully-scoped leaf
  - Migration `00020`: `feedback.severity TEXT NULL CHECK (severity IN
    ('low','medium','high','blocker'))`. Nullable/optional (tenant-generic; NOT
    required-for-bug like GitCellar's internal rule).
  - `feedbackmonk-core`: `Severity` enum (parse/as_db_str, mirror `Sentiment`).
  - `feedback.rs`: parse `severity` body field (auth + anon), 400 on bad value,
    echo it; thread through `submit_authenticated`/`submit_anonymous`. Surface in
    me_feedback list/thread/export (Stream 1 coordinates the shared struct add).
  - Capability: `feedback.severity`. Contract doc: update §5.5 + §9-adjacent + §11.
- **A4b · submit `Idempotency-Key` dedupe** — fully-scoped leaf
  - Migration `00021`: `submit_idempotency (project_id, idempotency_key, feedback_id,
    created_at, PRIMARY KEY(project_id, idempotency_key))` (tenant-generic; TTL
    sweep optional/deferred).
  - `feedback.rs`: read `Idempotency-Key` header; if seen for this project, return
    the ORIGINAL `feedback_id` (200, no new row) — dedupe on flaky-network retry.
    Insert-or-return via the unique key inside the submit transaction path.
  - Capability: `feedback.idempotency`. Contract doc: update §5.5 + §11.
  - Tests: `tests/submit_idempotency.rs` — same key ⇒ same feedback_id, one row;
    different key ⇒ new row; absent header ⇒ current behavior unchanged.

**Stream 3 — attachments** (A2; owner of `attachments.rs`)
- **A2 · attachment list + download (P1)** — fully-scoped leaf
  - `AttachmentRepo`: `list_for_feedback(scope, fb) -> Vec<AttachmentMeta>` +
    `get_meta(scope, fb, att_id)` (tenant+project scoped, DEC-FBR-03).
  - Handlers on the existing attachments router:
    - `GET …/feedback/{id}/attachments` → JSON list (attachment_id, kind, url,
      content_type, byte_size, created_at).
    - `GET …/feedback/{id}/attachments/{att_id}` → **tenant-scoped download**:
      resolve meta (404 if not in scope), stream bytes via `ObjectStore::get`,
      set `Content-Type` + `Content-Disposition`. (Preferred over trusting the
      stored public URL, which for private S3 is not directly fetchable.)
  - Auth: match the existing upload route's public `open_for_submission` scope
    (same as upload — see attachments.rs §"Auth model"; identity-binding is the
    same documented hardening follow-up, not widened here).
  - Capability: `feedback.attachments`. Contract doc: flip §8 gap-#1 row context +
    add list/download to the attachment section.
  - Tests: `tests/attachment_list_download.rs` — list returns uploaded set;
    download round-trips bytes + content-type; cross-project/feedback → 404.

### STAGE 2 — Deploy + verify GATE (A6; serialized, LD-owned)

- **A6 · deploy to `feedback.gitcellar.com` + verify capabilities** — fully-scoped
  - Run `bash scripts/ci-local.sh --tests` (CI parity) + `cargo sqlx prepare
    --workspace -- --all-targets` (new test-target queries) and commit `.sqlx/`.
  - Bump crate version (→ 0.3.0) + append a change-log entry to
    `docs/integrations/gitcellar-adoption.md` covering §12/§13 + all new §11 flags.
  - Deploy per `docs/operations/RAILWAY_GITCELLAR.md` (apply migrations 00020/00021).
  - **Verify**: `GET https://feedback.gitcellar.com/api/v1/capabilities` advertises
    `feedback.delete`, `feedback.attachments`, `feedback.reply_state`,
    `feedback.severity`, `feedback.idempotency`, `feedback.export`; smoke each new
    route live. This is the EXIT CRITERION — GitCellar Phases B/C unblock only here.
  - Build/wire `capability-advertisement-parity` check (fold into a capabilities
    test): every implemented route ⇒ advertised flag, and vice-versa.
  - ⚠️ Deploy is the one outward/irreversible step — **requires user go/no-go**
    (normal propagation-consent; not AFK-adoptable). CI-local gate is mandatory
    before push (project CLAUDE.md § CI parity).

───────────────────────────────────────────────────────────────
ORACLE PRE-BUILD PLAN
───────────────────────────────────────────────────────────────

| Oracle | Question | Consumer(s) | Timing | Status |
|---|---|---|---|---|
| feedback-erasure-completeness | After DELETE, is ALL PII gone (body + cascade children + object-store bytes)? | A1 build + every finalize + pre-deploy | Stage 0 / Task Zero of A1 | not yet built |
| capability-advertisement-parity | Does every implemented capability have a matching CAPABILITIES string (and vice-versa)? | A6 gate | Stage 2 (fold into capabilities test) | not yet built |

**Rationale**: A1 is the privacy-critical P0; the "purge bytes not just rows"
invariant silently regresses on any future refactor of the delete path — a
Verification Oracle is the anti-reward-hacking leg (matches the project's
established per-trust-boundary oracle pattern). The parity check makes the deploy
gate (A6) truthful — GitCellar feature-detects on the array, so drift between
"route exists" and "flag advertised" breaks the consumer.

**Deferrals**: none material.

───────────────────────────────────────────────────────────────
TESTABILITY GATE FINDINGS
───────────────────────────────────────────────────────────────

Only A1 flags. Q1 iteration cost 2 · Q2 fidelity risk **4** (a delete that leaves
object-store bytes or a cascade child LOOKS successful to a naive test — the
reward-hacking surface) · Q3 critical-path 3 (P0, gates the rest by priority) ·
Q4 scaffolding leverage **5** (a Verification Oracle asserting zero-residual-PII
halves iteration cost AND closes the fidelity gap) · Q5 drift 2. Composite 16/25
→ **flagged**: pair A1 with the `feedback-erasure-completeness` oracle (S0.2)
BEFORE implementing the handler. All other items ≤10 (routine additive contract
work) — not flagged.

───────────────────────────────────────────────────────────────
INTERFACE CONTRACTS (between streams)
───────────────────────────────────────────────────────────────

- **Shared struct add (Stream 1 ↔ Stream 2)**: `MeFeedbackItem`/thread/export
  gain both `severity` (A4) and `updated_at`+`reply_count` (A3). Stream 1 OWNS
  `me_feedback.rs`; Stream 2 provides the `Severity` type from `feedbackmonk-core`.
  If parallel: Stream 1 adds all fields; Stream 2 lands `Severity` first (core
  crate) so Stream 1 can import it. If HERE-sequential: do A4 core-enum before
  Stream 1's struct edits.
- **Shared seam `capabilities.rs`**: each stream appends its flag; LD merges (or,
  HERE, a single edit adds all six). Final array (frozen at A6):
  `feedback.delete, feedback.attachments, feedback.reply_state, feedback.severity,
  feedback.idempotency, feedback.export` (+ existing five).
- **Shared seam `docs/integrations/gitcellar-adoption.md`**: LD owns section
  numbering (§12 delete, §13 export; updates to §5.5/§6/§8/§11). Streams draft
  their deltas; LD assembles to avoid section-number races.
- **`ObjectStore` (S0.1) → A1 + A2**: `delete` (A1) + `get` (A2) land in Stage 0;
  streams consume, never re-edit the trait.

───────────────────────────────────────────────────────────────
COORDINATION + DEFERRED DECISIONS
───────────────────────────────────────────────────────────────

- **Confirm D-A1/D-A4/D-A5** (gate above) — user go/no-go before Stage 1 code.
- **A3 materialized-column vs derived-in-query** — decide at Stage 1 start;
  recommend derived (no migration, no write-path bumps).
- **A2 auth model** — inherit the upload route's public `open_for_submission`
  scope; do NOT widen identity-binding here (same documented follow-up as upload).
- **A6 deploy** — separate explicit consent; irreversible/outward.

───────────────────────────────────────────────────────────────
RISKS + MITIGATIONS
───────────────────────────────────────────────────────────────

- **Residual-PII after delete** (privacy) → S0.2 oracle + A1 byte-purge-before-row
  ordering + `--full` behavioral test.
- **Capability array drift** → parity check at A6; contract doc is SSOT.
- **CI red from new test-target sqlx queries** → mandatory
  `cargo sqlx prepare --workspace -- --all-targets` + `scripts/ci-local.sh`
  before any push (project rule).
- **S3 new verbs untested against a live endpoint** → same posture as the
  existing PUT (offline SigV4 vector proves signing; thin reqwest wiring
  exercised at A6 smoke). LocalFs (self-host default, GitCellar's path) is fully
  unit-tested.
- **Contract already deployed at v0.2.0** → all changes additive; older clients
  degrade gracefully via capability feature-detection (§11 contract).

───────────────────────────────────────────────────────────────
EXECUTION COMMANDS
───────────────────────────────────────────────────────────────

1. Confirm D-A1/D-A4/D-A5 (or accept adopted defaults).
2. `/0-uldf-proceed` at the Stage 1 boundary → picks HERE (single session,
   stream-by-stream) vs. flat-PODS (3 workers: me_feedback / submit / attachments)
   by context budget. Stage 0 (oracles + ObjectStore trait + migration reservation)
   runs first regardless.
3. Stage 1 streams → Stage 2 A6 deploy/verify gate (explicit deploy consent).
4. `/0-uldf-finalize` per stream/stage (CI-local gate is the push guard).

═══════════════════════════════════════════════════════════════
