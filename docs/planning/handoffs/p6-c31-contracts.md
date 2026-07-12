# Contract C31 — P6 Autopilot control-surface wire deltas (FROZEN at Stage 0)

**Frozen**: 2026-07-12, Stage 0 freeze commit (P6 arc). Source plan: [`20260712T164326-p6-autopilot-control-surface-upgrades.md`](../plans/20260712T164326-p6-autopilot-control-surface-upgrades.md). Workers A/B/C build against these shapes VERBATIM; a needed change is a plan revision routed through the LD — never a unilateral worker decision. Decisions D-P6-1..4 are recorded in the plan.

## What Stage 0 already landed (do not re-do)

- Migration `00028_work_orders_p6.sql`: `recommendation_id`/`cluster_id` DROP NOT NULL; `routing_label TEXT NULL` (CHECK 1..128); CHECK `work_orders_provenance_pair` — `(recommendation_id IS NULL) = (cluster_id IS NULL)`.
- Repo (`feedbackmonk-repository/src/work_orders.rs`): `WorkOrder`/`NewWorkOrder`/`WorkOrderRow` carry `Option<Uuid>` provenance + `routing_label`; `create()` validates the provenance pair (mixed pair → `Conflict`; rec resolved in scope only when `Some`); INSERT/SELECTs include `routing_label`.
- Handler (`feedbackmonk-api/src/handlers/work_orders.rs`): `WorkOrderView` nullable provenance + `routing_label`; `approve()` guards the recommendation-status projection behind `Some`; `assemble_claimed_order()` emits `"recommendation": null` for owner-authored orders.
- Runner: `ClaimedOrder.recommendation: Option<RecommendationContext>` (serde: absent/null → `None`); `prompt::assemble` emits NO untrusted envelope when `None` (`wrap_untrusted` remains the single chokepoint — `feedback-as-data-audit` Probe A green); `AssembledPrompt::render()` omits the empty block.
- `types.gen.ts`: `WorkOrder.recommendation_id/cluster_id: string | null`, `WorkOrder.routing_label: string | null`, `CreateWorkOrderRequest = CreateDerivedWorkOrderRequest | CreateOwnerWorkOrderRequest`, `ApproveWorkOrderRequest.routing_label?`. **This file is FROZEN — workers consume, never edit.**
- `.sqlx/` regenerated (`--all-targets`); ci-local green; both Verification Oracles PASS; admin-ui tsc + vitest green.

## Wire contracts the workers implement

### C31 §3 — Create (Worker A serves, Worker C consumes)
`POST /api/v1/projects/:pid/work-orders` accepts EXACTLY ONE of:
- **Derived**: `{ recommendation_id, autonomy_rung, owner_overrides?, routing_label? }`
- **Owner-authored**: `{ title (1..512), instructions (non-empty), action_type, autonomy_rung, routing_label? }`

Validation: `recommendation_id` XOR (`title` + `instructions` + `action_type`); mixed/incomplete → 400. Owner-authored enters `draft`; rung gate 1..3 (inv. 4) and every C22 invariant unchanged. Owner-authored `title`/`instructions` are TRUSTED-layer input (owner-authored, like `owner_overrides`) — do NOT wrap them in the untrusted envelope.

### C31 §4 — Approve (Worker A serves, Worker C consumes)
`ApproveWorkOrderRequest` gains optional `routing_label` — sets/overrides the routing target at approve (the Q17 tweak surface). Persisted via the transition patch (COALESCE set-forward like `owner_overrides`).

### C31 §5 — Runner routing (Worker A only; D-P6-3)
- Poll `GET .../runner/work-orders?state=dispatched`: server filters `routing_label IS NULL OR routing_label = <verified token sub>`.
- Claim: same predicate enforced; mismatch → 409 `{"error":"routing_mismatch"}` (mirror existing conflict envelope shape).
- Runner client code: NO changes. Unlabeled orders stay first-claim-wins.

### C31 §6 — ClaimedOrder (landed in Stage 0; Worker A adds behavior tests + docs)
`recommendation: null` ⇔ owner-authored ⇒ runner prompt = trusted layer only. Version-skew note goes in `RUNNER_PROTOCOL.md` change log (old runner × owner-authored order → deserialization failure → order `failed`, recoverable via `retry`).

### C31 §7 — Kanban grouping (Worker B; UI-only)
Draft(`draft`) · Approved(`approved`) · In flight(`WORK_ORDER_EXECUTION_STATES`) · Reported(`reported`) · Done(`completed`) · Halted(`failed`, `cancelled`). Read-only board; per-card actions reuse `WORK_ORDER_OWNER_TRANSITIONS` + existing dialogs (D-P6-4: no drag-to-transition).

## Shared-file protocol (Stage 1)

- `types.gen.ts` — FROZEN (see above).
- `App.tsx` — B and C each add exactly ONE route branch: B `/admin/autopilot/board`; C `/admin/autopilot/work-orders/new` (registered BEFORE the `:id` match).
- `styles/index.css` — additive class blocks only (B: `.ap-board-*`; C: reuse existing form/dialog classes).

## Exit gates

Worker A: `bash scripts/ci-local.sh --tests` green + `approval-gate-enforcement --full` + `feedback-as-data-audit --full` PASS.
Workers B/C: `npx tsc -b` + vitest green + their a11y specs (axe WCAG 2.1 AA, 0 violations).
