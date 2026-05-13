# Oracles — Project Oraculurgy Directory

## Synopsis

Project-level Oraculurgy directory and authoring guide for deterministic agent-facing programs that pre-answer recurring questions. Come here for the four-part qualification test, manifest schema (incl. `kind: project-state` vs. `kind: verification`), freshness strategies, and the anti-pattern catalog before authoring or modifying an oracle in this project. Don't come here for live oracle output — invoke individual oracles in their subdirectories — or for the agent-readable enumeration of which oracles exist, which is `INDEX.md`.

## Overview

This directory holds **oracles**: deterministic programs that answer recurring questions LLM agents would otherwise derive through investigation. Practicing the discipline of building and curating these programs is called **Oraculurgy** — a first-class ULDF principle, sibling to Contexturgy.

Read first:
- `FOUNDATIONS/ORACULURGY_DESIGN.md` — full conceptual and architectural specification
- `docs/TERMINOLOGY_STANDARDS.md` — formal definitions and relationships
- `FOUNDATIONS/PRINCIPLES_OF_LLM_AGENT_ORCHESTRATION.md` Section 2.12 — Principle of Oraculurgy

## Quick Start for Agents

**You are an agent working on a task.** Before investigating project state, check `INDEX.md` in this directory to see whether an oracle already answers your question. Example:

```
Read .claude/oracles/INDEX.md
→ You see: `git-state` answers "Current branch, dirty count, last commit"
→ Invoke it: bash .claude/oracles/git-state/run.sh
→ You receive a JSON answer in one tool call instead of running git status, git log, parsing output, etc.
```

If no oracle matches your question, investigate as normal — but during `/0-uldf-finalize`, Phase 11 may propose your investigation as an oracle candidate for future sessions.

## Quick Start for Humans

**You are maintaining this project's oracles.** The starter set is installed by `/0-uldf-setup-project` and includes seven universal oracles (see `INDEX.md`). To add project-specific oracles:

1. Identify a recurring agent investigation (typically through `/0-uldf-finalize` Phase 11 candidate reports)
2. Confirm the four-part qualification test passes (deterministic, recurrent, freshness-contractable, gracefully absent)
3. Create `<oracle-name>/` with `oracle.json` manifest, `run.sh`/`run.ps1`, and `validate.sh`/`validate.ps1`
4. Add an entry to `INDEX.md`
5. Commit — `/0-uldf-finalize` Phase 11 will revalidate on subsequent runs

## Directory Structure

```
.claude/oracles/
├── README.md                 # This file
├── INDEX.md                  # Catalog of all oracles (agent-readable)
├── <oracle-name>/
│   ├── oracle.json           # Manifest (machine-readable)
│   ├── README.md             # Human/agent description (optional)
│   ├── run.sh                # Unix executable
│   ├── run.ps1               # Windows executable
│   ├── validate.sh           # Self-test (Unix)
│   ├── validate.ps1          # Self-test (Windows)
│   └── cache/                # Optional, for cached freshness strategies
│       └── latest.json
├── candidates/               # Draft oracles discovered by /0-uldf-finalize Phase 11
│   └── <candidate-name>.json
└── shared/                   # Shared libraries/utilities for oracle implementations
    └── ...
```

## The Four-Part Qualification Test

An artifact qualifies as an oracle only if **all four** hold:

1. **Deterministic Replacement** — A program can answer the question as well or better than the agent. If judgment is required, it is not an oracle.
2. **Recurrence** — The question is asked often enough that the build cost amortizes.
3. **Freshness Contract** — The oracle has a declared freshness strategy (`always-fresh`, `cache-ttl`, or `trigger-invalidate`). **Stale oracles are worse than no oracles.**
4. **Graceful Absence** — If the oracle is missing or broken, the workflow continues (just slower). Oracles are accelerators, never gatekeepers.

Fail any criterion and it is not an oracle. It may still be useful (tool, script, doc), but it does not belong here.

## Manifest Schema

Every oracle has an `oracle.json` manifest:

```json
{
  "name": "project-type",
  "version": "1.0.0",
  "question": "What language, framework, and build system does this project use?",
  "category": "environment",
  "kind": "project-state",
  "invocation": {
    "unix": ".claude/oracles/project-type/run.sh",
    "windows": ".claude/oracles/project-type/run.ps1"
  },
  "output": {
    "format": "json",
    "schema": {
      "language": "string",
      "framework": "string|null",
      "build_system": "string",
      "test_command": "string|null",
      "dev_command": "string|null"
    }
  },
  "freshness": {
    "strategy": "always-fresh",
    "compute_cost_ms": 50
  },
  "consultation": {
    "typical_sessions_using": "every",
    "estimated_token_savings_per_call": 400
  },
  "validation": {
    "self_test_unix": ".claude/oracles/project-type/validate.sh",
    "self_test_windows": ".claude/oracles/project-type/validate.ps1",
    "last_validated": "2026-04-08T00:00:00Z"
  },
  "fallback": "Read package.json, Cargo.toml, pyproject.toml, etc., and infer from their presence and contents.",
  "provenance": {
    "authored_by": "shared-pack:uldf-common",
    "created": "2026-04-08T00:00:00Z"
  }
}
```

All fields are required except `output.schema` (may be omitted for text-format oracles), `cache/` directory (only used by caching strategies), and `validation.last_validated` (populated on first run).

### `kind` field — project-state vs. verification

The `kind` field declares which **oracle kind** this oracle belongs to. Two values are defined:

| Kind | Question shape | Typical strategy | Examples |
|------|---------------|------------------|----------|
| **`project-state`** | "What is in this project?" | `always-fresh` or `trigger-invalidate` | `project-type`, `git-state`, `module-index`, `spec-status` |
| **`verification`** | "Did the last action break or violate something?" | `always-fresh` (only) | `markdown-link-validity` |

`kind` is **orthogonal to `category`**. Category answers *"what domain does this oracle's question belong to?"* (`environment`, `git`, `spec`, `module`, `documentation`, etc.). Kind answers *"what oracle-shape does this oracle have?"*. A verification oracle still has a category — pick the closest existing one; do not invent new categories.

`kind` is optional for backward compatibility — manifests without an explicit `kind` are treated as `project-state`. Adding the field to existing oracles is recommended but not required.

### Verification Oracle additional contracts

Manifests declaring `kind: "verification"` MUST satisfy these additional constraints beyond the base oracle contract:

- **Output shape**: top-level `status: "pass" | "fail" | "warn"` plus a `details` object describing what was checked and what failed.
- **Agent-actionable failure entries**: every entry inside `details.broken[]` (or equivalent) must include source location (file path, line number when applicable), the failed check in human-readable form, and the resolved/queried value that failed. No bare `{"file": "foo"}` entries.
- **Speed contract**: must run in **<2s on a typical project**. Verification oracles run in the inner loop of agent iterations; slower checks belong in the two-layer-verification outer loop.
- **Idempotence**: read-only. May write to `cache/` only. No other side effects.
- **Freshness strategy**: typically `always-fresh`. `cache-ttl` is forbidden for verification oracles (TTL caching loses the link between an iteration's diff and its verification answer). `trigger-invalidate` is acceptable only when the trigger set is comprehensive.

Full category specification: `FOUNDATIONS/ORACULURGY_DESIGN.md` Part 11 (Verification Oracles).

## Freshness Strategies

| Strategy | Guarantee | Use when |
|---|---|---|
| **always-fresh** | Correct at moment of invocation | Cheap questions (<200ms), live-changing state |
| **cache-ttl** | Correct as of at most N seconds ago | Moderate-cost questions whose answer changes slowly |
| **trigger-invalidate** | Correct as of last change to declared trigger files | Expensive questions whose answer changes only on specific file edits |

Manifests using `cache-ttl` require `ttl_seconds`. Manifests using `trigger-invalidate` require `triggers` (a list of glob patterns).

## Anti-Patterns to Avoid

- **Stale Oracle** — freshness contract silently violated; agents act on wrong data
- **Judgment Oracle** — encoding LLM judgment in heuristics; produces confident-sounding wrong answers
- **Chatty Oracle** — output so large that consulting it costs more tokens than investigating directly
- **Gold-Plated Oracle** — excessive configurability; oracles should answer one question, one way
- **Redundant Oracle** — two oracles answering the same question from different sources
- **Vanity Oracle** — built to show "we do oraculurgy" but answers questions no agent asks
- **Load-Bearing Oracle** — workflow breaks when the oracle is absent; violates Graceful Absence

See `FOUNDATIONS/ORACULURGY_DESIGN.md` Section 6.2 for full anti-pattern catalog and mitigations.

## Validation Regime

Every oracle has a self-test that exercises the oracle and verifies the output against known-good state. The validation regime has three layers:

1. **Self-test** — Per oracle, runs on demand and during `/0-uldf-finalize` Phase 11 (skip with `--skip-oraculurgy`)
2. **Regression test** — During `/0-uldf-finalize`, any oracle whose trigger files changed is revalidated; failure blocks commit
3. **Staleness sweep** — Oracles not validated within the staleness threshold (default 30 days) are flagged and re-tested

Stale oracles are marked `"stale": true` in their manifest. Stale oracles are excluded from the session-start briefing.

## How Agents Consult Oracles

Three mechanisms, in order of efficiency:

1. **Hook-injected briefing** — The `session-start` hook pre-invokes oracles with `consultation.typical_sessions_using = "every"` and surfaces their output in the agent's initial context. Zero agent action required.

2. **Index lookup** — The agent reads `INDEX.md`, finds the relevant oracle, and invokes it via `Bash` or equivalent. One agent action per consultation.

3. **Direct invocation** — The agent calls `bash .claude/oracles/<name>/run.sh` knowing the oracle's location in advance (e.g., from prior experience or from a module's README Oracles section).

All three return raw oracle output. Agents parse per the declared output schema.

## Integration Points

- **`session-start` hook** — Invokes every-session oracles, assembles startup briefing
- **`/0-uldf-finalize` Phase 11** — Oraculurgy Audit: revalidates, sweeps staleness, discovers candidates, reports economics
- **`/0-uldf-ldis-spec`** — Spec sessions identify oracles that implementing agents will need (proactive oraculurgy)
- **ULADP module README** — Section 7 (Oracles) lists module-specific oracles agents should consult before touching the module
- **`/0-uldf-setup-project`** — Installs the universal starter oracle set

## Authoring a New Oracle

1. Read `FOUNDATIONS/ORACULURGY_DESIGN.md` Sections 3 and 8 for the full specification and reference implementations.
2. Apply the four-part qualification test.
3. Create `<oracle-name>/oracle.json` with all required fields.
4. Implement `run.sh` and `run.ps1` (cross-platform — both must produce identical output).
5. Implement `validate.sh` and `validate.ps1` that verify the oracle against known-good state.
6. Add an entry to `INDEX.md` at the right category.
7. Run `validate.sh` manually to confirm it passes.
8. Commit. `/0-uldf-finalize` Phase 11 will include it in the next audit.

## Authoring Convention Notes

- Keep `run.sh` and `run.ps1` under ~100 lines each when possible
- Use `shared/` for helpers that multiple oracles share
- Output should be under 500 tokens — split large oracles into multiple narrow ones
- Include a one-paragraph `README.md` in the oracle directory if the implementation is non-obvious
- Use `jq` for JSON output in Unix; `ConvertTo-Json` in PowerShell
- All scripts must be ASCII-only (the template convention — see commit `be011cc`, `e82c6af`)
