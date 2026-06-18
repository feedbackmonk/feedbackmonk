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
| `WorkOrderList.tsx` | Table of work orders (audit-forward), linking to detail. |
| `WorkOrderDetail.tsx` | One work order: state, the append-only event ledger, and owner transitions legal from the current state (C22 authz table). |
| `badges.tsx` | `PriorityBadge` / `ActionTypeBadge` / `WorkOrderStateBadge` / `ConfidenceMeter` — color paired with a text label (WCAG 1.4.1). |
| `SourceRefList.tsx` | Renders recommendation `source_refs` as citations, never content dumps (exfiltration defense, C24 case f). |
| `useAdminProject.ts` | Resolves the admin's sole project id (shared cache key with the roadmap admin page). |
| `__tests__/` | Vitest unit tests (approval-gate behavior, rung dial, owner transitions). |

A11y witness lives at `admin-ui/e2e/autopilot-a11y.spec.ts` (Playwright + axe-core,
0 WCAG 2.1 AA violations on digest / cluster detail / approve dialog / work order).

## 3. Public API & Usage

Routed from `admin-ui/src/App.tsx` (project-less admin URLs, sole-project
resolution mirroring `/admin/roadmap`):

- `/admin/autopilot` → `AutopilotDigest`
- `/admin/autopilot/clusters/:clusterId` → `ClusterDetail`
- `/admin/autopilot/work-orders` → `WorkOrderList`
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
