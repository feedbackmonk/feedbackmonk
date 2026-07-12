---
schema: test-modification-justification/v1
commit: ""
session_id: collab-20260712-212050
authored_at: 2026-07-12T22:21:44Z
authored_by: primary
tests_modified:
  - path: crates/feedbackmonk-api/tests/work_order_state_machine.rs
    change_type: extend
    lines_changed: 175
    description: Add owner-authored lifecycle + create-union XOR (400) HTTP cases; update create_work_order call-sites for the new routing_label param.
  - path: admin-ui/e2e/autopilot-a11y.spec.ts
    change_type: extend
    lines_changed: 18
    description: Add the owner-authored "New story" form to the a11y smoke sweep (0 axe violations).
  - path: crates/feedbackmonk-api/tests/runner_e2e.rs
    change_type: extend
    lines_changed: 2
    description: Mechanical call-site update — create_work_order(..., None) -> (..., None, None) for the added routing_label arg.
  - path: admin-ui/src/pages/autopilot/__tests__/RecommendationCard.test.tsx
    change_type: extend
    lines_changed: 36
    description: Add cases asserting routing_label is passed on the approve call and omitted from the create body.
  - path: admin-ui/src/pages/autopilot/__tests__/WorkOrderDetail.test.tsx
    change_type: extend
    lines_changed: 47
    description: Add C31 provenance/routing/claimed-by render cases (owner-authored vs derived).
code_modified:
  - path: crates/feedbackmonk-api/src/handlers/work_orders.rs
    change_type: new-behavior
    lines_changed: 0
    description: Create-union XOR validation + create_owner_work_order + approve routing_label + claim 409 routing_mismatch + runner_list filter.
  - path: crates/feedbackmonk-repository/src/work_orders.rs
    change_type: new-behavior
    lines_changed: 0
    description: WorkOrderStatePatch.routing_label (+ both UPDATEs) + list_for_runner server-side poll filter.
  - path: crates/feedbackmonk-runner/src/prompt.rs
    change_type: new-behavior
    lines_changed: 0
    description: Owner-authored None-context assembly (trusted-only prompt, empty envelope).
  - path: admin-ui/src/pages/autopilot/RecommendationCard.tsx
    change_type: new-behavior
    lines_changed: 0
    description: Optional routing-label input on approve/tweak; routing_label on the approve call.
  - path: admin-ui/src/pages/autopilot/WorkOrderDetail.tsx
    change_type: new-behavior
    lines_changed: 0
    description: Provenance line + always-shown Routing + Claimed-by row.
rationale_summary: "P6 Stage 1 (C31 §3-7): additive tests for named-runner routing + owner-authored stories; no assertion weakened."
hypothesis_ledger_ref: ""
spec_change_ref: "docs/planning/handoffs/p6-c31-contracts.md (Contract C31 §3-7)"
plan_approval_grant: "plan-approval:docs/planning/plans/20260712T164326-p6-autopilot-control-surface-upgrades.md"
plan_ref: docs/planning/plans/20260712T164326-p6-autopilot-control-surface-upgrades.md
---

# Test Modification Justification

> Aggregated artifact for PODS session `collab-20260712-212050` (P6 Stage 1 — Autopilot
> control-surface upgrades), covering all five *modified existing* test files in the merged
> tree. Newly-*added* test files (`work_order_routing.rs`, `Board.test.tsx`, `NewStory.test.tsx`,
> `board-kanban-a11y.spec.ts`) are net-new coverage and do not arm the gate (§0.5.1 added-only
> carve-out); they are not listed here.

## Authority

- **ARHG-05 plan-approval grants** (recorded 2026-07-12, expire 2026-07-19, from the user-approved
  plan `20260712T164326`) cover two of the five files verbatim:
  `crates/feedbackmonk-api/tests/work_order_state_machine.rs` and
  `admin-ui/e2e/autopilot-a11y.spec.ts`.
- The remaining three (`runner_e2e.rs`, `RecommendationCard.test.tsx`, `WorkOrderDetail.test.tsx`)
  were **explicitly approved by the user at convergence** (Phase 5.4 sign-off, 2026-07-12) after
  review: additive coverage for the same approved C31 work, no assertions weakened or deleted.

## Why both tests and code changed in this commit

### 1. What behavior changed?

Three **new** contract behaviors were introduced under Contract C31 (§3–7), all net-additive to a
frozen Stage-0 type surface (commit `dbdc5d7`):

- **Owner-authored work orders (C31 §3):** a new create-union variant lets an owner author a story
  directly (`title`/`instructions`/`action_type`, no `recommendation_id`), driving the *untouched*
  state machine to `completed`. XOR validation rejects mixed/incomplete bodies with precise 400s.
- **Named-runner routing (C31 §4/§5):** an optional `routing_label` on create and approve;
  server-side poll filtering (`list_for_runner`) and a claim `409 {"error":"routing_mismatch"}`,
  both keyed on the *verified* runner token `sub` (never a client value).
- **Runner None-context (C31 §6):** an owner-authored order assembles a trusted-only prompt (empty
  untrusted envelope).

The state machine / transition table / authz matrix (`feedbackmonk-core`) were **not** modified —
routing is coordination, not a trust boundary.

### 2. Why was the existing test outdated, incorrect, or incomplete?

The existing tests were **not wrong** — they were **incomplete** for the new surface, and two files
had a **mechanical signature dependency**:

- `create_work_order(...)` gained a `routing_label: Option<String>` parameter. Every existing
  call-site therefore had to append one argument (`..., None)` → `..., None, None)`). This is the
  entirety of the change in `runner_e2e.rs` (+1/−1) and the 3 deleted lines in
  `work_order_state_machine.rs` (call-site + a re-import line). No assertion was altered.
- `work_order_state_machine.rs`, `RecommendationCard.test.tsx`, and `WorkOrderDetail.test.tsx`
  asserted only the pre-C31 (derived-only) behavior; they were silent on owner-authored provenance,
  XOR rejection, and routing-label passthrough. Leaving them unchanged would have left the new
  contract surface unverified.

### 3. Why is the new test correct?

Every modification is an **`extend`** — new assertions appended, none removed or loosened. The new
invariants checked:

- **`work_order_state_machine.rs`**: owner-authored order reaches `completed` through the unchanged
  transition path; each XOR failure mode returns the exact 400 message; `routing_label` bounds
  (1..128) rejected with a 400 (not a DB 500).
- **`RecommendationCard.test.tsx`**: `routing_label` is sent on the *approve* call and is **absent**
  from the create body — the tweak surface must not leak routing into creation.
- **`WorkOrderDetail.test.tsx`**: owner-authored (`recommendation_id === null`) renders as
  "Owner-authored" with no provenance link; derived renders "From feedback recommendation"; Routing
  and Claimed-by rows always render.
- **`autopilot-a11y.spec.ts`**: the new "New story" form has 0 axe violations (WCAG 2.1 AA).

That the prior suites stayed green with only the mechanical arg appended, and the new cases assert
behaviors the code newly provides (not tautologies), is the anti-reward-hacking evidence: the tests
were expanded to the contract, not bent to the implementation.

### 5. Adversarial check

- `RecommendationCard.test.tsx` asserts `routing_label` is **omitted from the create body** (not
  merely present on approve) — catches a regression where routing leaks into creation.
- `work_order_state_machine.rs` asserts an owner-authored body carrying `owner_overrides` is a hard
  **400**, not a silent drop — a dishonest-success path the old suite could not see.
- `WorkOrderDetail.test.tsx` distinguishes `recommendation_id === null` (owner-authored) from a
  derived order rather than assuming provenance is always present.

## Cross-References

- **Contract**: `docs/planning/handoffs/p6-c31-contracts.md` §3–7
- **Plan**: `docs/planning/plans/20260712T164326-p6-autopilot-control-surface-upgrades.md`
- **Probandurgy principle**: `FOUNDATIONS/PRINCIPLES_OF_LLM_AGENT_ORCHESTRATION.md` § 2.13
- **Gate spec**: `~/.claude/segments/-finalize/phase0.5-test-mod-gate.md`
