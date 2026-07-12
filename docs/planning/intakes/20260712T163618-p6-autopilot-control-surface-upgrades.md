# Intake Assessment
**Source**: /0-uldf-ldis-intake
**Generated**: 2026-07-12T16:36:18Z
**Task**: feedbackmonk P6 — Autopilot control-surface upgrades: (1) Kanban board view over the work-order state machine; (2) named-runner routing (routing label + claim filtering; external tools are runner-protocol clients, no bespoke integrations); (3) owner-authored stories as first-class (work orders not derived from feedback). Out of scope: DEFER-002 (volume-gated), A6 redeploy (GitCellar ops). Additive to frozen Contracts C22–C27; approval-gate + feedback-as-data invariants must hold.

---

## PERCEPTION

**Type**: Enhancement — Feature Addition ×3 (one UI view, two additive backend+UI features on the P5 Autopilot surface)
**Scope**: MEDIUM (migration, repository, handlers, runner poll/claim, admin-UI, types.gen.ts, tests — but all inside one well-bounded subsystem)
**Risk**: MEDIUM — the work touches the work-order surface that carries the FR-FBR-25a security boundary, but **no new states, no new transitions, no new actors**: the state machine is untouched; risk is regression, contained by the `approval-gate-enforcement` + `feedback-as-data-audit` oracles staying green.

**Professional assessment**: This is a well-grounded additive tranche on a mature, contract-frozen subsystem. The riskiest-looking feature (routing) is actually the safest — claim filtering is coordination metadata, not a trust boundary (a runner-class token still cannot author `approve` regardless of routing, C25). The subtlest feature is owner-authored stories, because `recommendation_id`/`cluster_id` are `NOT NULL` today and `WorkOrderView` exposes them as non-optional — a nullability migration ripples through repo structs, the JSON view, `types.gen.ts`, and the autopilot UI's provenance rendering.

### Grounding facts (verified in code, 2026-07-12)

| Fact | Source | Consequence |
|---|---|---|
| `work_orders.recommendation_id` + `cluster_id` are `NOT NULL` FKs | `migrations/00014_work_orders.sql:52-53` | Feature 3 requires a migration to relax nullability (or equivalent provenance design) — it is NOT ui-only |
| `create_work_order(state, scope, recommendation_id, …)` derives title/instructions FROM the recommendation | `crates/feedbackmonk-api/src/handlers/work_orders.rs:508-542` | Feature 3 needs a second creation path (owner-supplied title/instructions/action_type) |
| Runner identity = token `sub`, already recorded as `claimed_by_runner` at claim | `work_orders.rs:941-972` (claim handler) | Feature 2 routes on `sub` — no new runner registry needed |
| Runner poll = `GET /runner/work-orders?state=dispatched` | FR-FBR-24 / RUNNER_PROTOCOL.md | Feature 2 adds poll-side filtering + claim-side enforcement |
| Work-order list/detail handlers + autopilot admin-UI pages exist | `work_orders.rs:726-786`, `admin-ui/src/pages/autopilot/*` | Feature 1 is presentation-layer over existing API |
| State machine + invariants FROZEN (C22) | SPECIFICATION.md § P5 Q15 | All three features must be additive; state-machine edits would require a plan revision |

## SPECIFICATION ANALYSIS

**Coverage**: 8/11 dimensions specified (Purpose, Scope, Behavior, Integration, Constraints, Success, Data (partial), Agent Testability (inherited oracles))
**Gaps**: 0 critical, 2 high (assumable with documentation), 3 medium

### Explicitly Specified

| Dimension | Content | Clarity |
|---|---|---|
| Purpose | Make the Autopilot loop legible (Kanban) + routable (named runners) + usable for non-feedback work (owner stories) | clear |
| Scope | 3 features; DEFER-002 + A6 explicitly OUT | clear |
| Behavior | Columns from WorkOrderState; routing label + claim filter, untagged = first-claim-wins; owner-created orders through same approve→dispatch pipeline | clear |
| Integration | External tools are runner-protocol clients (pull-based competing consumers); no bespoke adapters | clear |
| Constraints | Additive to C22–C27; approval-gate + feedback-as-data oracles must stay green | clear |
| Data | Routing label field named illustratively (`preferred_runner`); owner-story provenance shape unspecified | partial |
| Appearance | Kanban granularity (which states collapse into which columns), card content | partial |
| Users | Owner/admin (implicit) | clear enough |

## Ambiguities Detected

| Item | Interpretations | Risk if wrong | Resolution |
|---|---|---|---|
| Kanban interactivity | A: read-only columns + existing action buttons on cards; B: drag-to-transition | B would route state transitions through a drag gesture — easy to fat-finger an `approve` (the security boundary) | **Assume A** for this tranche; drag-to-transition deferred (see Deferrals) |
| Routing enforcement point | A: claim-time reject only; B: poll-filter only; C: both | A-only wastes runner cycles; B-only is advisory (another runner could still claim) | **Assume C** — poll filters (`sub` mismatch rows omitted), claim enforces (409) |
| Owner-story provenance | A: nullable `recommendation_id`/`cluster_id` + `origin` discriminator; B: synthetic "owner" cluster per project | B pollutes cluster analytics and the sweep corpus | **Assume A** — nullable FKs are idiomatic here (D1 precedent: first-class nullable column) |

## Documented Assumptions

### Safe (95%+)
| ID | Assumption | Rationale |
|----|------------|-----------|
| S1 | Kanban is read-only columns; state changes via existing per-card actions (approve/tweak/reject/cancel/accept), reusing the existing endpoints | Approval is a security boundary (FR-FBR-25a); drag-driven writes are a UX risk with no counterbalancing value in v1 |
| S2 | Runner routing key = runner token `sub` (the identity already recorded in `claimed_by_runner`) | Zero new registry; consistent with C25 key-class model; "naming" a resource = the sub on its runner key |
| S3 | Routing is coordination, not security — a mis-routed claim is a 409, not a trust-boundary breach | Runner class cannot author `approve` (C25); approval-gate invariant untouched |
| S4 | Owner-authored orders enter the existing machine at `draft` and flow through the same approve→dispatch path; no new states/transitions/actors | C22 frozen; the feature is a second *creation* path, not a second *lifecycle* |

### Reasonable (80%+)
| ID | Assumption | Rationale | Validation point |
|----|------------|-----------|------------------|
| R1 | Routing label column named `routing_label TEXT NULL` on `work_orders`, settable at create/approve/tweak by owner only | Matches "owner_overrides" pattern; plan freeze names it finally | Plan contract freeze |
| R2 | Owner-story creation surface = new `POST /work-orders` admin endpoint variant (owner-supplied `title`, `instructions`, `action_type`, `autonomy_rung`, optional `routing_label`) + admin-UI "New story" form on the autopilot area | `create` handler exists but is recommendation-coupled; a parallel owner-authored path is cleaner than overloading | Plan contract freeze |
| R3 | Kanban columns: Draft · Approved · In flight (dispatched+claimed+building+verifying) · Reported · Done (completed) · Halted (failed/cancelled) | 11 raw states are too many columns; grouping preserves the machine without UI sprawl | User eyeball at first render |

### Risky (needs validation)
| ID | Assumption | Risk if wrong | Validation point |
|----|------------|---------------|------------------|
| X1 | Relaxing `recommendation_id`/`cluster_id` to NULL does not break the analyst/runner code paths that assume provenance exists (prompt assembly reads recommendation body; sweep digests) | Runner prompt-assembly could panic/mis-assemble on a provenance-less order | Plan phase MUST inventory every consumer of `recommendation_id`/`cluster_id` (runner `implementer.rs`, `prompt.rs`, digest queries, admin-UI provenance rendering) before freezing the migration |

## Deferred Decisions

| Decision | Deferred until | Default if unresolved | Why defer |
|---|---|---|---|
| Drag-to-transition on the Kanban | Post-tranche user feedback | Read-only + buttons | Security-adjacent UX; needs real usage signal |
| Runner "display name" separate from `sub` | When multiple runners actually exist in the wild | `sub` is the name | No registry until proven need |
| Owner stories feeding back into clusters/analytics | DEFER-002-adjacent future | Owner stories excluded from sweep analytics | Volume-gated, same logic as DEFER-002 |

## Decision Points Identified

### 🛑 Blocking (resolve at plan freeze, not before implementation)
| ID | Decision | Category |
|----|----------|----------|
| 1 | X1 consumer inventory outcome → final migration shape for nullable provenance | Architecture |
| 2 | Exact wire contract for routing label + owner-story create (new Contract C31 candidate) | Design |

### ℹ️ Auto-decidable
- Column grouping labels, card layout, empty states (follow existing autopilot UI patterns)
- 409 error shape for claim-filter rejection (mirror existing IllegalTransition envelope)
- Test file placement (mirror `work_order_state_machine.rs` conventions)

## CALIBRATION

**Task type**: Enhancement / Feature Addition on production system
**Required level**: Standard→Thorough (production, security-adjacent subsystem)
**Current level**: Standard achieved; thorough achieved on constraints/invariants via frozen contracts

**VERDICT: SUFFICIENT** — no user questions required; the two high gaps (routing semantics, provenance shape) are resolved by documented assumptions S2/R1/R2 + blocking decision points at plan freeze, which is where contract shapes belong in this repo's workflow (C22–C30 precedent).

## ORACLE CANDIDATES

No NEW oracle candidates — this tranche's verification posture is **regression of existing oracles**:
- `approval-gate-enforcement` (A/B/C) must stay green — the state machine is untouched, so any red is a defect in this tranche.
- `feedback-as-data-audit` must stay green — owner-authored instructions are TRUSTED-layer input (owner-authored, like `owner_overrides`), which the C27 envelope model already handles; the plan should note this explicitly so no worker "helpfully" wraps owner text in the untrusted envelope.
- Claim filtering gets integration tests, not an oracle (coordination semantics, not an invariant against reward-hacking).

## COLLABORATION ASSESSMENT

**Scope**: MEDIUM

Friction (qualitative CVA, DEC-121):
- **Subdivisible**: YES — 3 units, each namable in a sentence (Kanban UI / routing / owner stories)
- **Spec stable**: YES — additive to frozen contracts; assumptions above pin the moving parts
- **Coupling**: **PARTIAL** — features 2 and 3 both edit `work_orders.rs`, the migration sequence, `WorkOrderView`, and `types.gen.ts`; feature 1's cards want to render both new fields (routing label, origin). Shared surface is enumerable but central.

Value: wall-clock (modest), cross-checking on the security-adjacent surface (real).

**Verdict: STAGED** — Stage 0 freezes the shared surface (one migration pair, `WorkOrderView` + `types.gen.ts` deltas, wire contracts — Contract C31 candidate), then features implement in parallel (rung 1, flat PODS or 2–3 workers) against the frozen shapes. Straight flat-parallel without the freeze would contend on `work_orders.rs`/types; sequential would waste the clean feature boundaries. This mirrors the Public Board arc's proven Stage 0 → Stage 1 shape.

**Partitioning rung**: 1 (flat PODS after Stage 0 freeze) — no charter recursion needed.

## RECOMMENDED NEXT STEPS

1. `/0-uldf-ldis-plan "feedbackmonk P6 — Autopilot control-surface upgrades"` — resolve blocking decisions 1–2 (X1 consumer inventory → migration shape; C31 wire contracts), freeze Stage 0 shared surface, design the staged/parallel topology.
2. Execute per plan (likely Stage 0 freeze commit → parallel feature workers → converge).
3. `/0-uldf-finalize` with oracle regression (approval-gate-enforcement + feedback-as-data-audit green) as the exit gate.
