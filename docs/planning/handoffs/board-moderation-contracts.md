# Public Feedback Board + Moderation Gate — Frozen Contracts (Stage 0)

**Source plan**: `docs/planning/plans/20260619T001105-public-feedback-board-moderation-gate.md`
**Frozen**: 2026-06-19 (Stage 0). These contracts are the interface the Stage 1 fanout workers (A backend / B oracle / C admin-UI / D public-UI) build against. Do NOT renegotiate a contract mid-fanout without a plan revision.

Substrate already frozen in Stage 0:
- Migration `migrations/00016_feedback_moderation.sql` — `feedback.moderation_status`, append-only `feedback_moderation_events` ledger, `projects.public_board_enabled` (DEFAULT FALSE) + `projects.board_requires_moderation` (DEFAULT TRUE).
- Core `crates/feedbackmonk-core/src/moderation.rs` — `ModerationStatus {Pending, Approved, Rejected}`, `legal_moderation_transitions_from`, `is_publicly_visible` (only `Approved`), `event_type_for_target`, `ModerationError`.
- Oracle skeleton `.claude/oracles/public-board-moderation-gate/`.

---

## Contract C28 — Moderation state machine + admin queue

**Owner**: Worker A (backend). **Consumer**: Worker C (admin-UI client).

### Endpoints
```
GET  /api/v1/admin/feedback/moderation-queue?status=pending&limit=&offset=
POST /api/v1/admin/feedback/{feedback_id}/moderate
```
Both behind the `AdminSession` extractor, tenant-scoped via the repository layer (DEC-FBR-03). `moderation-queue` defaults to `status=pending` (the queue); accepts `approved`/`rejected` for review.

### `POST .../moderate` request
```json
{ "to_status": "approved" | "rejected" | "pending", "reason_note": "string (optional)" }
```
### `POST .../moderate` 200 response
```json
{
  "feedback_id": "FB-XXXXXX",
  "from_status": "pending",
  "to_status": "approved",
  "moderated_at": "2026-06-19T...Z",
  "audit_id": "uuid"
}
```
### Errors
- `409` illegal transition — body `{ "error": "IllegalModerationTransition", "from_status": "...", "to_status": "..." }` (mirror C7's transition-error shape).
- `404` feedback not in scope.

### Hard Invariants (C28)
1. **No feedback is publicly visible without a recorded owner-authored `approve` event.** Reaching `moderation_status='approved'` MUST write a `feedback_moderation_events` row with `event_type='approve'`, `actor='admin'` in the **same DB transaction** as the column UPDATE (mirror the `perform_transition` / `append_in_executor` same-txn pattern in `admin_feedback.rs`). This is the trust boundary the oracle defends.
2. Illegal transitions rejected **pre-DB-check** against `legal_moderation_transitions_from`; no-op (`from==to`) is illegal.
3. `feedback_moderation_events` is **append-only** — no UPDATE/DELETE repo methods (like `feedback_status_history` / `work_order_events`).
4. Submit path (`feedback.rs::submit`) is **unchanged**: new rows default to `pending` via the column default; no response-shape or behavior change.

### Repository methods (Worker A, `feedbackmonk-repository`)
- `moderate_in_executor(scope, &mut tx, feedback_id, to_status, reason_note, actor_id) -> audit_id` — same-txn UPDATE + ledger append.
- `list_pending_for_admin(scope, status, limit, offset)` — moderation queue read.
- `has_approve_event(scope, feedback_id) -> bool` — the ledger predicate (mirror `has_approved_event`); the oracle's Probe B target.

---

## Contract C29 — Public board read + privacy shape

**Owner**: Worker A (backend). **Consumers**: Worker D (public-UI client), Worker B (oracle).

### Endpoints (public, unauthenticated — mirror `roadmap_router`)
```
GET /api/v1/projects/{project_id}/board?limit=&offset=
GET /api/v1/projects/{project_id}/board/items/{short_code}
```
Project scope via `ProjectRepo::open_for_submission` (the public pre-auth boundary, DEC-PODS-001). Merged into `build_app` **with `.layer(cors)`** (public surface — matches submit/attachments; see `cors-allowlist-enforcement`).

### Board item wire shape — PRIVACY INVARIANT (load-bearing, sibling to Q24)
```json
{
  "short_code": "FB-XXXXXX",
  "body": "verbatim feedback body",
  "kind": "bug|feature|question|other",
  "status": "submitted|triaged|in-progress|shipped|wontfix|duplicate",
  "vote_count": 0,
  "accepted_at": "2026-06-19T...Z"
}
```
**MUST NEVER include**: `end_user_email` / `end_user_name` / `end_user_sub` / `anon_token_hash` / any submitter identity, `external_metadata`, `crash_event_id`, or internal/admin reply content. Model the isolation test on `tests/me_feedback_isolation.rs`. Document as untouchable in the board module README.

### Hard Invariants (C29)
1. **Approved-only.** The board repo queries (`list_public_board`, `get_public_board_item`) MUST hard-filter `moderation_status = 'approved'` **in SQL** — not in the handler, not optional. A `pending`/`rejected` row is structurally unreachable through a board endpoint. (Probe A asserts `is_publicly_visible` stays `{Approved}`; Probe B asserts the SQL filter is present.)
2. **Board-disabled → 404/empty.** When `projects.public_board_enabled = FALSE`, board endpoints return 404 (board not enabled for this project). No approved rows leak from a project that hasn't opted in.
3. **No PII** per the wire shape above.
4. Tenant/project isolation via the repository layer (DEC-FBR-03); `multi-tenant-isolation-check` stays green.

### Voting (DEFERRABLE — Worker A Task Zero decides)
Board voting reuses the anon/JWT voter-resolution pattern from `roadmap.rs` (`resolve_voter` / `AnonGate::token_hash` / `Set-Cookie`). Storage decision (new `feedback_board_votes` table vs. generalizing `roadmap_votes`) is deferred to Worker A Task Zero. **Voting may slip to a follow-up if it threatens GATE 1** — the core gate is approved-only board read + the moderation queue; `vote_count` may ship as a hard `0` placeholder initially (as `reply_count` did in C8 Stage 1).

---

## GATE 1 (Stage 1 convergence) exit criteria
- `public-board-moderation-gate` oracle GREEN with Probe A + B ACTIVE (C live or pending-with-test).
- `board_moderation_gate.rs` (approved-only) + `board_privacy_isolation.rs` (no PII) integration tests pass.
- Admin moderation queue renders pending rows + approve/reject works; per-project board settings toggle works.
- Public board page renders **only approved** rows; board-disabled project shows nothing.
- a11y (axe-core) green on both new UI surfaces.
- `multi-tenant-isolation-check` + `cors-allowlist-enforcement` still green.
