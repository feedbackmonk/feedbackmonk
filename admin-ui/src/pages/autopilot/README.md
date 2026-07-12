# `pages/autopilot/` — Review & approval surface (FR-FBR-21)

> **Synopsis**: The owner's admin-UI for the agentic feedback-resolution loop —
> digest of prioritized clusters, recommendation cards with approve/tweak/reject,
> the autonomy-rung dial, and work-order detail with the event ledger. The
> approval control **is** the security boundary (FR-FBR-25a). P5a, recommend-only.

## 1. Purpose & Responsibilities

Render the P5a "autopilot" review surface: what the analyst clustered and
recommended since the last sweep, and the owner's deliberate decisions over it.
This module is **product UI**, lives in the AGPL repo (DEC-FBR-12), and is
**recommend-only** — it creates and approves work orders, but nothing claims a
dispatched order in P5a (the implementer/runner is P5b).

The load-bearing responsibility is the **approval gate**: approval must be an
explicit, deliberate, non-default action (FR-FBR-25a). There is no inline
one-click approve and no pre-checked box — every approval opens a confirm dialog
the owner actively submits, which drives the C22 `create → approve` flow.

## 2. File Index

| File | One-line purpose |
|---|---|
| `AutopilotDigest.tsx` | Top page: latest-sweep digest ("what changed") + cluster list grouped by priority, each with its `priority_rationale`. |
| `ClusterDetail.tsx` | One cluster: summary, rationale, recommendation cards, and the grouped feedback members (quoted data). |
| `RecommendationCard.tsx` | A recommendation with **Approve / Tweak / Reject**. Approve/Tweak run C22 create→approve; Tweak carries authoritative `owner_overrides` (Q17). The security-boundary component. |
| `AutonomyRungDial.tsx` | The graduated rung dial (1–3 selectable; Rung 0 = no order). Surfaces what each rung *authorizes* — a security control, not UX sugar. |
| `WorkOrderList.tsx` | Table of work orders (audit-forward), linking to detail; header links to **New story** + the board view. |
| `NewStory.tsx` | Owner-authored "New story" create form (C31 §3, P6). Composes the owner-authored create union variant (no `recommendation_id` key), optional routing label; creates a `draft` only — **never approves**. |
| `WorkOrderDetail.tsx` | One work order: state, provenance (owner-authored vs. feedback-derived, C31), routing target + claimed runner, the append-only event ledger, and owner transitions legal from the current state (C22 authz table). |
| `Board.tsx` | Read-only Kanban of work orders grouped into 6 lifecycle columns (C31 §7). No drag-to-transition (D-P6-4); cards link to detail where the transition dialogs live. Explicit "+N more" on any capped column (no silent drop). |
| `BoardCard.tsx` | One board card: title (escaped) + action/state badges, rung, `routing_label` + `claimed_by_runner` tags, owner-authored marker (null provenance). Whole card links to detail. |
| `boardColumns.ts` | Pure state→column model for the board: `columnForState` / `groupByColumn` / `BOARD_COLUMNS`. `Record<WorkOrderState, …>` makes the mapping exhaustive by construction (a new state fails `tsc`). |
| `badges.tsx` | `PriorityBadge` / `ActionTypeBadge` / `WorkOrderStateBadge` / `ConfidenceMeter` — color paired with a text label (WCAG 1.4.1). |
| `SourceRefList.tsx` | Renders recommendation `source_refs` as citations, never content dumps (exfiltration defense, C24 case f). |
| `useAdminProject.ts` | Resolves the admin's sole project id (shared cache key with the roadmap admin page). |
| `__tests__/` | Vitest unit tests (approval-gate behavior, rung dial, owner transitions). |

A11y witnesses live at `admin-ui/e2e/autopilot-a11y.spec.ts` (digest / cluster
detail / approve dialog / work order) and `admin-ui/e2e/board-kanban-a11y.spec.ts`
(the Kanban board across all columns, incl. an injection-payload card) — both
Playwright + axe-core, 0 WCAG 2.1 AA violations.

## 3. Public API & Usage

Routed from `admin-ui/src/App.tsx` (project-less admin URLs, sole-project
resolution mirroring `/admin/roadmap`):

- `/admin/autopilot` → `AutopilotDigest`
- `/admin/autopilot/clusters/:clusterId` → `ClusterDetail`
- `/admin/autopilot/board` → `Board` (read-only Kanban, C31 §7)
- `/admin/autopilot/work-orders` → `WorkOrderList`
- `/admin/autopilot/work-orders/new` → `NewStory` (owner-authored create, C31 §3; registered **before** `:id`)
- `/admin/autopilot/work-orders/:id` → `WorkOrderDetail`

Data access is via `shared/ApiClient.ts` (`fetchClusters`, `fetchClusterDetail`,
`fetchLatestSweep`, `fetchWorkOrders`, `fetchWorkOrderDetail`, `createWorkOrder`,
`approveWorkOrder`, `transitionWorkOrder`, `rejectRecommendation`). Types are in
`shared/types.gen.ts` (the P5a block).

## 4. Constraints & Business Rules

- **Approval is the security boundary** (FR-FBR-25a): never default-checked,
  always a deliberate confirm; runs `POST /work-orders` then
  `POST /work-orders/:id/approve` (the owner-authored `approved` ledger event
  C22 inv. 1 requires before any execution state).
- **All feedback/cluster/recommendation text is untrusted public input** — render
  it as escaped React text nodes only, **never** `dangerouslySetInnerHTML`
  (FR-FBR-25 data-envelope; C24 corpus). `source_refs` are citations, not dumps.
- **`priority_rationale` and `source_refs` are explainability surfaces** — always
  shown; the owner must see *why* a cluster is prioritized and *what* was inspected.
- **Tweak = authoritative `owner_overrides`** (Q17), not appended instructions —
  the owner's edit is the trust signal and wins at dispatch.
- **Autonomy rung** is sent at work-order create (C22); the dial offers 1–3 only.
- **Owner-authored create never approves** (C31 §3, P6) — `NewStory` composes the
  owner-authored union variant and creates a `draft`; approval stays the separate
  deliberate act on the detail page (no approval-adjacent auto-focus/default).
- **Routing label is coordination metadata, not a trust boundary** (C31 §4/§5) —
  it pins an order to a runner identity (token `sub`); the approval signature is
  still the security gate. Optional everywhere it surfaces (empty = any runner).

## 5. Relationships & Dependencies

- **Consumes (read-only)** Contract **C22** (work-order API + state machine, Worker
  A) and **C23** (cluster/recommendation/sweep shapes; reads served by Worker B).
- The **A→C seam** (Interface Contract 1): work-order response shapes are A's
  `WorkOrderView`/`WorkOrderDetailResponse`/`TransitionResponse`, mirrored in
  `types.gen.ts`. Recommendation/cluster/sweep shapes are B's (MSG-003).
- All P5a endpoint paths are centralized in `ApiClient.ts` `P5A_PATHS`.

## 6. Decision Log

- **Project-less admin URLs + sole-project resolution** — mirrors the established
  `/admin/roadmap` convention rather than introducing a project segment (multi-
  project URL routing remains deferred).
- **Reject is a recommendation-status write, not a C22 transition** — the C22
  `reject` (`reported → failed`) is a work-order transition; rejecting a *proposed*
  recommendation sets its status to `rejected` with no work order, via Worker B's
  recommendation surface (path pending B confirmation — MSG-004).
- **List/transition return-type fidelity** — A's list returns full `WorkOrderView`
  per row and approve/transition return a `TransitionResponse` (not the order);
  `types.gen.ts` mirrors that exactly, with a distinct name to avoid colliding
  with the feedback-status `TransitionResponse` (C7).
- **No global CSS edits** — only additive `.ap-*` rules were added; shared
  `.primary`/`.dialog-overlay` classes were left as-is (out of this module's lane).
  `NewStory` reuses the global form/`.ap-page`/`.dialog-actions` classes (no new
  classes — C31 §7 shared-file protocol kept `index.css` a single-owner surface).
- **`new`-before-`:id` route ordering** (C31 §3) — `/work-orders/new` is an exact
  path check registered *before* the `:id` regex in `App.tsx`; otherwise the
  literal `new` segment is captured as a work-order id and 404s on load.
- **Routing set at approve, not create, on the rec card** — the rec card's
  create body stays unchanged; `routing_label` rides the `approve` call (the C31
  §4 tweak surface), so the existing create-body assertions are untouched.
