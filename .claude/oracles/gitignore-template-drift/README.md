# gitignore-template-drift Oracle

> **Synopsis** — Project-state oracle that flags when a project's `.gitignore` lacks any framework-managed patterns from the current `claude-template/.gitignore` baseline. Emits a `[gitignore-template-drift]` line in the session-start ORACLE BRIEFING when drift is detected, nudging the user to run `/0-uldf-migrate-hygiene`. Empty briefing on no-drift (gracefully absent).

## Purpose & Responsibilities

Surface baseline drift cheaply (≤50 ms) at session start so legacy projects — those that ran `/0-uldf-setup-project` before CSI Phase 1+1.5+1.6+HYGIENE-01 — get nudged onto the post-CSI-1.6 gitignore baseline. Companion to `/0-uldf-migrate-hygiene` (HYGIENE-02): the migrate command is a one-shot manual operation; this oracle is the recurring sentinel that catches future drift whenever `claude-template/.gitignore` adds new patterns.

The oracle does **not**:

- Mutate state (read-only — `kind: "project-state"`, no Verification Oracle execution-state semantics).
- Check user-customized sections of project `.gitignore` (only patterns under the `# Claude Code (session artifacts — never commit)` header are tracked).
- Fuzzy-match patterns (line-trimmed exact match only — preserves baseline semantics verbatim).

## File index

| File | Role |
|---|---|
| `oracle.json` | Manifest (frozen output schema, freshness triggers, baseline/project resolution rules). |
| `run.sh` | Bash entry point — emits the FROZEN JSON output. Honors `CLAUDE_GITIGNORE_BASELINE` / `CLAUDE_GITIGNORE_PROJECT` env overrides for fixture testing. |
| `run.ps1` | PowerShell parallel — same output schema, same env-override contract. |
| `validate.sh` | Self-test harness (Bash) — runs run.sh against the 6 fixtures and asserts expected drift classification. |
| `validate.ps1` | Self-test harness (PowerShell) — same 6 cases. |
| `test-fixtures/` | 6 baseline+project pairs covering the meaningful Cartesian product of (drifted vs no-drift × baseline-present vs absent × project-present vs absent). See `test-fixtures/README.md`. |

## Public API & Usage

### Output schema (FROZEN)

Programmatic consumers (session-start hook briefing assembly) read these fields verbatim. Schema is locked at 2026-05-07T07:30Z; see `channels/messages.md [ARC1-W3]` of `collab-20260507-070154` for the freeze rationale.

```json
{
  "drifted": false,
  "missing_patterns": [],
  "baseline_patterns": 13,
  "project_patterns": 0,
  "briefing": ""
}
```

| Field | Type | Semantics |
|---|---|---|
| `drifted` | bool | `true` when one or more baseline patterns are missing from project `.gitignore`. |
| `missing_patterns` | string[] | Verbatim baseline pattern lines NOT present in project `.gitignore` (preserves order from baseline). |
| `baseline_patterns` | int | Total framework-managed patterns extracted from baseline (after the section header). |
| `project_patterns` | int | Total non-comment, non-blank lines in project `.gitignore`. `0` when project file is absent. |
| `briefing` | string | `""` when `drifted=false` OR baseline missing (graceful absent — hook suppresses the line). When drift detected: `"gitignore-template-drift: N framework patterns missing — run /0-uldf-migrate-hygiene to update"`. |

### Invocation

```bash
# Unix (autodiscovers baseline at $HOME/.claude/.gitignore or walks up for claude-template/.gitignore)
bash .claude/oracles/gitignore-template-drift/run.sh

# Windows
powershell -NoProfile -File .claude/oracles/gitignore-template-drift/run.ps1
```

### Test override env vars

| Var | Effect |
|---|---|
| `CLAUDE_GITIGNORE_BASELINE` | Overrides baseline file path. If set to a non-existent path, oracle hits graceful-absent branch. |
| `CLAUDE_GITIGNORE_PROJECT` | Overrides project `.gitignore` path. Defaults to `./.gitignore`. |

## Constraints & Business Rules

- **Compute budget**: ≤ 50 ms (`compute_cost_ms` declared in `oracle.json`). Implementation is two `awk`/`Get-Content` passes plus one `grep -F -x` per missing-check candidate; well within budget on typical project sizes.
- **Section discipline**: Only patterns AFTER the literal header line `# Claude Code (session artifacts — never commit)` are tracked. Sub-headers (`# CSI registry (mutates every session-start)`, etc.) are skipped as comments — only their pattern lines are extracted.
- **Match semantics**: Line-trimmed exact match (UTF-8 byte equality). No regex, no glob expansion, no fuzzy matching. The em-dash in the section header is U+2014 (UTF-8 `0xE2 0x80 0x94`); files must be UTF-8 (no BOM required, but no other encodings).
- **Empty-briefing convention**: When `drifted=false`, `briefing=""` so the session-start hook suppresses the line (per ULDF gracefully-absent briefing convention). The hook MUST NOT emit a "No drift detected" fallback.
- **Baseline graceful absent**: When neither `$HOME/.claude/.gitignore` nor a walk-up `claude-template/.gitignore` is found, output is `{drifted:false, missing_patterns:[], baseline_patterns:0, project_patterns:0, briefing:""}` — never warns about a missing baseline (silent on success).
- **Extras not flagged**: Patterns in project `.gitignore` that are NOT in baseline are never reported. Only baseline → project direction is checked (one-way drift detection).

## Relationships & Dependencies

| Direction | Counterpart | Relationship |
|---|---|---|
| Consumed by | `claude-template/hooks/session-start.{sh,ps1}` | Hook reads `briefing` field and emits it as a `[gitignore-template-drift]` line in the ORACLE BRIEFING (when non-empty). |
| Reads from | `claude-template/.gitignore` (or `~/.claude/.gitignore` deployed) | The framework baseline pattern source — owned by HYGIENE-01 (CLAUDE-A). |
| References | `/0-uldf-migrate-hygiene` (HYGIENE-02) | Briefing-line text instructs the user to run this command. The oracle does not invoke it; the user does. |
| Sibling oracle | `dispatchable-sessions` | Output-shape lineage: structured `count`-or-`drifted` field plus a pre-formatted `briefing` string in a single emit (avoids hook special-casing). |

## Decision log

- **Why expose env-var overrides for baseline/project paths?** Required for fixture-driven validation. The 6 test cases need to point at synthetic input files inside `test-fixtures/`. The alternative — copying the oracle into a sandbox + `cd` per case — was the pattern used by `dispatchable-sessions/validate.sh` Phase 2, and proved fragile on Git Bash (path-mangling of `--` arguments to `jq`, see DISC-CSI cross-project portability gotchas). Env-var injection is portable, doesn't touch the real registry, and keeps the script pure-bash on Unix and pure-PowerShell on Windows.

- **Why the walk-up fallback for the framework-dev case?** The deployed setting is `~/.claude/.gitignore`, but the framework's own dogfood run (running inside the ULDF repo before sync) needs the oracle to find `claude-template/.gitignore` directly — without that, the oracle would be permanently in "no baseline" mode while authoring/testing the framework itself, defeating the purpose of paired-shell smoke tests during HYGIENE-03 development. Bounded depth (6 ancestors) keeps the cost trivial.

- **Why exact-match (not glob-match)?** Drift detection is about "is this exact framework-managed line in the project's `.gitignore`," not "does the project effectively cover the same paths via different patterns." Project-scope patterns (e.g., `**/.claude/**`) might cover the same files as the framework's specific `**/.claude/session-state/`, but the framework wants the exact pattern in the project so that future baseline updates don't silently overlap and create maintenance ambiguity. Spec acceptance pinned this: "preserves pattern text exactly; no fuzzy matching."

- **Why no Verification Oracle category (`kind: "verification"`)?** This is project-state — it answers "what is in this project?" not "did the last action break something?" The `kind: "project-state"` declaration in `oracle.json` aligns with the catalog placement under `workflow` (the same category as `dispatchable-sessions` — surfaces actionable session-start state).

- **Why include a non-framework prelude in fixture baseline files?** Verifies that the section parser correctly skips pre-header content. A naive implementation that started extraction at line 1 would falsely include `node_modules/` etc. as framework patterns; the prelude in fixtures forces the test to detect that bug.
