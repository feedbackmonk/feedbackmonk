# `pages/board/` — Public feedback board page (Public Feedback Board + Moderation Gate, Stage 1)

## Synopsis

`/public/projects/:projectId/board` — the hosted, no-auth public feedback board end-users see. Renders APPROVED feedback only (a server-side SQL invariant), no admin chrome, mirroring `pages/roadmap/PublicRoadmap.tsx`. Read-only consumer of `GET /api/v1/projects/{id}/board` (Contract C29). Voting is deferred to a follow-up — `vote_count` ships read-only.

## Purpose & Responsibilities

The customer-embeddable public board surface for the Public Feedback Board + Moderation Gate feature (sibling to the public roadmap). Intentionally minimal layout so it can sit under a customer's docs domain or be linked from the widget.

- **Approved-only list** — renders whatever the C29 board endpoint returns; does NOT re-filter. Approved-only is a server invariant (the board repo query hard-filters `moderation_status = 'approved'` in SQL — Worker A / C29 inv. 1).
- **Board-disabled state** — a project with `public_board_enabled = FALSE` 404s server-side (C29 inv. 2); the page renders a clean "not available" message, not an error.
- **Read-only vote counts** — voting deferred (see Decision Log); `vote_count` is rendered as text, no vote button.

## File Index

| File | Role |
|---|---|
| `PublicBoard.tsx` | Page component — query + loading/empty/error/board-disabled/list states; `BoardItemRow` renders one approved item |
| `__tests__/PublicBoard.test.tsx` | Vitest suite — list render, **privacy-leak guard**, empty state, board-disabled (404) state |

E2E a11y coverage lives in `admin-ui/e2e/public-board-a11y.spec.ts` (Playwright + axe-core, idle + board-disabled, 0 violations).

## Public API & Usage

```tsx
// Mounted at /public/projects/:projectId/board in App.tsx router (no admin chrome)
<PublicBoard projectId={projectId} />
```

Data shape consumed verbatim from Contract C29 (`BoardListResponse` / `BoardItem`) — see `shared/types.gen.ts`. Single read path: `fetchPublicBoard(projectId)` in `shared/ApiClient.ts`.

## Constraints & Business Rules

- **PRIVACY INVARIANT — UNTOUCHABLE (C29, sibling to Q24).** The board item payload carries **NO submitter identity** — never `end_user_email` / `end_user_name` / `end_user_sub` / `anon_token_hash`, never `external_metadata` / `crash_event_id`, never internal/admin reply content. The server never sends these fields, so there is nothing to anonymize client-side. **Do NOT widen `BoardItem` with any submitter-identity field, and do NOT render one.** A unit test (`PublicBoard.test.tsx` "never references a submitter-identity field") asserts that even a malformed payload smuggling identity fields produces no leak in the DOM. The backend isolation test is modeled on `tests/me_feedback_isolation.rs`.
- **Approved-only is a SERVER invariant.** This component must not be the place that filters non-approved rows — it trusts the C29 SQL hard-filter. If a non-approved row ever reaches the client, that is a backend gate failure (`public-board-moderation-gate` oracle), not a client display bug to paper over.
- **WCAG 2.1 AA** — verified by the axe-core sweep returning 0 violations. The shared `--status-in-progress` color token was darkened to `#9a6400` (~5.0:1 on `--surface`) during this work because the board renders in-progress status badges the roadmap fixture never exercised; do not lighten it without re-running the a11y sweep.

## Relationships & Dependencies

- **`shared/ApiClient.ts`** — `fetchPublicBoard()` is the single read path. It deliberately does NOT swallow a 404 (board-disabled), so the page can distinguish it from a transport error. `PUBLIC_BOARD_PATHS.vote` is a stub for the deferred voting follow-up — no client fn calls it yet.
- **`shared/types.gen.ts`** — `BoardItem` / `BoardListResponse` (Contract C29 mirror), plus `KIND_LABELS` / `STATUS_LABELS` / `FeedbackKind` / `FeedbackStatus`.
- **Backend**: `crates/feedbackmonk-api/src/handlers/board.rs` (`board_router`, CORS-exposed) is the server side of Contract C29. The `public-board-moderation-gate` Verification Oracle (Probe B) asserts the approved-only SQL filter + no-PII from the Rust side.

## Decision Log

### Current

#### Voting deferred — `vote_count` rendered read-only

**Decision**: Stage 1 renders `vote_count` as a read-only text count with no vote/retract button.

**Rationale**: Worker A's Task Zero deferred board voting to a follow-up (new `feedback_board_votes` table; not generalizing `roadmap_votes`). The server returns a hard `0` this stage, mirroring how `reply_count` shipped in C8 Stage 1. The core GATE 1 deliverable is approved-only board read + the moderation queue — voting is additive.

**Trade-offs**: The board looks "backed by 0 people" until the voting follow-up lands and counts populate. Acceptable for the gate.

**Implementation**: `BoardItemRow` renders a `board-item-votes` span; no mutation hooks. `PUBLIC_BOARD_PATHS.vote` and the reserved `BoardItem.voted_by_me?` widening mark the follow-up's seam. `TODO(board-voting follow-up)` in `PublicBoard.tsx` header.

#### Board-disabled (404) is a normal state, not an error

**Decision**: A 404 from the board endpoint renders a plain "This feedback board isn't available." message (no `role="alert"`, no Retry), and the query does not retry on 404.

**Rationale**: `public_board_enabled = FALSE` is an expected configuration, not a failure (C29 inv. 2 — no approved rows leak from a project that hasn't opted in). Treating it as an error would be misleading and would retry-spam a deterministic 404.

**Implementation**: `PublicBoard.tsx` — dedicated 404 branch before the generic `isError` branch; per-query `retry` callback returns `false` for 404.

#### Privacy by absence — the client never sees identity to leak

**Decision**: The page references zero submitter-identity fields; `BoardItem` does not declare any.

**Rationale**: C29 enforces no-PII at the server (the board query selects exactly `short_code, kind, status, body, accepted_at` + `vote_count`). Mirroring that on the client — declaring no identity fields and asserting their absence in a test — makes the invariant defensible on both sides and resistant to a future well-meaning "show who submitted this" edit.

**Implementation**: `BoardItem` in `types.gen.ts`; the "never references a submitter-identity field" test in `PublicBoard.test.tsx`.
