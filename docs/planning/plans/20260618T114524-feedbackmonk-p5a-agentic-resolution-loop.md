# Execution Plan
**Source**: /0-uldf-ldis-plan
**Generated**: 2026-06-18T11:45:24
**Task**: feedbackmonk P5a — Agentic Feedback Resolution Loop (recommend-only): FR-FBR-19/20/21/22; freeze Contracts C22–C24
**Strategy**: STAGED (foundation → parallel implementation)
**Intake Source**: `docs/planning/ideations/20260618T103248-agentic-feedback-resolution-loop.md` (ideation note — no formal `/0-uldf-ldis-intake` ran for P5; spec session crystallized § P5 directly)
**Spec Source**: `docs/specs/SPECIFICATION.md` § P5 + § P5 Design detail; `docs/specs/DECISIONS.md` DEC-FBR-12; `docs/specs/OPEN_QUESTIONS.md` Q11–Q20

---

═══════════════════════════════════════════════════════════════
       LDIS EXECUTION PLAN
═══════════════════════════════════════════════════════════════

## Scope & boundary (READ FIRST)

**P5a is the AGPL-repo substrate for the agentic loop, recommend-only — no code execution.**

What P5a builds, entirely inside this repo:

1. **C23 data model** — 5 new tables + 1 column + 2 enums + tenant-scoped repository layer.
2. **FR-FBR-19 clustering** — continuous, cheap, **deterministic server-side** near-duplicate assignment on submit, plus owner merge/split. (LLM-grade re-clustering is a *sweep ingestion* concern, customer-side, P5b.)
3. **FR-FBR-20 sweep** — the sweep **record + orchestration + digest** + recommendation lifecycle + the **analyst-ingestion API** (the seam a customer-side analyst writes recommendations into). The *deep code-grounded recommendation generation* runs customer-side (the runner) and is **P5b** — P5a provides the seam, the scheduled trigger, the storage, and the digest.
4. **C22 work-order API + state machine** — `work_orders` + append-only `work_order_events`, the full state machine mirroring FR-FBR-08, owner-facing transitions (approve/tweak/reject/cancel/request-changes/accept) and the runner-facing transition *contract* (claim/building/verifying/reported/failed). In P5a **nothing claims a dispatched work order** (the implementer is P5b) — the runner-facing half is specced, authz-enforced, and oracle-guarded but exercised only by tests/a no-op probe.
5. **FR-FBR-21 review & approval surface** — admin-UI digest + cluster list + recommendation cards + approve/tweak/reject + autonomy-rung dial. **The approval action IS the security boundary** (FR-FBR-25a).
6. **`approval-gate-enforcement` oracle** — built *before/with* the work-order API; detection-from-event-ledger (anti-reward-hacking leg).
7. **C24 adversarial corpus** — `tests/feedback_injection_corpus.rs` cases a–h. The **data-envelope + poisoned-cluster** defenses apply to P5a clustering/digest *now*; the implementer-side defenses (destructive-steering, full exfiltration) are specced but land with FR-FBR-23 (P5b).

**Explicitly OUT of P5a** (→ P5b): FR-FBR-23 autonomous implementer (the ULDF loop), FR-FBR-24 runner host + runner-token *issuance*. The `agpl-boundary-check` + `feedback-as-data-audit` oracles stay PROPOSED/conditional (DEC-FBR-12 packaging deferred; build as one coherent system).

---

## Strategy Rationale

**Selected Strategy: STAGED** — a single foundation stage (Stage 0 / Task Zero) that freezes the three contracts and lays the data model + enums + repository scaffolding + oracle, followed by a **parallel** implementation stage (Stage 1) with three workers over near-independent surfaces.

Why STAGED and not pure PARALLEL: there is a hard, irreducible dependency. C22 (work-order state machine + tables), C23 (data model + enums), and the repository layer are the substrate that the work-order API, the clustering/sweep logic, the admin-UI, *and* the oracle all consume. Fanning out before that substrate is frozen guarantees three workers inventing incompatible enum spellings, column names, and transition semantics. Freeze first, then fan out.

Why not pure SEQUENTIAL: post-foundation, the three surfaces are genuinely separable with clean ownership boundaries (backend work-order API ∥ backend clustering/sweep ∥ React admin-UI), each benefits from focused attention, and the security-critical work-order path benefits from a dedicated worker who isn't context-switching into React. Scope is MEDIUM-LARGE.

### Collaboration Value Assessment (per `docs/PARALLEL_COLLABORATION_DESIGN.md`)

| Factor | Score (1–5) | Note |
|---|---|---|
| Specialization | 5 | Rust work-order/state-machine, Rust clustering/sweep, React admin-UI, and Python oracle are distinct skill surfaces |
| Quality | 4 | Security boundary (approval gate) benefits from a dedicated, non-distracted owner + independent oracle author |
| Discovery | 3 | State machine + clustering are novel here; admin-UI ports known patterns |
| Speed | 4 | Three backend+frontend surfaces parallelize cleanly after foundation |
| **Value total** | **16/20** | |
| Boundary Clarity (higher=less friction) | 4 | Crisp file ownership once contracts frozen; one shared seam (the repository layer + types.gen.ts) |
| Coupling (higher=less friction) | 3 | Admin-UI depends on work-order API shape; mitigated by frozen C22 + generated types |
| **Friction total** | **7/10** | |
| **Net score** | **16 − (7/2) = 12.5** | **PARALLEL strongly recommended for Stage 1** |

Net 12.5 (≥8) → parallel for the implementation stage. The foundation stage is single-threaded by necessity (it *is* the shared contract).

---

## Context Budget Assessment

| Agent | Assigned scope | Sibling summaries | Contracts | Reasoning reserve | Est. budget | Verdict |
|---|---|---|---|---|---|---|
| **Stage 0 / Foundation (LD or solo)** | Migrations 00013–00014, 2 enums in `-core`, 6 repo modules in `-repository`, oracle scaffold | repository pattern (~500), FR-FBR-08 status pattern (~500) | C22/C23/C24 (authored here) | ~25% | ~55% | ✅ Pass |
| **Worker A — Work-order API + state machine** | `-core/src/work_order.rs` (state machine), `-api/src/handlers/work_orders.rs`, router wiring, work-order repo methods, oracle finalization | C22 (full), FR-FBR-08 status summary (~500), repo scope summary (~500) | C22 | ~25% | ~60% | ✅ Pass |
| **Worker B — Clustering + sweep + ingestion** | `-api/src/handlers/clusters.rs` + `sweeps.rs` + `recommendations.rs`, clustering assignment on submit, repo methods, C24 corpus (clustering-relevant cases) | C23 (full), submit-handler summary (~500), repo scope summary (~500) | C23, C24 | ~25% | ~60% | ✅ Pass |
| **Worker C — Review & approval admin-UI** | `admin-ui/src/pages/autopilot/*` (digest, cluster list, recommendation cards, rung dial), API client additions, `types.gen.ts` regen | C22 + C23 (consumed read-only), FeedbackList page pattern (~500) | C22, C23 | ~25% | ~58% | ✅ Pass |

All four agents land ≤ 85% effective capacity. No decomposition required. The security-critical surface (Worker A + the oracle) is deliberately isolated so the approval-gate logic gets undistracted attention.

---

## Oracle Pre-Build Plan

| Oracle | Question | Consumer(s) | Timing | Status |
|---|---|---|---|---|
| `approval-gate-enforcement` | Can any work order reach a state ≥ `dispatched` without a prior owner-authored `approved` event in the ledger? (FR-FBR-25a / FR-FBR-22) | Worker A (primary), all reviewers | **Task Zero** (scaffold in Stage 0; finalize with Worker A as the state machine lands) | not yet built |

**Rationale**: This is the single highest-leverage scaffolding artifact in P5a. The approval gate is a *security boundary between public input and code execution* — the dominant fidelity risk (Testability Gate Q2 below). A unit test that asserts "approving sets state to approved" does **not** catch a bypass code path that writes `dispatched` directly. The oracle reads the **`work_order_events` ledger + the state-machine source** and proves *from state/code* that no path reaches `dispatched` without the owner-authored `approved` event — detection-from-state, not a self-reported flag (anti-reward-hacking leg). Build it as Worker A writes the state machine so the gate and the code co-evolve; a green oracle is Stage 1's exit criterion for the work-order surface.

**Deferrals** (evaluated, not scheduled for P5a):
- `agpl-boundary-check`: **conditional** — activates only if the deferred open-core split (DEC-FBR-12) is later chosen. Moot while the loop is built as one coherent system. Do not build in P5a.
- `feedback-as-data-audit`: prompt-assembly happens in the **runner** (customer-side, P5b), not this repo. The static "feedback-as-data-envelope" check has no in-repo prompt-assembly path to guard in P5a. Defer to P5b alongside the implementer. (The corpus C24 covers the P5a-reachable surface behaviorally in the meantime.)
- `multi-tenant-isolation-check` (already LIVE): no new oracle needed — it auto-covers the 5 new tables because they route through the tenant-scoped repository. Worker B/A simply must not introduce raw SQL outside `-repository` (the existing oracle will catch it).

---

## Frozen Contracts

> These three contracts are **FROZEN** by this plan (the handoff's core deliverable). Stage 1 workers implement against them verbatim; any change requires a plan revision, not a worker's unilateral decision.

### Contract C22 — Work-order API + approval state machine

**Tables**: `work_orders`, `work_order_events` (shapes in C23). Migration `00014_work_orders.sql`.

**Enum `WorkOrderState`** (`feedbackmonk-core/src/work_order.rs`, `#[serde(rename_all = "kebab-case")]`, `.as_db_str()`/`.from_db_str()` per the `FeedbackKind` pattern; DB column `state TEXT NOT NULL CHECK (state IN (...))`):
`draft | approved | dispatched | claimed | building | verifying | reported | completed | failed | cancelled`

**Legal-transition table** (`legal_transitions_from(WorkOrderState) -> &'static [WorkOrderState]`, mirrors `feedbackmonk-core/src/status.rs::legal_transitions_from`):

```
draft      → [approved, cancelled]
approved   → [dispatched, cancelled]
dispatched → [claimed, cancelled]
claimed    → [building, cancelled]
building   → [verifying, failed, cancelled]
verifying  → [reported, failed, cancelled]
reported   → [completed, building, failed]      // building = request-changes loop
failed     → [approved, cancelled]               // retry → approved
completed  → []                                  // terminal
cancelled  → []                                  // terminal
```

**Transition authorship authz** (who may author each `event_type`):

| event_type | from → to | Authorized actor |
|---|---|---|
| `approve` | draft → approved | **authenticated owner/admin only** (`AdminSession`) |
| `cancel` | any non-terminal → cancelled | authenticated owner/admin only |
| `dispatch` | approved → dispatched | system (server-side, only after an `approved` event exists) |
| `claim` | dispatched → claimed | **runner** (project-scoped write token, Q14) |
| `building`/`verifying`/`reported`/`failed` | per table | **runner** only, only from legal prior state |
| `accept` | reported → completed | authenticated owner/admin (or auto per autonomy rung ≥ 2) |
| `request-changes` | reported → building | authenticated owner/admin only |
| `reject` | reported → failed | authenticated owner/admin only |
| `retry` | failed → approved | authenticated owner/admin only |

**Five hard invariants** (specced like FR-FBR-08's five; each gets a unit test + the oracle covers #1):
1. **No state ≥ `dispatched` without a recorded owner-authored `approved` event.** Enforced pre-DB-check *and* audited; `approval-gate-enforcement` oracle verifies no code path bypasses it. *(THE security boundary, FR-FBR-25a.)*
2. Actor-role enforcement per the authz table — owner-only events vs runner-only events, each only from the legal prior state.
3. **Same-transaction event-row parity**: every state change opens a txn, writes the `work_orders.state` update *and* the `work_order_events` row in the same `sqlx` transaction (use `append_in_executor` pattern from `feedback_status_history.rs`). Audit can never drift from state.
4. `draft` entered only at autonomy Rung ≥ 1; Rung 0 never produces a work order. `reported → completed` is auto only at Rung ≥ 2 and only for the action classes that rung authorises.
5. Terminal states (`completed`, `cancelled`) are immutable.

**HTTP surface** (router `/api/v1/projects/:project_id/...`, admin endpoints behind `AdminSession`, runner endpoints behind the runner write-token verifier):
- `POST   /work-orders` — create draft from a recommendation (admin). Body: `{recommendation_id, autonomy_rung, owner_overrides?}`.
- `GET    /work-orders` — list (admin). Filters: `state`, `cluster_id`.
- `GET    /work-orders/:id` — detail incl. event ledger (admin).
- `POST   /work-orders/:id/approve` — owner approval gate (admin). Body: `{owner_overrides?}`.
- `POST   /work-orders/:id/transition` — owner transitions: cancel/accept/request-changes/reject/retry (admin). Body: `{event_type, detail?}`.
- `POST   /work-orders/:id/claim` + `/runner-transition` — **runner-only** (P5b will drive these; in P5a they exist, are authz-guarded, and are exercised only by `tests/work_order_state_machine.rs` + a no-op probe). Body validated against the legal-transition table.

**Q14 resolution (FROZEN here, issuance deferred to P5b)**: the runner authenticates with a **project-scoped, write-scoped runner token** minted from the *same per-project Ed25519 signing-key infrastructure* used for end-user JWTs (DEC-FBR-04) — a new credential **class**, not a new crypto stack. Scope is limited to runner-authored work-order transitions for one `project_id`; rotation/revocation mirror signing-key deactivation (`POST`/`DELETE` on a `/projects/:id/runner-tokens` endpoint). **P5a freezes the verification/authz seam** (the runner-transition endpoints verify a token of this class and reject everything else); **issuance of the token + the runner that uses it is FR-FBR-24 (P5b)**. Rationale: the work-order API's invariant #2 references "runner (project-scoped write token, Q14)" — the authz contract must be frozen now so Worker A can enforce it; the token-minting UX rides with the runner.

**Q17 resolution (FROZEN here)**: "tweak before approve" = **authoritative overrides**, captured in `work_orders.owner_overrides jsonb`. At dispatch, `owner_overrides` is merged over the recommendation's fields with **overrides winning** (the recommendation is provenance; the work order is the order). The recommendation row is stamped `tweaked_approved` (vs `approved`). Post-report, `request-changes` re-opens `reported → building` carrying a new `owner_overrides` delta in the event `detail`. Not "additional instructions appended" — authoritative, because feedback-derived recommendations are untrusted and the owner's edit is the trust signal.

### Contract C23 — Clusters / recommendations / work-orders data model

Migrations `00013_feedback_clusters.sql` (clusters + sweeps + recommendations + the `feedback.cluster_id` column) and `00014_work_orders.sql` (work_orders + work_order_events). All tables carry `tenant_id uuid NOT NULL` + `project_id uuid NOT NULL`, reachable **only** through `feedbackmonk-repository` (DEC-FBR-03; raw SQL = security incident). Enums are `TEXT` + `CHECK(...)` per the `FeedbackKind` precedent (no Postgres native enums — keeps migrations cheap, matches existing convention).

| Table / column | Shape | Notes |
|---|---|---|
| `feedback.cluster_id` *(new, nullable FK)* | `cluster_id uuid NULL REFERENCES feedback_clusters(id) ON DELETE SET NULL` | **Decision point D1 RESOLVED: first-class nullable column, NOT a join table.** A feedback belongs to ≤1 cluster; re-clustering is audited via `feedback_clusters.updated_at` + sweep provenance, not historised in a membership table. Idiomatic per the `crash_event_id` precedent (`00010`). |
| `feedback_clusters` | `id, tenant_id, project_id, label, summary, kind (FeedbackKind), priority (high\|medium\|low\|none), priority_rationale, status (open\|actioned\|dismissed\|merged), merged_into_id uuid NULL, member_count int, last_swept_at timestamptz NULL, created_by (agent\|admin), created_at, updated_at` | `priority_rationale` load-bearing for explainability (owner must see *why*). `merged_into_id` supports merge (status=merged points at survivor). |
| `recommendations` | `id, tenant_id, project_id, cluster_id FK, sweep_id FK NULL, action_type (ActionType), title, body, rationale, source_refs jsonb, confidence float, status (proposed\|approved\|tweaked_approved\|rejected\|superseded), generated_at, created_at` | `source_refs` = file/line/doc **references** the analyst inspected (grounding evidence, *never dumps* — exfiltration defense). 1:N from cluster; superseded recs retained for history. |
| `analysis_sweeps` | `id, tenant_id, project_id, triggered_by (schedule\|on_demand), started_at, completed_at NULL, status (running\|completed\|failed), clusters_touched int, recommendations_emitted int, runner_id text NULL, agent_version text NULL, digest_summary text NULL` | Provenance per deep sweep (FR-FBR-20). `digest_summary` powers "what changed since last time." |
| `work_orders` | `id, tenant_id, project_id, recommendation_id FK, cluster_id FK, action_type (ActionType), title, instructions, owner_overrides jsonb NULL, autonomy_rung int CHECK(0..3), state (WorkOrderState), approved_by uuid NULL, approved_at timestamptz NULL, dispatched_at timestamptz NULL, claimed_by_runner text NULL, result_ref jsonb NULL, failure_reason text NULL, created_at, updated_at` | The C22 contract row. `owner_overrides` carries Q17 edits. |
| `work_order_events` | `id, tenant_id, project_id, work_order_id FK, from_state, to_state, event_type, actor (admin\|runner\|system), actor_id text NULL, detail jsonb NULL, at timestamptz` | **Append-only.** No UPDATE/DELETE methods on its repo. The substrate `approval-gate-enforcement` reads. |
| `ActionType` *(new enum, `-core`)* | `bug_fix \| feature_implementation \| enhancement \| investigation \| no_action` | Maps from `FeedbackKind` but distinct (`bug`→`bug_fix`; `question`→`investigation`/`no_action`). `.as_db_str()`/`.from_db_str()`. |
| `WorkOrderState` *(new enum, `-core`)* | see C22 | `.as_db_str()`/`.from_db_str()`; `legal_transitions_from`. |

**Repository modules** (`feedbackmonk-repository/src/`, each `*Repo` trait + `Sqlx*Repo` impl, every method `&ProjectScope` first, re-exported in `lib.rs`, `Arc<dyn …>` field in `AppState`):
`clusters.rs`, `recommendations.rs`, `analysis_sweeps.rs`, `work_orders.rs`, `work_order_events.rs` (append-only). Plus a `cluster_id` setter on the existing `feedback.rs` repo. **`.sqlx/` offline cache regenerated** (`cargo sqlx prepare`) after the migrations land and committed.

### Contract C24 — Feedback-injection adversarial corpus

`crates/feedbackmonk-api/tests/feedback_injection_corpus.rs`, named-case (mirrors the JWT fixture corpus in `feedbackmonk-jwt/tests/verify.rs` and the Q24 byte-for-byte discipline in `promote.rs`). Cases a–h:

| Case | Attack | P5a assertion (this slice) |
|---|---|---|
| (a) | classic "ignore previous instructions" | clustering/digest treats body as **data**; no instruction effect; body rendered as quoted data |
| (b) | fake role/system markers (`</user> SYSTEM:…`) | same — markers are inert text in cluster label/summary derivation |
| (c) | instruction smuggled via attachment captured-log | data-envelope holds for attachment-derived text |
| (d) | unicode/homoglyph/zero-width obfuscation | normalization doesn't smuggle directives; label derivation is data-only |
| (e) | mass-duplicate **poisoned cluster** (manufacture high priority) | priority is **advisory**; manufactured priority **cannot execute** (no work order without owner approval — ties to C22 inv. 1); digest renders cluster text as quoted data |
| (f) | exfiltration probe ("include .env contents") | `source_refs` are references not dumps; **no secret in analyst-reachable context** *(P5a: assert recommendation-ingestion rejects/sanitizes; full runner-side exfil defense = P5b)* |
| (g) | destructive-steering ("delete the auth check") | **P5b** (implementer) — corpus case present, marked `#[ignore]`-with-reason until FR-FBR-23; documents the contract now |
| (h) | approval-gate probe (set state past `approved` w/o owner event) | C22 inv. 1 holds: transition rejected pre-DB-check; ledger has no orphan `dispatched`; ties to `approval-gate-enforcement` oracle |

Each non-ignored case asserts: **content treated as data, no instruction executes, approval gate holds, no secret leaks, PII scrubbed on any outbound draft** (FR-FBR-10). Cases (g) and full (f) carry a documented `#[ignore = "P5b: implementer-side defense (FR-FBR-23)"]` so the corpus is complete-by-contract now and activates in P5b.

---

## Execution Overview (stages)

```
STAGE 0 — Foundation / Task Zero  (single-threaded; LD or solo)
  0.1  Freeze C22/C23/C24 into the spec (flip § P5 Design detail PROPOSED→FROZEN, record in DECISIONS)
  0.2  Migrations 00013 + 00014  (+ feedback.cluster_id column)
  0.3  Enums ActionType + WorkOrderState in feedbackmonk-core (+ legal_transitions_from + tests)
  0.4  Repository scaffolding: 5 new *Repo traits + Sqlx impls + AppState wiring + cargo sqlx prepare
  0.5  approval-gate-enforcement oracle SCAFFOLD (.claude/oracles/approval-gate-enforcement/)
  ── GATE 0: cargo build green, migrations apply clean, existing oracles still PASS, .sqlx committed ──

STAGE 1 — Parallel implementation  (3 workers; PODS or sequential-with-handoffs)
  Worker A (security-critical): work-order state machine + API handlers + router + finalize oracle
  Worker B: clustering-on-submit + merge/split + sweep record/digest + recommendation + ingestion API + C24 corpus
  Worker C: admin-UI review & approval surface (digest, clusters, rec cards, approve/tweak/reject, rung dial)
  ── GATE 1 (exit): approval-gate-enforcement PASS, C24 corpus green, /0-uldf-verify-fast green, admin-UI builds ──

CONVERGENCE → /0-uldf-finalize  (or /0-uldf-pods-converge if PODS)
```

Stage 0 is the contract; Stage 1 fans out. Worker C consumes C22/C23 read-only and regenerates `admin-ui/src/shared/types.gen.ts` from Worker A's API shapes — the one cross-worker seam (see Interface Contracts).

---

## Component Breakdown

| # | Component | Owner | Files (anchor paths) | FR / Contract |
|---|---|---|---|---|
| C1 | Migrations + column | Stage 0 | `migrations/00013_feedback_clusters.sql`, `00014_work_orders.sql` | C23 |
| C2 | Domain enums + state machine | Stage 0 → A | `crates/feedbackmonk-core/src/work_order.rs` (new), `src/action_type.rs` (new) | C22, C23 |
| C3 | Repository layer (6 modules) | Stage 0 → A/B | `crates/feedbackmonk-repository/src/{clusters,recommendations,analysis_sweeps,work_orders,work_order_events}.rs` + `feedback.rs` cluster setter | C23 |
| C4 | Work-order API + handlers | Worker A | `crates/feedbackmonk-api/src/handlers/work_orders.rs` (new), router wiring in `main.rs`/`router.rs` | C22, FR-FBR-22 |
| C5 | Runner write-token verifier (seam only) | Worker A | `crates/feedbackmonk-api/src/handlers/work_orders.rs` (runner extractor) + reuse `feedbackmonk-jwt` | C22 (Q14) |
| C6 | Clustering on submit + merge/split | Worker B | `crates/feedbackmonk-api/src/handlers/clusters.rs` (new) + submit-path hook | FR-FBR-19 |
| C7 | Sweep record + digest + recommendations + ingestion API | Worker B | `crates/feedbackmonk-api/src/handlers/{sweeps,recommendations}.rs` (new) | FR-FBR-20 |
| C8 | Adversarial corpus | Worker B | `crates/feedbackmonk-api/tests/feedback_injection_corpus.rs` (new) | C24, FR-FBR-25b |
| C9 | Review & approval admin-UI | Worker C | `admin-ui/src/pages/autopilot/*` (new), `admin-ui/src/shared/ApiClient.ts`, `types.gen.ts` | FR-FBR-21 |
| C10 | approval-gate-enforcement oracle | Stage 0 scaffold → A finalize | `.claude/oracles/approval-gate-enforcement/{oracle.py,oracle.sh,oracle.ps1,manifest.json}` | FR-FBR-25a |

---

## Testability Gate Findings

Per-item SMURF scores (Q1 iteration-cost, Q2 fidelity-risk, Q3 critical-path, Q4 scaffolding-leverage, Q5 drift-detection; 5 = worst). Flag rules: composite > 12; Q2 = 5 alone; Q5 = 5 with scaffolding; Q1=5 ∧ Q3≥4.

| Item | Q1 | Q2 | Q3 | Q4 | Q5 | Composite | Flag |
|---|---|---|---|---|---|---|---|
| **C4/C10 Work-order approval gate** | 2 | **5** | **5** | 5 | 2 | **19** | 🚩 **FLAGGED** (Q2=5 + composite) |
| C6 Clustering-on-submit | 3 | **4** | 3 | 4 | 4 | 18 | 🚩 **FLAGGED** (composite + drift) |
| C2 State machine (enum + legal transitions) | 1 | 2 | 4 | 2 | 1 | 10 | ok (mirrors proven FR-FBR-08) |
| C7 Sweep/digest/ingestion | 2 | 3 | 2 | 3 | 3 | 13 | 🚩 FLAGGED (composite, borderline) |
| C9 Admin-UI surface | 3 | 2 | 2 | 3 | 2 | 12 | ok |

### Flag 1 — C4/C10 Work-order approval gate (Q2=5, composite 19) — HIGHEST PLAN-WIDE RISK

**Why Q2=5**: this is *the* security boundary between public internet input and code execution. A verifier that misses a bypass is catastrophic (the ImpossibleBench / METR reward-hacking surface in its most literal form — a passing test while a `dispatched` row exists with no `approved` event = remote-code-execution path). A standard "approve → state==approved" unit test has high fidelity risk: it confirms the happy path, not the *absence of bypass paths*.

**Mandatory mitigation (already in the plan)**: the `approval-gate-enforcement` **Verification Oracle** (C10), built in Stage 0 and finalized with Worker A. It is **detection-from-state**: it parses the state-machine source + queries the `work_order_events` ledger and asserts *no* code path and *no* ledger row reaches state ≥ `dispatched` without a prior owner-authored `approved` event. This is the anti-reward-hacking leg — it cannot be satisfied by a self-reported flag. **Q5 drift detection**: the oracle's `--full` mode runs `tests/work_order_state_machine.rs` against the real DB, so the static probe and live behavior can't silently diverge. A green oracle is the **exit gate** for Worker A.

### Flag 2 — C6 Clustering-on-submit (composite 18, Q5=4)

**Why flagged**: clustering correctness is fuzzy ("same thing said many ways"), and an LLM-based clusterer would be non-deterministic (Q2 high) with hard-to-detect drift (Q5 high). 

**Mitigation / design constraint (raised to a plan decision)**: **P5a clustering MUST be deterministic and server-side** — a cheap, testable near-duplicate heuristic (normalized-text similarity / trigram or token-set Jaccard over the feedback body, threshold-based assignment), **not** an LLM call on the submit hot path. This (a) keeps the submit path fast and side-effect-free, (b) makes clustering unit-testable with fixed fixtures (kills Q2/Q5), and (c) is the honest "cheap always-on half" FR-FBR-19 describes. LLM-grade re-clustering is a *sweep* concern that arrives via the **ingestion API** (customer-side analyst, P5b) — it overwrites `cluster_id` + `feedback_clusters` rows transactionally, and the deterministic heuristic is the always-on floor between sweeps. **Drift detection**: a fixture corpus of known near-dup / non-dup pairs with asserted assignments; the heuristic's threshold is pinned by these fixtures.

### Flag 3 — C7 Sweep/digest/ingestion (composite 13, borderline)

**Mitigation**: the sweep *record/orchestration* is deterministic CRUD (low real risk); the borderline score is the **ingestion-API trust boundary** (a customer-side analyst writing recommendations). Constraint: ingestion validates `source_refs` are references not dumps (exfil defense, C24 case f), and ingested text is stored as data (never executed). Covered by C24 corpus + the existing PII scrubber on any outbound draft. No new scaffolding beyond the corpus.

---

## Ripple Analysis (modified interfaces & consumers)

| Modified interface | Consumers traced | Impact | Migration task |
|---|---|---|---|
| `feedback` table (+`cluster_id` column) | `SqlxFeedbackRepo` (`feedback.rs`), submit handlers, `multi-tenant-isolation-check` oracle, `.sqlx/` cache | Nullable column → **additive, non-breaking**. Existing `INSERT`/`SELECT` unaffected (column defaults NULL). `.sqlx/` must regen. | Stage 0.4 |
| `AppState` (+5 `Arc<dyn …Repo>` fields) | `build_state` in `main.rs`, every handler signature taking `State<AppState>` | Additive — new fields, existing handlers untouched. | Stage 0.4 |
| Router (`build_app`/`router.rs`) | `main.rs` composition, integration tests, CORS layering | **Additive merges** — new admin + runner routers `.merge()`d. Admin routers behind `AdminSession` (no CORS); runner routers behind the new write-token verifier. Must NOT accidentally CORS-expose admin/runner endpoints. | Worker A |
| `feedbackmonk-core` public API (+2 enums, +state machine) | `-repository`, `-api` | Additive new modules; no change to `FeedbackKind`/`FeedbackStatus`. | Stage 0.3 |
| `admin-ui/src/shared/types.gen.ts` | every admin-UI page importing API types | Regenerated to add work-order/cluster/recommendation types. Worker C owns regen *after* Worker A freezes response shapes. | Worker C (post-A) |
| `.sqlx/` offline cache | CI `cargo build` (offline), `multi-tenant-isolation-check` | Regenerate + commit after every migration/query change. **Common breakage source.** | Stage 0.4 + each backend worker |

**Blast radius: 🟡 Medium** — almost entirely *additive* (new tables, new enums, new routers, new UI pages). The two real cascade points are (1) `.sqlx/` regeneration discipline and (2) the `types.gen.ts` seam between Worker A and Worker C. No existing requirement IDs renamed/deleted (no G4 trigger). No breaking change to existing public API (no G3 trigger).

---

## Interface Contracts (between Stage 1 workers)

The frozen contracts C22/C23 already pin enum spellings, column names, and the state machine. The remaining worker-to-worker seams:

1. **A → C (the API response shapes)**: Worker A owns the JSON response structs for `work_orders` + `recommendations` list/detail. These freeze the moment A's handlers compile. **Coordination**: Worker C does NOT hand-author types — A publishes the response structs (and, if a generator exists, C runs it; otherwise C mirrors A's structs into `types.gen.ts`). Worker C should stub against the C22 HTTP-surface table until A's structs land, then reconcile. Sync point: end of A's handler implementation.
2. **B → A (recommendation → work-order creation)**: `POST /work-orders` takes a `recommendation_id`; Worker A reads the recommendation via the repo Worker B is also writing. **Contract**: the `recommendations` repo trait + row shape is frozen in C23 — both consume it; B owns writes, A owns the read in work-order creation. No ambiguity if both implement the C23 trait signature verbatim.
3. **B → (submit path)**: clustering-on-submit hooks the existing submission handler. **Contract**: B adds a *post-insert, same-transaction* `cluster_id` assignment via the new clusters repo + the `feedback.rs` setter — it must NOT change the submit handler's response shape or add latency beyond the deterministic heuristic. The submit path stays public + CORS-exposed; clustering adds no new external surface.
4. **All backend workers ↔ `.sqlx/`**: whoever adds/changes a `sqlx::query!` regenerates `.sqlx/` and commits it. Convergence resolves any cache conflict by a final `cargo sqlx prepare` over the merged tree.

---

## Coordination Requirements

- **Stage 0 must complete and pass GATE 0 before any Stage 1 worker spawns.** The contracts and the repository scaffolding are the shared substrate; fanning out earlier produces incompatible implementations.
- **Same-branch-by-default** (PODS ownership-not-isolation): file ownership is crisp (A=`work_orders.rs`+state machine; B=`clusters.rs`/`sweeps.rs`/`recommendations.rs`+corpus; C=`admin-ui/`). The only shared files are `main.rs`/`router.rs` (router merges — append-only, low conflict) and `lib.rs`/`state.rs` (Stage 0 already added the fields). If using `--worktrees`, the router-merge lines are the one manual-merge point.
- **Oracle co-evolution**: the `approval-gate-enforcement` oracle is scaffolded in Stage 0 and finalized by Worker A; A's exit criterion is a green oracle.
- **`.sqlx/` regeneration** after the migrations (Stage 0) and after any worker adds a query.
- **Convergence**: `/0-uldf-finalize` (solo/staged) or `/0-uldf-pods-converge` (PODS). Final `cargo sqlx prepare`, full `cargo test`, all oracles PASS, `admin-ui` `pnpm build` green.

---

## Deferred Decisions

| ID | Decision | Deferred to | Why |
|---|---|---|---|
| Q14-issuance | Runner write-token **minting UX** + endpoint (`/runner-tokens`) | **P5b (FR-FBR-24)** | The verification/authz *seam* is frozen in C22 now; the token issuance + the runner that uses it are the runner's concern. No runner exists in P5a. |
| Q20 | BYO-agent contract docs + reference adapter; open-vs-proprietary analyst | **P5b / packaging (DEC-FBR-12)** | Rides with the deferred packaging axis. P5a documents the work-order API as a seam in an ops doc; no reference adapter yet. |
| FR-FBR-23 | Autonomous implementer (ULDF loop) | **P5b** | The runner-facing work-order transitions exist + are guarded in P5a but nothing claims a dispatched order. |
| `agpl-boundary-check` oracle | licensing==code-location enforcement | **conditional** | Activates only if the open-core split is later chosen (DEC-FBR-12). Moot now. |
| `feedback-as-data-audit` oracle | static prompt-assembly envelope check | **P5b** | Prompt assembly is customer-side (runner); no in-repo path to guard in P5a. C24 covers the reachable surface behaviorally. |
| LLM-grade re-clustering | sweep-driven cluster overwrite via ingestion API | **P5b** | P5a ships the deterministic always-on heuristic + the ingestion seam; the intelligent re-cluster is the analyst's job. |

---

## Risks and Mitigations

| Risk | Severity | Mitigation |
|---|---|---|
| Approval-gate bypass path (RCE from public input) | 🔴 Critical | `approval-gate-enforcement` oracle (detection-from-state, exit gate for Worker A) + C22 inv. 1 pre-DB-check + same-txn ledger + C24 case (h). Isolate the security-critical surface to one undistracted worker. |
| Non-deterministic / untestable clustering | 🟠 High | Plan decision: P5a clustering is **deterministic server-side heuristic**, fixture-pinned. LLM re-cluster deferred to sweep/ingestion (P5b). |
| `.sqlx/` cache drift breaking offline CI builds | 🟡 Medium | Regen-and-commit discipline at Stage 0 + each query change; final `cargo sqlx prepare` at convergence. |
| `types.gen.ts` seam → A/C incompatible shapes | 🟡 Medium | C freezes types from A's compiled structs (Interface Contract 1); C stubs against the C22 HTTP table until A lands. |
| Accidentally CORS-exposing admin/runner endpoints | 🟠 High | Ripple analysis flags it; only the public submit/attachments/widget-config routers get `.layer(cors)`; new admin + runner routers merge WITHOUT cors. Reviewer checks the merge in `main.rs`. |
| Scope creep into P5b (building the implementer) | 🟡 Medium | Hard boundary stated up top; runner-facing transitions are *contract + guard only* in P5a, exercised by tests/no-op probe. |
| Contract drift (worker edits C22/C23 unilaterally) | 🟡 Medium | Contracts FROZEN by this plan; changes require plan revision. Stage 0 writes them into the spec (PROPOSED→FROZEN) so they're authoritative. |

---

## Execution Commands

**Recommended next step**: `/0-uldf-proceed` — context-budget-aware router. Given this session is the planning session and Stage 0 + Stage 1 are substantial (4 agents, security-critical surface), `/0-uldf-proceed` will most likely HANDOFF to a fresh orchestrated session (or PODS for Stage 1) rather than continue in-session.

Explicit control, if preferred:
- **Sequential / staged**: `/0-uldf-ltads-start` — feeds this plan as the task queue; run Stage 0 first, then Stage 1 (it can spawn an orchestrated worker per Stage-1 component).
- **Parallel Stage 1**: complete Stage 0 here or via one worker, then `/0-uldf-pods-parallelize --from-ldis-plan=docs/planning/plans/20260618T114524-feedbackmonk-p5a-agentic-resolution-loop.md` → `/0-uldf-pods-spawn-collaborator --all` (Workers A/B/C) → `/0-uldf-pods-collab-sync` → `/0-uldf-pods-converge`.

**Gate before spawning Stage 1 workers**: GATE 0 must be green (build + migrations + existing oracles PASS + `.sqlx/` committed).

═══════════════════════════════════════════════════════════════
