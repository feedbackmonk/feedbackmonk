# module-tree-map Oracle

## Synopsis

Hierarchical synopsis tree of every module in this project — the cheap-triage map that powers Hierarchical Context Triage (HCT). Come here to read the agent-facing breadth-first triage artifact: every module's `## Synopsis` plus File Index, aggregated into one JSON tree mirroring the directory hierarchy. Do NOT come here for the flat module roster — that is the sibling `module-index` oracle, which answers a different question (*"what modules exist with READMEs?"*) at a smaller token cost.

## Purpose

Answers the recurring agent question *"which subtree of this codebase is relevant to my work?"* at log(n) ingestion cost — agents read the Tree Map once for breadth-first scoping, then drill into selected subtrees instead of paying full-README cost across every breadth-first relevance candidate.

This oracle is the **load-bearing mechanism** of HCT. The Synopsis Discipline (`docs/ULADP/ULADP_PROTOCOL.md` § 1.2.1) is the data source; this oracle is the aggregation surface that makes log-cost traversal possible.

## Question

> *What is the hierarchical synopsis tree of this project's modules?*

## Output Schema

JSON, hierarchical. Mirrors the directory tree. Per `FOUNDATIONS/HIERARCHICAL_CONTEXT_TRIAGE.md` § 4.2.

```json
{
  "root": {
    "path": ".",
    "synopsis": "<root-level synopsis from project README, or null>",
    "file_index": [{"name": "...", "purpose": "..."}],
    "children": [
      {
        "path": "src/auth",
        "synopsis": "<1-5 line synopsis>",
        "file_index": [
          {"name": "tokens.ts", "purpose": "Token generation"},
          {"name": "session.ts", "purpose": "Session lifecycle"}
        ],
        "children": [
          {"path": "src/auth/session", "synopsis": "...", "children": []}
        ]
      }
    ]
  },
  "stats": {
    "total_modules": 47,
    "synopsized": 45,
    "missing_synopsis": ["src/legacy/util", "src/internal/scratch"]
  }
}
```

**Empty project** (no module READMEs): `{"root": {"path": ".", "synopsis": null, "children": []}, "stats": {"total_modules": 0, "synopsized": 0, "missing_synopsis": []}}` — graceful absence.

## Invocation

```bash
# Unix
bash .claude/oracles/module-tree-map/run.sh

# Windows
powershell -NoProfile -File .claude/oracles/module-tree-map/run.ps1
```

Or via the framework: `/0-uldf-oracle module-tree-map`.

## Consultation Protocol

**Before** doing breadth-first investigation across a project's module tree (e.g., *"which modules touch authentication?"*, *"where does state management live?"*), invoke this oracle. The output is structured for one-pass agent ingestion: read the Synopsis at each tree level, decide which subtrees to drill into, ignore the rest.

**Don't use it for**:

- Depth reading inside a single known module — read that module's README directly.
- Finding files by name across the project — use Glob.
- Listing which modules have READMEs without their content — use the sibling `module-index` oracle (smaller payload).

## Freshness

- **Strategy**: `trigger-invalidate`
- **Triggers**: `**/README.md`
- **Compute cost**: ~200ms (lightweight tree walk + per-README section extraction)

The oracle re-runs whenever any README.md changes — README modifications are the only events that materially alter Synopsis content, and they are already a checkpoint surface in `/0-uldf-finalize`.

## Relationship to Other Oracles

| Oracle | Question | Shape |
|---|---|---|
| `module-tree-map` (this) | What is the hierarchical synopsis triage tree? | Tree (recursive) |
| `module-index` (sibling) | What modules exist with READMEs, and what is each one's purpose? | Flat list |
| `synopsis-coverage` (paired Verification Oracle, HCT-04) | What fraction of modules conform to the Synopsis discipline? | Coverage report |

The three are siblings, not replacements — each answers a distinct question at a distinct cost.

## Validation

```bash
bash .claude/oracles/module-tree-map/validate.sh
powershell -NoProfile -File .claude/oracles/module-tree-map/validate.ps1
```

Asserts: T1 (single-module), T2 (multi-module hierarchical), T3 (missing-synopsis surfaced in `stats.missing_synopsis[]`), T4 (graceful empty-project), T5 (File Index entries extracted), T6 (cross-platform parity — sh + ps1 emit identical JSON for identical input; T6 is asserted by parity between the two validate scripts).

## Spec Reference

- Requirement: `docs/specs/SPECIFICATION.md` § HCT-03
- Principle: `FOUNDATIONS/HIERARCHICAL_CONTEXT_TRIAGE.md` § 4 (the Module Tree Map Oracle)
- Synopsis discipline: `docs/ULADP/ULADP_PROTOCOL.md` § 1.2.1
