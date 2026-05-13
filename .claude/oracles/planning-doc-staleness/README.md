# planning-doc-staleness Oracle

## Synopsis

Verification Oracle (`kind: "verification"` per Oraculurgy Part 11) that detects shipped planning docs in `docs/planning/intakes/` and `docs/planning/plans/` whose corresponding work has landed but whose files have not been moved to `docs/planning/archive/`. Detection-only — the action leg is `/0-uldf-finalize` Phase 8.7 (SWEEP-03), which consumes the `stale[]` partition. The `unknown[]` partition surfaces heuristic uncertainty for human triage and is **never** auto-archived (DEC-53 safety direction).

> **Category**: spec | **Kind**: verification | **Strategy**: always-fresh | **Speed contract**: <2s

## Why this oracle exists

Trigger: DISC-HYGIENE-03 — the SessionHelm 2026-05-07 audit and same-pattern reproduction in this very repo (15 intakes / 1 archived; 13 plans / 1 archived). The previous mechanism was advisory text inside `claude-template/skills/0-uldf-finalize/SKILL.md` lines ~635-639 instructing finalizer subagents to manually migrate planning docs. That advisory was consistently ignored. Detection without enforcement is the failure mode this oracle (paired with Phase 8.7) replaces. Per DEC-53, detection (oracle) and action (finalize Phase) are split — the Probandurgy idiom.

## Heuristic — what counts as "stale"

A planning doc is partitioned by two work-shipped signals (DEC-53):

**Signal 1 — `commit-hash-found`**: scan `git log --since=60.days.ago` for any commit whose message references the planning doc's slug (filename minus the leading `\d{8}T\d{6}-` timestamp prefix and `.md` suffix) OR the full basename. Match is case-insensitive and substring-anchored. The 60-day window covers a full quarterly cadence; older signals are not recomputed because re-discovery would require re-reading every commit.

**Signal 2 — `all-spec-entries-done`**: parse the planning doc's body for spec-entry references of the form `[A-Z][A-Z0-9]*-\d+` (e.g., `SWEEP-03`, `CSI-14`, `DEC-53`). For each referenced entry, look it up in `docs/specs/SPECIFICATION.md` and check whether its status marker is `[DONE]` or `[DELIVERED]`. If at least one valid spec reference is found AND every found reference is `[DONE]` or `[DELIVERED]`, signal fires. If no spec references are found, signal is silent (not negative — absence of signal, not negation).

| stale[] entry's `staleness_signal` | Signal 1 | Signal 2 |
|------------------------------------|----------|----------|
| `commit-hash-found`                | yes      | no (or no refs found) |
| `all-spec-entries-done`            | no       | yes      |
| `both`                             | yes      | yes      |

**`fresh[]`**: neither signal fires AND the file's mtime is within the last 14 days. Recent activity is treated as evidence the planning doc is in-flight, not stale.

**`unknown[]`**: neither signal fires AND mtime is older than 14 days. The oracle cannot confidently classify these — surfaced for human triage by Phase 8.7 with a `reason` field. **Never auto-archived.**

## Heuristic transparency — known edge cases

The README is the contract for what the heuristic does and does not catch:

- **False-positive defense**: the heuristic does not auto-archive `unknown[]` entries. A commit-message reference to a planning doc whose spec entries are still `[PLANNED]` produces an `unknown[]` entry, not a `stale[]` entry — work is in flight even though the filename was mentioned in passing. Phase 8.7 surfaces these for the human to decide.
- **False-negative defense**: a planning doc that has fully shipped but whose spec entries were never written (or were written under different IDs) and whose filename never appeared in a commit message will land in `unknown[]` after 14 days. The human archives manually. Better quiet false-negative than loud false-positive.
- **Slug derivation**: filename `20260508T034022-hygiene-arc-3-disk-side-sweep-mechanisms.md` → slug `hygiene-arc-3-disk-side-sweep-mechanisms`. Both forms (full basename + slug) are tried against commit messages.
- **Archive directory exclusion**: files already under `docs/planning/archive/` are not classified — they have already been swept.
- **Missing planning dirs**: if neither `docs/planning/intakes/` nor `docs/planning/plans/` exists, the oracle returns empty arrays + empty briefing. Graceful absence.
- **Malformed planning doc**: a planning doc with no readable content (zero bytes, binary, encoding error) is partitioned by mtime alone (no spec refs detectable → signal 2 silent; signal 1 may still fire).

## Output schema (frozen — README is the contract)

```json
{
  "stale": [
    {
      "path": "docs/planning/plans/20260101T010101-foo.md",
      "staleness_signal": "commit-hash-found",
      "last_modified": "2026-04-15T10:00:00Z"
    }
  ],
  "fresh": [
    {"path": "docs/planning/plans/20260507T180000-bar.md"}
  ],
  "unknown": [
    {
      "path": "docs/planning/intakes/20260301T120000-baz.md",
      "reason": "no commit reference; no spec mapping detectable; mtime 38 days"
    }
  ],
  "briefing": "[planning-doc-staleness] N stale planning docs, run /0-uldf-finalize to archive"
}
```

Field rules:

- `stale[]` — entries that should be archived by Phase 8.7. Each entry MUST include `path`, `staleness_signal` (one of `commit-hash-found` | `all-spec-entries-done` | `both`), `last_modified` (ISO-8601 UTC).
- `fresh[]` — entries deliberately retained (in-flight work). Each entry includes `path` only; no further fields.
- `unknown[]` — entries surfaced for human triage. Each entry MUST include `path` and `reason` (free-form one-line diagnostic).
- `briefing` — empty string when `stale[]` empty AND `unknown[]` empty (graceful silence at session-start hook). Otherwise: `[planning-doc-staleness] N stale planning docs, run /0-uldf-finalize to archive`.

**Schema deviation from Oraculurgy Part 11 §11.3.1** (`{status, details}` shape): justified — the partition-shaped output makes Phase 8.7's consumption a straight read of `stale[]` without re-parsing details. The `briefing` field replaces the headline `status` for hook-side rendering. Per-entry agent-actionability (§11.3.2) is preserved: every entry carries source-location (`path`) and the failed-check signal (`staleness_signal` / `reason`).

## Speed contract

`always-fresh` with `compute_cost_ms: 1700`. Oraculurgy Part 11 §11.3.3 sets a hard 2s budget for Verification Oracles; this oracle's design fits that envelope by minimizing fork count on Windows MSYS:

- One `git log --since=60.days.ago --pretty=format:%s` invocation
- One `awk` pass over `SPECIFICATION.md` (builds spec ID → status table)
- One `awk` pass over all planning docs (builds the per-file refs-found / refs-done table)
- One `stat` call across all planning docs (mtime table)
- Per-file work uses bash builtins (`${var,,}` for lowercasing, parameter expansion lookup against the precomputed tables, `printf '%(…)T'` for ISO-8601 formatting) — zero forks per iteration.

Measured cost on the ULDF repo: ~1.7s on Git Bash (Windows), ~0.5s on PowerShell. Both well under the 2s contract. If runtime ever drifts above 2s, the per-file iteration is the place to add bounds (e.g., scope the planning-doc walk to the diff via `recent-activity` per Oraculurgy §11.5).

## Idempotence

Read-only. Never modifies the filesystem (no `cache/` writes either). Calling it twice in a row returns the same answer for the same project state.

## Invocation

```bash
# Unix / Git Bash
bash .claude/oracles/planning-doc-staleness/run.sh

# Windows PowerShell
powershell -NoProfile -File .claude/oracles/planning-doc-staleness/run.ps1
```

Both produce structurally identical JSON; only field ordering may differ.

## Modes

This oracle has **no `--gc` mode** — it is a read-only Verification Oracle by definition (Part 11 §11.3.4). The action leg lives in `/0-uldf-finalize` Phase 8.7 (SWEEP-03), which consumes `stale[]` and uses `git mv` to preserve history. Because Phase 8.7 uses `git mv` (move, not delete), there is no audit-trail JSONL invariant for this surface — the move is reversible by inspecting git history.

## When to consult this oracle

- **At session-start** via the hook (briefing-line nudge when drift exists)
- **Inside `/0-uldf-finalize` Phase 8.7** (action leg consumes the output)
- **Reflexively** after shipping an arc whose work is documented in `docs/planning/`
- **Before manual archival** (`/0-uldf-finalize --recommend-planning-archive` runs the oracle and surfaces output without acting)

## Fallback (Graceful Absence)

If this oracle is missing or broken, the agent can manually:

1. List files in `docs/planning/intakes/` and `docs/planning/plans/`
2. For each, scan `git log --since=60.days.ago --grep=<slug>` and check `docs/specs/SPECIFICATION.md` for the spec entries it references
3. Move via `git mv <path> docs/planning/archive/<filename>`

Slow and error-prone vs. the oracle, but the workflow continues.

## Self-test

`validate.sh` and `validate.ps1` cover ≥7 cases each: shipped-via-commit, shipped-via-spec-status, shipped-via-both, in-flight (recent mtime), mixed (multiple docs across partitions), no-planning-dir (graceful absence), malformed (partition by mtime only). Run via `.claude/oracles/planning-doc-staleness/validate.sh`.

## Cross-references

- Spec: `docs/specs/SPECIFICATION.md` § SWEEP-02 (this oracle), SWEEP-03 (Phase 8.7 action leg), SWEEP-07 (session-start hook wire-in)
- Decision: DEC-53 (heuristic + safety direction); DEC-54 (no TTL — work-shipped signal, not time)
- Discovery: DISC-HYGIENE-03 (trigger pattern)
- Substrate: `claude-template/oracles/markdown-link-validity/` (precedent — only other `kind: "verification"` oracle in the framework)
- Verification Oracle conventions: `FOUNDATIONS/ORACULURGY_DESIGN.md` Part 11
