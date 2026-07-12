# Execution Plan
**Source**: /0-uldf-ldis-plan
**Generated**: 2026-07-12T16:43:26Z
**Task**: feedbackmonk P6 — Autopilot control-surface upgrades (Kanban board · named-runner routing · owner-authored stories)
**Strategy**: STAGED (Stage 0 contract freeze → Stage 1 parallel, 3 workers)
**Intake Source**: docs/planning/intakes/20260712T163618-p6-autopilot-control-surface-upgrades.md

---

## Strategy Rationale (CVA — reused from intake, deepened by investigation)

- **Scope**: MEDIUM. **Friction**: subdivisible YES (3 clean units), spec stable YES (additive to frozen C22–C27), coupling PARTIAL — features 2+3 share `work_orders.rs` (repo + handler), the migration sequence, `WorkOrderView`, and `types.gen.ts`; feature 1 renders both new fields.
- **Resolution**: the shared surface is small, enumerable, and **entirely freezable up front** → **STAGED**: Stage 0 lands the migration + type ripple + wire contracts as one atomic commit; Stage 1 fans out 3 ownership-clean workers against frozen shapes. (Same proven shape as the Public Board arc: Stage 0 freeze `e8ef874` → 4-worker fanout.)
- **Value**: cross-checking on a security-adjacent surface (real), wall-clock (modest). PARALLEL within Stage 1; no M12 coupling-debt record needed (coupling resolved by freeze, not routed to SEQUENTIAL).

**CTD**: `ctd.modelTiering` not enabled → **Model tiering: OFF (uniform default model)**. Decomposition still applies; no CTD Plan table, no economics line.
**RSPD**: every component is a **fully-scoped leaf** (prescriptive task lists below); no charter-mode delegation.
**SPODS**: not eligible — creative/judgment implementation work, not rule-enumerable items with schema-verifiable outputs.

## Investigation Inputs (crystallized — two subagent reports, 2026-07-12)

### X1 consumer inventory (decides the migration shape — blocking decision 1: RESOLVED)

`work_orders.recommendation_id`/`cluster_id` NOT NULL consumers:

| Site | Break class |
|---|---|
| `migrations/00014_work_orders.sql:52-53` | The DDL to relax; FKs + btree indexes tolerate NULL |
| Repo `work_orders.rs:36-37, 57-58, 178-179` — `WorkOrder`, `NewWorkOrder`, `WorkOrderRow` | Type change `Uuid → Option<Uuid>` (sqlx compile-checked) |
| Repo `create()` `:229-246` — resolves recommendation, asserts `rec.cluster_id == wo.cluster_id` | Logic guard: make conditional on `Some(rec_id)` |
| Handler `WorkOrderView :553-554` + `From` impl | Type change (serializes `null`) |
| Handler `approve()` `:826-836` — best-effort `set_status` on the recommendation | Logic guard: `if let Some(rec_id)` |
| Handler `assemble_claimed_order()` `:1083-1088` — 3 unconditional FK dereferences; `ClaimedOrder.recommendation: RecommendationContext` **required** | **The sharpest break** — see D-P6-1 |
| Runner crate (`prompt.rs`, `implementer.rs`, `poll/claim/report`, `types.rs`) | **NO break — provenance-blind** (consumes copied title/instructions + pre-assembled context; never the FKs) |
| Tests `work_order_state_machine.rs` + repo unit `seed()` | No break; zero NULL-path coverage → add fixtures |
| `types.gen.ts:713-714` | Type-only: `string → string \| null` |
| `approval-gate-enforcement` oracle | **Unaffected** — probes never parse/query either FK |
| SQL elsewhere (`sweeps.rs`, `recommendations.rs`, migrations) | Zero joins on these FKs |

Total ripple: **5 type changes + 3 logic guards.**

### Admin-UI surface survey (feature placement)

- **Routes**: plain `pathname` matching in `App.tsx`; deeper matches before index. Kanban → `/admin/autopilot/board`; New story → `/admin/autopilot/work-orders/new` registered **before** the `:id` match.
- **Data**: TanStack Query; key prefixes `autopilot-work-orders` etc.; pages gate on `useAdminProject()` then render `...Inner({projectId})`.
- **API client**: all autopilot fns in `ApiClient.ts` (`P5A_PATHS`); `CreateWorkOrderRequest.recommendation_id` currently **required** — wire change per C31.
- **Types**: `WorkOrderState` lowercase single words; `WORK_ORDER_EXECUTION_STATES` + `WORK_ORDER_TERMINAL_STATES` + `WORK_ORDER_OWNER_TRANSITIONS` constants exist and drive the Kanban grouping for free.
- **Conventions**: plain global CSS (`.ap-*` additive classes in `styles/index.css`); modal form pattern = `RecommendationCard.tsx` `ApproveDialog` (a11y-complete, mutation + toast + invalidate); approval controls never auto-focused/default-checked (security boundary).
- **Tests**: vitest colocated `__tests__/`; e2e a11y mirrors `autopilot-a11y.spec.ts` (fake-API fixtures incl. a prompt-injection payload to prove escaping; axe WCAG 2.1 AA, zero violations).

## Decisions (resolved this plan round)

**D-P6-1 — `ClaimedOrder.recommendation` becomes `Option<RecommendationContext>`** (not a synthesized empty context). An owner-authored order has NO feedback grounding: its prompt is 100% trusted owner instructions with **no untrusted envelope at all** — which is the honest security shape (an empty synthesized context would fake provenance and dilute the C24/C27 envelope discipline). This is an **additive plan-round revision to frozen C26** (the field only ever absents on the new order class, which old data cannot contain). Version-skew caveat: an outdated in-the-wild runner claiming an owner-authored order fails deserialization → order reports `failed` with a parse `failure_reason` — recoverable via `retry`, documented in RUNNER_PROTOCOL.md change log. In-repo runner + BYO example updated in the same commit (one coherent system, DEC-FBR-12).

**D-P6-2 — Provenance discriminator is derivational, not a column**: `recommendation_id IS NULL ⇔ owner-authored`. Migration adds CHECK `(recommendation_id IS NULL) = (cluster_id IS NULL)` so half-null provenance is unrepresentable. No `origin` enum (YAGNI; derivable).

**D-P6-3 — Routing is server-side on the verified token `sub`** (zero client protocol change): poll returns `routing_label IS NULL OR routing_label = <caller sub>`; claim enforces the same predicate → 409. `routing_label` is a first-class column (poll SQL filters on it), NOT tucked into `owner_overrides`. Settable at create + approve/tweak; owner-only. Coordination semantics, not a trust boundary (runner class still cannot author `approve`, C25).

**D-P6-4 — Kanban is read-only columns + existing per-card owner actions** (no drag-to-transition; intake S1 — fat-finger risk on the approval boundary). Grouping derives from the existing state constants.

## Contract C31 — Autopilot control-surface wire deltas (FROZEN at Stage 0)

1. **Migration `00028_work_orders_p6.sql`**:
   - `ALTER TABLE work_orders ALTER COLUMN recommendation_id DROP NOT NULL, ALTER COLUMN cluster_id DROP NOT NULL;`
   - `ADD COLUMN routing_label TEXT NULL CHECK (routing_label IS NULL OR length(routing_label) BETWEEN 1 AND 128);`
   - `ADD CONSTRAINT work_orders_provenance_pair CHECK ((recommendation_id IS NULL) = (cluster_id IS NULL));`
2. **`WorkOrder` / `WorkOrderView` / TS `WorkOrder`**: `recommendation_id: Uuid?/string|null`, `cluster_id: Uuid?/string|null`, NEW `routing_label: string|null`.
3. **Create** `POST /projects/:pid/work-orders` — body is EXACTLY ONE of:
   - derived: `{ recommendation_id, autonomy_rung, owner_overrides?, routing_label? }` (existing, + routing_label)
   - owner-authored: `{ title (1..512), instructions (non-empty), action_type, autonomy_rung, routing_label? }`
   - Validation: `recommendation_id` XOR (`title`+`instructions`+`action_type`); violation → 400. Owner-authored enters `draft`; rung gate 1..3 and ALL C22 invariants unchanged.
4. **Approve** `ApproveWorkOrderRequest` gains optional `routing_label` (set/override at approve; approve-with-overrides = the Q17 tweak path).
5. **Runner poll/claim** (D-P6-3): poll filter + claim predicate on `routing_label` vs token `sub`; mismatch → 409 `{"error":"routing_mismatch"}` shape mirroring existing conflict envelopes. Runner client code: NO changes required.
6. **`ClaimedOrder.recommendation: Option<RecommendationContext>`** (D-P6-1): absent ⇒ prompt assembly emits NO untrusted envelope; `prompt::wrap_untrusted` remains the sole chokepoint (feedback-as-data-audit Probe A unchanged).
7. **Kanban grouping (UI-only, not wire)**: Draft(`draft`) · Approved(`approved`) · In flight(`WORK_ORDER_EXECUTION_STATES`) · Reported(`reported`) · Done(`completed`) · Halted(`failed`,`cancelled`).

## Component Breakdown (all fully-scoped leaves)

### Stage 0 — Contract freeze (single agent, one atomic commit)
**Scope**: migration 00028; repo struct/type ripple (`WorkOrder`, `NewWorkOrder`, `WorkOrderRow`, mappers, INSERT binds — `Option` binds NULL); the 2 cheap logic guards (repo `create()` conditional rec-resolution; handler `approve()` guarded `set_status`); `WorkOrderView` + `From`; `types.gen.ts` deltas (nullable FKs, `routing_label`, `CreateWorkOrderRequest` union, `ApproveWorkOrderRequest.routing_label`); `ClaimedOrder.recommendation → Option` type change ONLY (runner compiles, behavior for `None` = Worker A); `.sqlx` regen (`cargo sqlx prepare --workspace -- --all-targets`); this contract section copied to `docs/planning/handoffs/p6-c31-contracts.md`.
**Exit gate (GATE 0)**: `bash scripts/ci-local.sh` green; `approval-gate-enforcement` + `feedback-as-data-audit` oracles PASS; existing tests green (NULL path exercised only in Stage 1).

### Stage 1 — Parallel fanout (3 workers, ownership-clean)

**Worker A — Backend: routing + owner-authored path + runner tolerance** (`crates/**`, `migrations/` none further, `docs/operations/RUNNER_PROTOCOL.md`, `examples/byo-runner/`)
1. Owner-authored create: handler validation (C31 §3 XOR), repo path with `recommendation_id/cluster_id = None`.
2. `assemble_claimed_order`: conditional context assembly (`None` ⇒ no recommendation/cluster/member fetches).
3. Runner: `prompt::assemble` handles `recommendation: None` ⇒ render NO `<untrusted-feedback-data>` block; DEC-84 preamble + trusted layer unchanged; update BYO example + RUNNER_PROTOCOL.md (§6/§7 + change log; version-skew note per D-P6-1).
4. Routing: poll SQL filter + claim predicate + 409 (D-P6-3); `routing_label` accepted at create/approve, persisted, surfaced in views and `/runner/work-orders` list.
5. Tests: `work_order_state_machine.rs` — owner-authored fixture through full lifecycle (draft→…→completed) + XOR validation 400s + provenance-pair CHECK; NEW `work_order_routing.rs` — poll filtering (labeled order invisible to other sub), claim 409 mismatch, unlabeled first-claim-wins; runner unit test for `None`-context prompt assembly (StubAgent); repo unit tests for NULL path.
**Contract obligations**: serves C31 §3–6 exactly; the `feedback-as-data-audit` + `approval-gate-enforcement` oracles green is the exit gate.

**Worker B — Admin-UI: Kanban board** (`admin-ui/src/pages/autopilot/Board*.tsx`, `App.tsx` route branch, `styles/index.css` `.ap-board-*` additions, `admin-ui/e2e/board-kanban-a11y.spec.ts` — NOTE: name it distinctly from the public-board spec)
1. `/admin/autopilot/board` page: 6 columns per C31 §7 from the existing state constants; cards show title, `ActionTypeBadge`, `WorkOrderStateBadge`, rung, `routing_label` (when set), `claimed_by_runner` (when claimed); click-through to `WorkOrderDetail`; per-card owner actions reuse `WORK_ORDER_OWNER_TRANSITIONS` + the existing transition dialog pattern (read-only board otherwise, D-P6-4).
2. Data: `fetchWorkOrders` (existing; paginate/aggregate states client-side; limit param sized accordingly), query key `autopilot-board`.
3. Nav link from `AutopilotDigest` + `WorkOrderList`.
4. Tests: vitest column-grouping unit (state→column mapping exhaustive over all 10 states); e2e a11y spec mirroring `autopilot-a11y.spec.ts` (fixtures spanning every column incl. an injection-payload title).

**Worker C — Admin-UI: New story + routing controls** (`admin-ui/src/pages/autopilot/NewStory*.tsx`, `RecommendationCard.tsx` dialog delta, `WorkOrderDetail.tsx` display delta, `ApiClient.ts` additions, `App.tsx` route branch)
1. `/admin/autopilot/work-orders/new` (registered before `:id`): form per `ApproveDialog` pattern — title (maxLength 512), instructions textarea, `ActionType` select, `AutonomyRungDial`, optional routing label input; submit → `createWorkOrder` with owner-authored body → navigate to the draft's detail; approval remains a separate deliberate act on the detail page (never auto-approve from the form).
2. `ApiClient.ts`: `createWorkOrder` accepts the C31 §3 union; `approveWorkOrder` body + types updated per frozen `types.gen.ts`.
3. Approve/Tweak dialogs (`RecommendationCard`, `WorkOrderDetail`): optional routing-label input; `WorkOrderDetail` renders `routing_label` + owner-authored provenance line ("Owner-authored" when `recommendation_id === null`, replacing any provenance link).
4. Entry points: "New story" button on `WorkOrderList` + `AutopilotDigest`.
5. Tests: vitest for the form (validation, XOR body composition, no-auto-approve) + `WorkOrderDetail` owner-authored rendering; extend `autopilot-a11y.spec.ts` with the new-story route.

**Shared-file protocol**: `types.gen.ts` FROZEN at Stage 0 (workers consume, never edit; a needed change = plan-revision escalation to LD). `App.tsx`: B and C each add exactly one route branch (disjoint lines; LD merges trivially). `index.css`: additive class blocks only, `.ap-board-*` (B) vs form reuse (C, no new classes expected).

## Context Budget

Stage 0 ≈ 15–20% of one session. Workers: A ≈ 40–50%, B ≈ 25–30%, C ≈ 25–30% — all pass (≤85%). LD coordination ≈ 10–15%.

## Testability Gate (Q1–Q8)

Ran clean — **no items flagged** (composites ≤ 8/25). Q1 low everywhere (`ci-local.sh` + vitest/Playwright fake-API inner loops); Q2 low (routing + NULL-path get direct integration tests; the two Verification Oracles are the anti-reward-hacking backstop and are unaffected per X1); Q5 n/a (no new scaffolding — existing StubAgent + fake-API patterns reused, drift already covered by `--full` legs); Q6 no TIER-SHORTFALL (admin-UI verification via the established Playwright fake-API surface); Q7 no GEN-SHORTFALL; Q8 MOD-OK (work touches the existing `work_orders`/`autopilot` module boundaries; no size-band breach expected).

## Oracle Pre-Build Plan

| Oracle | Question | Consumer(s) | Timing | Status |
|---|---|---|---|---|
| — none scheduled — | | | | |

**Rationale**: verification posture is **regression of existing oracles** — `approval-gate-enforcement` (state machine untouched; X1 confirms probes blind to the FKs) and `feedback-as-data-audit` (chokepoint discipline unchanged; `None`-context renders no envelope) must stay green at GATE 0 and convergence.
**Deferrals**: a routing-semantics oracle — declined; claim filtering is coordination (not a reward-hackable trust boundary), integration tests suffice.

## Risks & Mitigations

| Risk | Mitigation |
|---|---|
| C26 `ClaimedOrder` version skew (old runner × owner-authored order) | D-P6-1: fails safe (`failed` + parse reason, `retry` recovers); RUNNER_PROTOCOL change-log note; in-repo runner + BYO example ship in the same release |
| Workers "helpfully" wrapping owner text in the untrusted envelope | Explicit in Worker A brief: owner instructions are TRUSTED layer (like `owner_overrides`); `feedback-as-data-audit` Probe A green required |
| `types.gen.ts` drift between B and C | Frozen at Stage 0; edits are plan-revision-only |
| Kanban pagination hiding orders (fetchWorkOrders limit) | Worker B sizes the fetch (state-filtered queries or raised limit) + shows per-column truncation count — no silent caps |
| `new` captured by the `:id` route | C registers the `new` branch first (survey-confirmed ordering behavior) |

## Execution Commands

1. **Stage 0**: single focused agent (topology per `/0-uldf-proceed` — HERE-adjacent or one orchestrated worker), commit `feat(p6): C31 contract freeze — nullable provenance + routing_label`, verify GATE 0.
2. **Stage 1**: `/0-uldf-pods-parallelize` with Workers A/B/C above (uniform default model).
3. Converge: `/0-uldf-pods-converge --finalize`; exit gate = ci-local green + both oracles PASS + admin-ui vitest/e2e green.

**Plan-approval note (ARHG-05)**: this plan names existing test files as modify-deliverables (`crates/feedbackmonk-api/tests/work_order_state_machine.rs`; `admin-ui/e2e/autopilot-a11y.spec.ts`). On explicit user approval of this plan, record the grant via `plan-approval-grant.ps1` for exactly those files.
