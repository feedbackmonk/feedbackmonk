# synopsis-coverage Oracle

## Synopsis

Verification Oracle reporting what fraction of the project's modules conform to the HCT Synopsis discipline. Come here for the dogfood progress meter during HCT-06 migrations and for `/0-uldf-uladp-compliance`'s underlying Synopsis presence/length check. Do NOT come here for the hierarchical synopsis tree itself — that is the sibling `module-tree-map` oracle, which answers a different question.

## Purpose

Conformance check: every module README has a `## Synopsis` H2 section AND content is between 1 and 5 non-empty lines. Modules that fail either condition are surfaced with their path so the agent can act on them.

This is a **Verification Oracle** (`kind: "verification"` per `FOUNDATIONS/ORACULURGY_DESIGN.md` Part 11) — it answers *"did the last action break or violate something?"* in execution-state terms. It does not just report state; it reports a pass/fail signal calibrated to a discipline.

## Question

> *What fraction of this project's modules conform to the HCT Synopsis discipline (presence + length)?*

## Output Schema

```json
{
  "coverage_pct": 92,
  "conformant_count": 47,
  "total_modules": 51,
  "missing": ["src/legacy/util", "src/internal/scratch"],
  "over_length": ["docs/foo"],
  "briefing_summary": "92% (2 missing, 1 over-length). Run /0-uldf-uladp-compliance for details.",
  "briefing": "92% (2 missing, 1 over-length). Run /0-uldf-uladp-compliance for details."
}
```

- `coverage_pct`: integer 0-100. `100` when `total_modules == 0` (graceful absence).
- `missing[]`: module paths with a README but no `## Synopsis` section (or zero non-empty content lines). Sorted lexically.
- `over_length[]`: module paths with a Synopsis section exceeding 5 non-empty lines (soft-violation surface). Sorted lexically.
- `briefing_summary` / `briefing`: one-liner suitable for ORACLE BRIEFING display. Empty string when `coverage_pct == 100` — gracefully absent from the briefing line. Both fields carry the same value (`briefing_summary` is the spec-named field; `briefing` is the hook-iteration convention).

## Invocation

```bash
# Unix
bash .claude/oracles/synopsis-coverage/run.sh

# Windows
powershell -NoProfile -File .claude/oracles/synopsis-coverage/run.ps1
```

Or via the framework: `/0-uldf-oracle synopsis-coverage`.

## Consultation Protocol

**Before** asserting that the project is HCT-conformant, invoke this oracle. The output is sufficient for the `/0-uldf-uladp-compliance` Synopsis check (HCT-07) and for HCT-06 dogfood-migration progress monitoring.

**During an HCT migration** (per `docs/specs/SPECIFICATION.md` § HCT-06 acceptance), workers run this oracle periodically as their progress meter; their assigned subtree must reach 100% coverage before signaling completion.

## Freshness

- **Strategy**: `trigger-invalidate`
- **Triggers**: `**/README.md`
- **Compute cost**: ~100ms (lightweight per-README scan; no tree assembly)

## Verification Oracle Contract

Per `FOUNDATIONS/ORACULURGY_DESIGN.md` Part 11:

- **Read-only**: never mutates project state.
- **<2s runtime**: well under the cap; ~100ms typical.
- **Agent-actionable failure entries**: `missing[]` and `over_length[]` give the agent exact paths to fix.
- **Deterministic**: identical project state -> identical output.

## Validation

```bash
bash .claude/oracles/synopsis-coverage/validate.sh
powershell -NoProfile -File .claude/oracles/synopsis-coverage/validate.ps1
```

Asserts: T1 all-conformant -> 100%, T2 missing -> correct `missing[]` population, T3 over-length -> correct `over_length[]` population, T4 graceful empty-project, T5 cross-platform parity (asserted by parity between the two validate scripts producing equivalent assertions over equivalent fixtures).

## Spec Reference

- Requirement: `docs/specs/SPECIFICATION.md` § HCT-04
- Verification Oracle category: `FOUNDATIONS/ORACULURGY_DESIGN.md` Part 11
- Synopsis discipline: `docs/ULADP/ULADP_PROTOCOL.md` § 1.2.1
- Principle: `FOUNDATIONS/HIERARCHICAL_CONTEXT_TRIAGE.md`
