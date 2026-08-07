# rspd-elaboration-tree oracle

> Contexturgy substrate for Recursive Specification & Planning Delegation (RSPD, Principle 2.15 / RSPD-06). Derives the decomposition tree's *elaboration state* from the RSPD-05 spec node statuses already shipped in Phase 3, so a resuming parent/root can distinguish a **deliberately-unelaborated** subtree (`charter`, awaiting its session) from a **lost** one (`missing`, something broke). `kind: project-state`. Gracefully absent — emits `applicable:false` when the project has no delegated nodes (the common case today).

## Purpose & Responsibilities

When spec/plan authorship recurses down the tree (RSPD), the interior of a delegated node comes into existence over many sessions and weeks. The node's status records *where in the elaboration arc* it is — `[CHARTER] -> [ELABORATED] -> [IN_PROGRESS] -> [DONE]` — but reading that arc across a root spec plus arbitrarily-nested sub-spec stores is exactly the recurring investigation an oracle should pre-compute.

This oracle parses requirement statuses across the root `docs/specs/SPECIFICATION.md` and every nested `docs/specs/<domain>/SPECIFICATION.md` store it can reach, and classifies each **delegated** node (a requirement row carrying `Spec-Owner: child (delegated)` with a `<domain>/SPECIFICATION.md` pointer).

**The load-bearing distinction is `missing`.** A delegated row whose pointed-at store does *not* exist is the one case the flat progress map cannot represent and the one a resuming agent most needs surfaced: did this subtree's child author its interior and we lost it, or is it correctly deferred? See *Classification rule* below.

## File Index

| File | Purpose |
|---|---|
| `oracle.json` | Manifest -- `kind: project-state`, frozen output schema, trigger-invalidate freshness, `gracefullyAbsent`, fallback, provenance |
| `run.sh` | Unix deriver. Worklist-recurses root + nested stores, classifies each delegated node, emits one JSON object |
| `run.ps1` | Windows mirror -- byte-identical JSON shape (string-assembled, so single-element `tree` is always an array) |
| `validate.sh` | Unix self-test / smoke -- sandbox fixtures covering charter / elaborated / in_progress / missing / done-via-recursion / graceful-absence / no-spec / schema (31 assertions) |
| `validate.ps1` | Windows mirror (31 assertions) |

The `validate.{sh,ps1}` harnesses ARE this oracle's RSPD-10 smoke coverage (oracles ship validate harnesses); no separate `scripts/smoke-tests/` file is needed because the validate exercises the full derive logic.

## Public API & Usage

```bash
# Unix
.claude/oracles/rspd-elaboration-tree/run.sh

# Windows
powershell -NoProfile -ExecutionPolicy Bypass -File .claude/oracles/rspd-elaboration-tree/run.ps1
```

## FROZEN Output Schema

Programmatic consumers (e.g. the RSPD-09 LTADS/autopilot completeness check) may rely on these top-level keys. Single-line JSON:

```json
{
  "status": "ok",
  "applicable": true,
  "spec_root": "docs/specs/SPECIFICATION.md",
  "tree": [
    {
      "id": "PAY-DELEG",
      "domain": "payments",
      "status": "[CHARTER]",
      "store": "docs/specs/payments/SPECIFICATION.md",
      "store_exists": true,
      "elaboration": "charter"
    }
  ],
  "summary": {
    "total": 1, "charter": 1, "elaborated": 0,
    "in_progress": 0, "done": 0, "missing": 0
  },
  "briefing": ""
}
```

| Key | Type | Meaning |
|---|---|---|
| `status` | string | `"ok"` whenever the oracle ran (project-state oracles do not emit pass/fail). |
| `applicable` | boolean | `false` when there are no delegated nodes (or no spec) -- gracefully absent. When `false`, `tree` is `[]` and every `summary` count is `0`. |
| `spec_root` | string \| null | The root spec path, or `null` when no `docs/specs/SPECIFICATION.md` exists. |
| `tree[]` | array | One object per delegated node, in tree-walk order (root before descendants). |
| `tree[].id` | string | The requirement ID (e.g. `PAY-DELEG`). |
| `tree[].domain` | string | Store path relative to `docs/specs/`, sans `/SPECIFICATION.md` (e.g. `payments`, `auth/session`). |
| `tree[].status` | string | The declared spec status, bracketed (e.g. `[CHARTER]`). |
| `tree[].store` | string | Repo-relative path to the node's sub-spec store. |
| `tree[].store_exists` | boolean | Whether that store file is present. `false` iff `elaboration == "missing"`. |
| `tree[].elaboration` | string | The derived state -- enum: `charter` \| `elaborated` \| `in_progress` \| `done` \| `missing`. |
| `summary` | object | Tree-wide rollup: `total` + per-`elaboration` counts. |
| `briefing` | string | One-line human summary, **non-empty only when `missing > 0`** (the noteworthy case). Empty otherwise. **Wired into the session-start briefing**: the oracle is an explicit extra in the briefing fan-out (`session-start.{sh,ps1}`), so a `[rspd-elaboration-tree]` line surfaces at session start exactly when a delegated node's sub-spec store is missing; the empty-briefing case suppresses the line (gracefully absent on every project without a broken delegated node), parallel to `dispatchable-sessions`. |

**Schema contract**: the top-level keys, the `tree[]` node keys, and the `elaboration` enum values are frozen. New `elaboration` variants or node keys require a spec extension.

## Classification rule

For each delegated node, `elaboration` is derived from `(declared status) x (store existence)`:

| `store_exists` | declared status | `elaboration` |
|---|---|---|
| no | any | **`missing`** |
| yes | `[CHARTER]` | `charter` |
| yes | `[ELABORATED]` | `elaborated` |
| yes | `[IN_PROGRESS]` | `in_progress` |
| yes | `[DONE]` | `done` |

Store absence always wins (-> `missing`). This pins the operational convention that **a delegated node's `<domain>/SPECIFICATION.md` store is created -- charter-seeded -- at delegation time**; its later absence therefore means the store was *lost*, not that the node is deferred-by-design. A correctly-deferred node carries status `[CHARTER]` *and* a present (stub) store, classifying as `charter`. This is the deferred-vs-lost distinction RSPD-06 exists to make. (`[CHARTER]` is delegated-in-tree, distinct from `[DEFERRED]` = postponed-in-time; see `SPECIFICATION_STANDARD.md` Status Vocabulary.)

Only rows marked `Spec-Owner: child (delegated)` with a `<domain>/SPECIFICATION.md` pointer are tree nodes; ordinary parent-authored requirements (`[DONE]`, `[PLANNED]`, flat `modules/` partitions) are not delegated and are excluded.

## Constraints & Business Rules

- **Always-correct-at-invocation**: freshness is `trigger-invalidate` on `docs/specs/SPECIFICATION.md` + `docs/specs/**/SPECIFICATION.md`. The derive is cheap (cost scales with the number of stores, not total spec size).
- **Read-only**: never mutates spec state. Pure derivation.
- **Graceful absence on every failure path**: missing root spec, no delegated nodes, unreadable store -- all emit a consistent `applicable:false` payload, never an error. The tree functions without the oracle; this only pre-computes the read.
- **Recursion is cycle-guarded**: a `seen`-set prevents a self-referential pointer from looping; only existing stores are recursed into (a `missing` node has no interior to walk).
- **Parser portability**: bash side uses locale-independent `awk` (no `grep -P`); both sides assemble JSON as strings (no `jq` / `ConvertTo-Json` array-unwrap surprises), so bash and PowerShell output are byte-identical.

## Relationships & Dependencies

| Depends on | Why |
|---|---|
| `docs/specs/SPECIFICATION.md` (+ nested `docs/specs/<domain>/SPECIFICATION.md`) | Source of delegated rows, their statuses, and their store pointers |
| `awk` (Unix) | Locale-independent requirement-block + Spec-Owner parsing |

| Consumed by | Where |
|---|---|
| RSPD-09 LTADS / autopilot completeness check (design `RSPD_DESIGN.md` Sec 6.4) | Reads `summary.charter` (expected-incomplete-awaiting-session -- do NOT flag) vs `summary.missing` (broke -- DO flag) to avoid mis-firing on a correctly-deferred `[CHARTER]` node. References the contract; the oracle is gracefully-absent, so the consumer never hard-depends on a live invocation. |

**Session-start briefing wiring**: the oracle is registered as an explicit extra in the briefing fan-out (`~/.claude/hooks/session-start.{sh,ps1}` — alongside `planning-doc-staleness`), so a `[rspd-elaboration-tree]` line surfaces at session start only when `briefing` is non-empty (i.e. `missing > 0` — a delegated node's sub-spec store is gone). On every project without a broken delegated node the briefing is empty and the line is suppressed (gracefully absent). The oracle keeps `typical_sessions_using: "some"` (honest — only RSPD-delegation projects consult it); the explicit-extra mechanism is how it rides the every-session fan-out without misdeclaring its consultation frequency.

## Decision Log

- **`missing` is the load-bearing classification** -- the oracle's whole value is distinguishing deferred-by-design (`charter`, store present) from lost (`missing`, store absent). Store-absence dominates the declared status so a stale `[ELABORATED]`/`[DONE]` row whose store vanished is surfaced, not silently trusted.
- **Store-existence is the discriminator even for `[CHARTER]`** (operational convention set here, per the RSPD-06 task fixtures): a delegated node's store is charter-seeded at delegation, so a `[CHARTER]` row with no store is `missing`, not `charter`. There are no live delegated nodes in this repo yet (RSPD-04 shipped the mechanism, not a delegation), so against the real repo this oracle emits `applicable:false` -- the fixtures exercise the populated cases.
- **Worklist recursion over stores**, root-first, cycle-guarded, walking only existing stores -- handles arbitrary `<domain>/<sub>/` nesting without depth assumptions.
- **String-assembled JSON on both shells** rather than `jq`/`ConvertTo-Json` -- guarantees a single-element `tree` serializes as an array and that bash/PowerShell outputs match byte-for-byte (the cross-shell parity contract the validate harnesses assert).
- **Validate IS the smoke** (RSPD-10) -- the sandbox-fixture self-tests cover charter / elaborated / missing / done-via-recursion / absence / no-spec / schema, so no separate `scripts/smoke-tests/` harness is warranted.
