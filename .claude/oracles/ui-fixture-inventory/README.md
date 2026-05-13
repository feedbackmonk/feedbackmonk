# `ui-fixture-inventory` Oracle

> **Synopsis**: Project-state oracle answering *"what fixture/smoke-test infrastructure does this project have, and what conventions are in use?"* Composes with `ui-surface-detector` (ARIA-02) for the upstream "is there UI?" question; this oracle answers the downstream "is there fixture infrastructure?" question. Consumed by `/0-uldf-ldis-plan` Phase 4 testability gate Q4 (scaffolding-leverage scoring) and surfaced as `[fixture-inventory]` in the session-start ORACLE BRIEFING for surface-with-no-fixtures projects.

## Purpose & Responsibilities

Answers the recurring question that the Phase 4 testability gate Q4 needs: *"What fixture conventions does this project actually use, and how cheap is fixture-style scaffolding here?"* Without this oracle, agents discover fixture conventions by precedent-rediscovery (the SessionHelm D123 trigger incident — agent had to remember `tests/fixtures/diff-panel-smoke.ts` existed by name). This oracle pre-answers the question deterministically.

## File Index

- `oracle.json` — manifest declaring `kind: "project-state"`, frozen output schema, freshness strategy
- `run.sh` — Unix invocation (filesystem stat + glob; never executes project scripts)
- `run.ps1` — Windows PowerShell invocation (parity with `run.sh`)
- `validate.sh` — three-scenario self-test (Unix)
- `validate.ps1` — three-scenario self-test (Windows)
- `README.md` — this file

## Public API & Usage

### Output Schema (frozen)

```json
{
  "has_fixtures": false,
  "patterns": ["tests/fixtures/*-smoke.{ts,js,py}", "tests/smoke/*.spec.{ts,js,py}", "playwright.config.ts"],
  "counts": { "fixtures": 0, "smoke_specs": 0, "e2e_specs": 0 },
  "conventions": ["co-located smoke", "playwright"],
  "briefing": ""
}
```

| Field | Type | Notes |
|-------|------|-------|
| `has_fixtures` | bool | `true` iff any pattern matched OR any count > 0 |
| `patterns` | string[] | Detected glob patterns (NOT all possible — only the ones present) |
| `counts.fixtures` | int | Files matching `tests/fixtures/*-smoke.{ts,js,py}` |
| `counts.smoke_specs` | int | Files matching `tests/smoke/*.spec.{ts,js,py}` |
| `counts.e2e_specs` | int | Files matching `e2e/**/*.spec.{ts,js,py}` |
| `conventions` | string[] | High-level conventions: `co-located smoke`, `playwright`, `cypress`, `jest fixtures`, `vitest fixtures`, `visual regression` |
| `briefing` | string | ≤200 chars; what the session-start hook emits as `[fixture-inventory]`. Empty string suppresses the line. |

### Briefing-Line Forms

| Condition | Briefing |
|-----------|----------|
| No surface (`ui-surface-detector` says `none`/`cli-tool`/`backend-service`) | `""` (line suppressed) |
| Surface present + fixtures present | `""` (line suppressed — no nudge needed for healthy projects) |
| Surface present + no fixtures | `"fixture-inventory: UI surface detected; no fixture infrastructure. /0-uldf-ldis-plan can scaffold."` |

### Direct Invocation

```bash
# Unix
.claude/oracles/ui-fixture-inventory/run.sh
```

```powershell
# Windows
.claude\oracles\ui-fixture-inventory\run.ps1
```

### Self-Test

```bash
bash .claude/oracles/ui-fixture-inventory/validate.sh
```

```powershell
.claude\oracles\ui-fixture-inventory\validate.ps1
```

## Constraints & Business Rules

- **Filesystem stat + glob only.** Never executes project scripts (no `npm test`, no `cargo test`, no `pytest`). The oracle is consulted from the session-start hook; it MUST be safe to run on any project unconditionally.
- **Bounded depth.** Searches go to `maxdepth 6` (Unix) / `Recurse -Depth 6` (PowerShell) to avoid pathological repos.
- **Frozen output schema.** Adding new fields is non-breaking; renaming or removing fields requires a `version` bump in `oracle.json` and downstream-consumer updates (TGFP-04 testability gate, TGFP-05 persistence check).
- **Composes with `ui-surface-detector`.** No-surface projects short-circuit immediately with empty patterns/counts/briefing. Backend-service and cli-tool surfaces also short-circuit (fixture-inventory is UI-scoped).

## Relationships & Dependencies

- **Upstream**: `ui-surface-detector` (ARIA-02) — composed for surface presence
- **Downstream**: 
  - `/0-uldf-ldis-plan` Phase 4 testability gate Q4 (TGFP-04)
  - Session-start hook briefing emission (TGFP-03)
  - Future: `/0-uldf-finalize` Phase 0.6 may read the oracle's `patterns` field to predict where fixture-evidence files would land for matched flagged items

## Decision Log

- **2026-05-09 — Briefing-line policy**: surface-with-fixtures suppresses the line (no nudge for healthy projects); only surface-with-no-fixtures emits. Mirrors `aria-status`'s present-and-healthy suppression. WHY: spam-prevention; agents only need the nudge when there's an action to take.
- **2026-05-09 — Pattern set is iterable**: initial set covers TypeScript/JavaScript/Python conventions (co-located smoke, playwright, cypress, jest, vitest, visual). Patterns may be extended in future versions without breaking the output schema (the `patterns` array is open-ended). WHY: avoid premature pattern-set commitment; let real-project usage drive expansions.
- **2026-05-09 — Surface kind exclusions**: `cli-tool` and `backend-service` short-circuit alongside `none`. WHY: fixture-inventory is UI-scoped; agentic UI fixtures (the threat model the Phase 4 gate Q4 defends) don't apply to backend services or CLIs. If those surfaces grow agent-driven testing patterns, the exclusion can be relaxed.

## Cross-References

- **Spec**: TGFP-02 in `docs/specs/SPECIFICATION.md`
- **Decisions**: DEC-57 (mechanism framing), DEC-58 (justification artifact path)
- **Related oracles**: `ui-surface-detector` (composed upstream), `aria-status` (parallel pattern for ARIA briefing)
- **Consumers**: `claude-template/segments/-ldis/plan-phase4-testability-gate.md` Q4 (TGFP-04), `claude-template/hooks/session-start.{sh,ps1}` (TGFP-03), `claude-template/segments/-finalize/phase0.6-testability-persistence-gate.md` (TGFP-05)
