# change-coupling oracle

**Question**: *Which files and modules repeatedly change together in the same commits (co-change) across module boundaries — the language-agnostic coupling signal no import graph sees?*

Kind: **project-state** (category `module`). **On-demand** — NOT in the every-session briefing fan-out (`typical_sessions_using: "on-demand"`). Invoke directly (`.claude/oracles/change-coupling/run.{sh,ps1}`) or via `/0-uldf-oracle`.

Scrutiny ADD proposal **A2** (KEEP verdict), Arc 1 deterministic detection substrate. Also an **M3** "second trigger family" member: high boundary-crossing co-change is the architecture-decomposition trigger the capacity-only (>85% context) rule misses (`docs/planning/scrutiny-hierarchical-modularity-20260707.md` §5 M3, §6 A2, §9 Arc 1).

## What it does

Mines `git log --name-only` history: two files (or modules) that repeatedly change in the same commits are coupled **in fact**, whatever the import graph says. Because it needs only `git log`, it is **language-agnostic** — it catches config/doc/cross-language coupling that no parser sees, including this repo's own bash/PowerShell/markdown substrate (the `*.sh` ↔ `*.ps1` twin pairs and the `DECISIONS.md` ↔ `SPECIFICATION.md` spec pair surface strongly).

Two aggregations:
1. **File pairs** — unordered file pairs co-changing at least `minCoChanges` times.
2. **Module pairs** — each file mapped to its **module** (nearest ancestor directory containing a `README.md`; fallback: the top-level directory component, or `(root)`), then pairs aggregated at module level. **Boundary-crossing** module pairs (`crossBoundary: true`, `moduleA != moduleB`) are the finding.

## ⚠ Correlation, not dependency (advisory)

Co-change is **evidence** of coupling, never proof of a dependency (the A2 critic's binding condition: "must stay advisory"). Two files may co-change because they share an owner, a release cadence, or a doc/code mirror — not because one imports the other. **The oracle never blocks and never exits nonzero on findings** (exit 0 always; consumers treat output as a router signal, not a gate).

## Filters (keep the signal clean)

| Filter | Default | Rationale |
|---|---|---|
| `bulkCommitMax` | 20 | Commits touching MORE than this many files are excluded (`excludedCommits` counts them). Bulk renames / format sweeps / mass reflows co-change everything with everything and pollute the signal (CodeScene change-coupling lineage). A k-file commit contributes O(k²) pairs; the ≤20 cap keeps it to ≤190 pairs/commit. |
| `minCoChanges` | 5 | A pair must co-change **at least** (`>=`) this many times to be reported. |
| `sinceDays` | 180 | History window. |
| `maxCommits` | 1000 | Hard commit cap (whichever of window/cap is smaller wins). |

**Renames**: `--name-only` lists a rename as its single (new) path, so a rename contributes no spurious pair by itself; the bulk filter covers rename/format sweeps. No separate rename-pair exclusion is applied (kept simple per the task's "if cheap" note).

## Output schema (FROZEN)

```json
{
  "status": "ok",                       // "ok" | "no-data"
  "reason": "…",                        // present only on no-data
  "shallow": false,                     // shallow clone — history may be truncated
  "window":  { "sinceDays": 180, "maxCommits": 1000, "commitsAnalyzed": 482, "qualifyingCommits": 319 },
  "filters": { "bulkCommitMax": 20, "minCoChanges": 5, "excludedCommits": 60 },
  "filePairs":        [ { "a": "path/x", "b": "path/y", "coChanges": 45 } ],
  "modulePairs":      [ { "moduleA": "docs", "moduleB": "docs", "coChanges": 277, "crossBoundary": false } ],
  "crossBoundaryTop": [ { "moduleA": "claude-template", "moduleB": "docs", "coChanges": 694, "crossBoundary": true } ],
  "truncated": true,                    // true when ANY array was capped at 50 (no silent drops)
  "cached": false,                      // true when served from the HEAD+config-keyed cache
  "briefing": "change-coupling: 178 cross-boundary pair(s); top … — advisory co-change evidence, not proven dependency."
}
```

- `filePairs`, `modulePairs`, `crossBoundaryTop` are sorted by `coChanges` descending and **capped at 50 entries each**; when any is capped, `truncated` is `true` (the no-silent-caps rule). `crossBoundaryTop` is the `crossBoundary: true` subset — the actionable finding.
- Within a tie group at the cap boundary, *which* equal-count entries fill the last slots is unspecified (hash-iteration order); the counts themselves are deterministic.

## NO-DATA honesty (never a silent "no coupling")

`status: "no-data"` with a `reason` — never an empty-but-`"ok"` result that reads as "no coupling" — is emitted when the history cannot be analyzed:

| Reason | Trigger |
|---|---|
| `git not available` / `not a git repository` | no git / outside a work tree |
| `no commit history` | repo with no HEAD |
| `no commits in the N-day window` | empty window |
| `insufficient history (N commit(s) in window)` | `< 2` commits — no co-change possible (covers shallow clones with little depth; `shallow: true` is also surfaced) |
| `no co-change-analyzable commits (all single-file or bulk-filtered)` | commits exist but none has 2..`bulkCommitMax` files |

Distinguished from a genuine clean result: when there ARE qualifying commits but no pair reaches `minCoChanges`, that is `status: "ok"` with empty arrays and a "checked, clean" briefing — the oracle **did** check.

## Cache

Keyed by `HEAD sha + config` at `.claude/session-state/change-coupling-cache.json` (session-state, **never committed**). History mining is slow on large repos (~5 s cold on this repo, 482 commits / ~1.6 k files); a cached rerun on the same HEAD+config is <1 s. A config or HEAD change invalidates the cache. Both shells share one interchangeable cache format (`{key, result}` where `result` is the raw JSON string with `cached` flipped on the fast path).

## Config

`.claude/config.json` → `changeCoupling` (all optional):

```json
{ "changeCoupling": { "sinceDays": 180, "maxCommits": 1000, "bulkCommitMax": 20, "minCoChanges": 5 } }
```

## Self-test

```bash
bash validate.sh          # Unix
powershell -File validate.ps1   # Windows
```

Asserts valid JSON, the frozen schema fields, `status ∈ {ok, no-data}`, and the NO-DATA path on a non-git directory. Full behavioral coverage (synthetic git fixture: coupled cross-module pair, in-module pair, a bulk commit that must be filtered out; bash + PowerShell parity) is in `~/.claude/scripts/smoke-tests/change-coupling-oracle-smoke.sh`.

## Cross-shell parity note

`run.sh` (awk, case-sensitive arrays) and `run.ps1` (ordinal `Dictionary`) produce byte-identical values on tie-free input. Two parity-load-bearing subtleties are handled: (1) awk pass-2 uses `-F'\t'` so file paths with spaces don't mis-split; (2) PS uses `[StringComparer]::Ordinal` dictionaries because the default `@{}` hashtable is case-insensitive and would wrongly merge case-distinct git paths (e.g. `LTADS/` design docs vs `ltads/` runtime state). Git is case-sensitive; both shells honor that.
