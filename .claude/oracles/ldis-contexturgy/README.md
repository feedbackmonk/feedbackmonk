# ldis-contexturgy Oracle

## Synopsis

Advisory verification oracle (CTXY-01, scrutiny 03 ADD-1) answering "did recent LDIS invocations in this project actually crystallize planning artifacts?" — the deterministic detector for Ephemeral Planning, the LDIS family's named primary anti-pattern that was previously enforced only by ALL-CAPS prose. Come here for the frozen output schema and the never-fail contract. Don't come here for what the artifacts should contain (`templates/planning-README.md`) or for spec staleness (`planning-doc-staleness`).

## Contract

- **Inputs**: the command-usage ledger (`~/.claude/command-usage/*.jsonl`; framework repo: `claude-usage/*.jsonl`; test override `CLAUDE_USAGE_LEDGER_DIR`), filtered to `type:skill` records of `0-uldf-ldis-{intake,plan,ideate,spec}` for THIS project (ledger `project` == project dir name) within the lookback window (default 24h; `CLAUDE_CTXY_WINDOW_HOURS` override).
- **Check per invocation**: a file exists in the skill's artifact home with mtime >= the invocation time — `intake→docs/planning/intakes/`, `plan→docs/planning/plans/`, `ideate→docs/planning/ideations/`, `spec→docs/specs/` (any `*.md`, recursive).
- **ADVISORY — never fail**: status is `pass` | `warn` only. A ledger/mtime false positive must not block a commit; the mandate's teeth stay instructional, this oracle makes skips *visible*.
- **NO-DATA honesty**: no ledger → `pass` with an explicit `details.note` saying Contexturgy was not verifiable — never a silent all-clear.

## Frozen output schema

```json
{
  "status": "warn",
  "details": {
    "checked": 2,
    "window_hours": 24,
    "gaps": [
      { "skill": "0-uldf-ldis-intake", "at": "2026-07-02T10:00:00", "expected": "docs/planning/intakes" }
    ],
    "note": ""
  },
  "briefing": "ldis-contexturgy: 1 LDIS invocation(s) in the last 24h left no crystallized artifact (Ephemeral Planning?) -- advisory, crystallize before finalize"
}
```

## Consumers

`/0-uldf-finalize` Phase 1a — auto-discovered by the standard `run-verification-oracles` runner (`kind: "verification"`, fast lane); `warn` surfaces in the finalize report but never blocks. Also invocable on demand during LDIS sessions as a self-check.

## Notes

- Window is a fixed lookback (not last-commit-anchored): the ledger's `at` is local time without offset, and mixing it with git's offset-bearing timestamps invites tz bugs; 24h covers a working session and finalize runs at session cadence.
- Requires the command-usage tracker hook (ships with the framework); projects without it get the NO-DATA note.
- PS gotcha baked into run.ps1: `$home` is an automatic variable — the artifact-home local is `$artHome` (assignment to `$home` silently fails and scans the user's home dir).
