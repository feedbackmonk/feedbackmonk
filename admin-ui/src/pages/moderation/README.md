# `pages/moderation/` — Owner moderation queue (Public Feedback Board, Stage 1)

## Synopsis

`/admin/moderation` — the owner's approve/reject queue for feedback before it can appear on the public board. Client of **Contract C28** (`GET /admin/feedback/moderation-queue`, `POST /admin/feedback/{id}/moderate`). Approving a row is the deliberate owner action that publishes it (FR-FBR-25a sibling — the moderation trust boundary).

## Purpose & Responsibilities

Renders the admin side of the moderation gate (migration `00016` + `feedbackmonk-core::moderation`):

- **Moderation queue** — pending feedback (filterable to approved/rejected for review), newest-first, each row showing kind, body excerpt, submitter label, and submitted time.
- **Per-row moderation actions** — the legal moderation transitions for the row's current state, each opening a confirmation dialog with an optional reason note, calling `POST .../moderate`.

The queue is an **admin-only** surface, so it may show `submitter_label` (already server-formatted, never raw email-only per Contract C8). The PII invariant (Contract C29) governs the **public** board, not this queue.

## File Index

| File | Role |
|---|---|
| `ModerationQueue.tsx` | Page component — status-filtered, paginated queue table (mirrors `FeedbackList` chrome) |
| `ModerationActions.tsx` | Per-row approve/reject/reset controls — state-machine UI mirroring `components/StatusControls` |
| `__tests__/ModerationQueue.test.tsx` | Vitest suite (5 tests) — queue render + the moderation state-machine invariant |

Client + type mirror: `admin-ui/src/shared/boardModerationApi.ts` (kept off the shared `ApiClient.ts`/`types.gen.ts`). Board-enable settings: `pages/settings/BoardSettings.tsx`. E2E a11y: `admin-ui/e2e/moderation-a11y.spec.ts`.

## Public API & Usage

```tsx
// Mounted in App.tsx router
//   /admin/moderation → <ModerationQueue />
```

```tsx
<ModerationActions
  feedbackId="FB-000123"
  currentStatus="pending"           // ModerationStatus
  invalidateKey={["admin-moderation-queue"]}  // refetched on success
/>
```

## Constraints & Business Rules

- **State-machine rendering invariant**: `LEGAL_MODERATION_TRANSITIONS[currentStatus]` (mirror of `legal_moderation_transitions_from`) is the ONLY source of offered actions. An illegal moderation transition is never reachable from the UI; the backend 409 (`IllegalModerationTransition`, C28) is belt-and-braces. No self-transition (`from==to`) is offered.
- **Approve = publish**: reaching `approved` writes a `feedback_moderation_events` `approve` row server-side (C28 inv. 1) — the ledger the `public-board-moderation-gate` oracle trusts. The UI copy makes the "this publishes it" consequence explicit before confirm.
- **No stored-XSS**: submitter body text is rendered as plain text, never via `dangerouslySetInnerHTML` (same invariant as `FeedbackDrawer`).
- **Tenant scope**: all reads/writes go through the C28 AdminSession endpoints, tenant-scoped at the repository layer (DEC-FBR-03).

## Relationships & Dependencies

- **Backend (Contract C28)** — `handlers/moderation.rs` + repository `moderate_in_executor` / `list_pending_for_admin`. Paths/shapes are isolated in `boardModerationApi.ts`'s `MODERATION_PATHS` for one-line reconcile if the backend surface moves.
- **Core state machine** — `feedbackmonk-core::moderation` (FROZEN Stage 0); the TS `LEGAL_MODERATION_TRANSITIONS` mirrors it.
- **Shared UI** — reuses `components/Toast`, the `.dialog`/`.pill`/`.feedback-table` styles, and `shared/router` + `shared/format`.

## Decision Log

- **Separate client file (`shared/boardModerationApi.ts`) instead of editing `ApiClient.ts`/`types.gen.ts`** — the *public* board client (C29) lives in `ApiClient.ts`/`types.gen.ts`; keeping the *admin* moderation surface in its own file keeps the two clients cleanly separated while reusing the shared axios `api` instance read-only.
- **Queue-read shape = `{feedback_id, kind, moderation_status, body_excerpt, submitted_at, submitter_label}`** — Contract C28 froze the `moderate` POST; the queue row is confirmed to these fields (no `reply_count`, no triage `status`). Isolated in `MODERATION_PATHS`/the interfaces so it reconciles in one place if `list_pending_for_admin` returns a different shape.
- **Mirror `StatusControls` rather than invent a new control** — the moderation gate is the same class of state-machine confirmation surface; reusing the proven, axe-clean pattern keeps a11y guarantees and UX consistent.
