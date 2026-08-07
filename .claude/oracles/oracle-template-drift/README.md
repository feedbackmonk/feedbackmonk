# oracle-template-drift Oracle

> **Synopsis** — Project-state oracle that flags when an oracle installed in this project's `.claude/oracles/` differs from the current framework baseline (`~/.claude/oracles/`). Emits an `[oracle-template-drift]` line in the session-start ORACLE BRIEFING when drift is detected, nudging the user to run `/0-uldf-migrate-oracles`. Empty briefing on no-drift (gracefully absent). The installed-oracle sibling of `gitignore-template-drift`.

## Purpose & Responsibilities

`/0-uldf-setup-project` installs the starter oracle pack **add-only** (project customizations win — it never overwrites an oracle that already exists). That is the right default, but it means a starter-pack bug fix never reaches projects that were set up before the fix. This oracle is the recurring sentinel that catches that propagation gap: at every session start it compares each installed oracle against the framework baseline and surfaces any that have drifted, so the user knows to refresh.

Companion to `/0-uldf-migrate-oracles` (REFRESH-02): the migrate command is the one-shot interactive refresh; this oracle is the deterministic detector that tells you when to run it. Together they close the framework→project propagation gap for the oracle artifact class (pending follow-up #11, `docs/pending/project-artifact-refresh-mechanism.md`).

The oracle does **not**:

- Mutate state (read-only — `kind: "project-state"`; no Verification Oracle execution-state semantics).
- Compare `README.md` or `test-fixtures/` (documentation / test data — comparing them would raise drift on harmless edits). Only the **5 functional files** are tracked.
- Flag oracles that exist only in the project (no baseline counterpart — these are project-authored, nothing to drift from).
- Flag oracles carrying a `.local-customized` marker (intentional divergence — opt-out).

## File index

| File | Role |
|---|---|
| `oracle.json` | Manifest (frozen output schema, freshness triggers, baseline/project resolution, tracked-file list, opt-out marker). |
| `run.sh` | Bash entry point — emits the FROZEN JSON output. Honors `CLAUDE_ORACLE_BASELINE_DIR` / `CLAUDE_ORACLE_PROJECT_DIR` env overrides for fixture testing. |
| `run.ps1` | PowerShell parallel — same output schema, same env-override contract. |
| `validate.sh` | Self-test harness (Bash) — builds synthetic baseline/project trees in a tmpdir and asserts 7 drift classifications. |
| `validate.ps1` | Self-test harness (PowerShell) — same 7 cases. |

## Public API & Usage

### Output schema (FROZEN base + PACK-02 additive extension)

Programmatic consumers (session-start hook briefing assembly, `/0-uldf-migrate-oracles` Phase 1) read these fields verbatim. Base schema locked at 2026-06-18T21:00Z; the `missing_starter`/`missing_count` pair was added **additively** 2026-07-02 (PACK-02 — base fields and `drifted` semantics unchanged).

```json
{
  "drifted": false,
  "drifted_oracles": [],
  "drift_count": 0,
  "compared_count": 12,
  "missing_starter": [],
  "missing_count": 0,
  "briefing": ""
}
```

| Field | Type | Semantics |
|---|---|---|
| `drifted` | bool | `true` when one or more compared oracles differ from baseline (content drift of INSTALLED oracles only — missing-starter gaps do not set it). |
| `drifted_oracles` | `{name, files[]}[]` | Per drifted oracle: its name and the tracked files that differ from (or are missing relative to) baseline. |
| `drift_count` | int | Number of drifted oracles (== `drifted_oracles.length`). |
| `compared_count` | int | Oracles present in BOTH project and baseline, not pinned (the comparison population). |
| `missing_starter` | string[] | PACK-02: `PACK_MANIFEST.json` `packs.starter` entries with no installed project dir. Empty when the baseline has no manifest (pre-PACK-01) — that means *detection unavailable*, not *nothing missing*. |
| `missing_count` | int | Length of `missing_starter`. |
| `briefing` | string | `""` when no drift AND no missing-starter gaps (graceful absent — hook suppresses the line). Otherwise a one-line `/0-uldf-migrate-oracles` nudge naming the drift count and/or missing count. |

### Tracked files (the comparison set)

`oracle.json`, `run.sh`, `run.ps1`, `validate.sh`, `validate.ps1`. A file is compared only if the **baseline** ships it for that oracle (an oracle with no `validate.*` simply has fewer compared files). Comparison is **CR-normalized** (strip `\r` before hashing) so CRLF↔LF differences from cross-platform copies never raise false drift.

### Invocation

```bash
# Unix (autodiscovers baseline at $HOME/.claude/oracles or walks up for claude-template/oracles) <!-- path-ok: framework-dev walk-up fallback -->
bash .claude/oracles/oracle-template-drift/run.sh

# Windows
powershell -NoProfile -File .claude/oracles/oracle-template-drift/run.ps1
```

### Test override env vars

| Var | Effect |
|---|---|
| `CLAUDE_ORACLE_BASELINE_DIR` | Overrides the baseline oracle directory. Non-existent path → graceful-absent branch. |
| `CLAUDE_ORACLE_PROJECT_DIR` | Overrides the project oracle directory. Defaults to `./.claude/oracles`. |

### Customization opt-out

A project oracle directory containing a `.local-customized` file is skipped by BOTH this oracle (never flagged) and `/0-uldf-migrate-oracles` (never re-copied). It is the escape hatch for an intentionally-divergent project oracle. `/0-uldf-migrate-oracles`'s "pin" action writes this marker.

## Constraints & Business Rules

- **Compute budget**: declared `expected_runtime_ms: 3000` in `oracle.json` (pessimistic — hashing up to 5 files per installed oracle, each a fork under Git Bash). The session-start briefing fan-out (`scripts/lib/briefing-oracles.{sh,ps1}`) admits it (every-session, fast lane, within the 15 000 ms budget) and gives it a 9 000 ms timeout (3×).
- **Match semantics**: CR-normalized content hash equality. No fuzzy matching. `sha256sum` → `shasum -a 256` → `cksum` fallback chain (Bash); `SHA256` over CR-stripped UTF-8 bytes (PowerShell).
- **Empty-briefing convention**: when `drifted=false`, `briefing=""` so the hook suppresses the line (gracefully-absent convention). The hook MUST NOT emit a "no drift" fallback.
- **Graceful absent**: when no baseline oracle dir is found, OR the project oracle dir is missing, output is the empty result and exit 0 — never warns. In the ULDF framework repo itself `.claude/oracles/` is empty, so the oracle is silent (correct — the framework edits the *source* in `claude-template/oracles/`, nothing is "installed stale" here). <!-- path-ok: framework repo dogfood behavior -->
- **One-way drift**: baseline → project. Files the project has that baseline lacks are not flagged (project additions are allowed).

## Relationships & Dependencies

| Direction | Counterpart | Relationship |
|---|---|---|
| Consumed by | `~/.claude/hooks/session-start.{sh,ps1}` via `scripts/lib/briefing-oracles.{sh,ps1}` | Auto-admitted into the briefing fan-out (`typical_sessions_using: "every"` + `briefing` field); the hook reads `briefing` and emits the `[oracle-template-drift]` line. No hook edit required. |
| Reads from | `~/.claude/oracles/` (deployed) or `claude-template/oracles/` (walk-up, framework-dev) | The framework baseline oracle pack. | <!-- path-ok: source baseline vs deployed -->
| References | `/0-uldf-migrate-oracles` (REFRESH-02) | Briefing-line text instructs the user to run this command. The oracle does not invoke it; the user does. |
| Sibling oracle | `gitignore-template-drift` (HYGIENE-03) | Same propagation-gap pattern for a different artifact class (`.gitignore` patterns vs. installed oracle bodies). Output-shape lineage: `drifted` boolean + pre-formatted `briefing` string in a single emit. |

## Decision log

- **Decision home**: DEC-90 (drift = content comparison; refresh via re-copy with pin/skip; setup-project stays add-only). Spec REFRESH-01 (this oracle) / REFRESH-02 (`/0-uldf-migrate-oracles`).

- **Why content comparison, not a `version` field?** A stale project carries the *old* version string and only updates it when the file is re-copied — the very act we are trying to trigger. A version pin can therefore never detect "you have V1, baseline is V2" before the fix has already landed (circular). Content hashing catches any divergence with zero discipline burden. (`mtime` is destroyed by copy/sync — also rejected.) This matches the pending-doc sketch verbatim.

- **Why exclude `README.md` and `test-fixtures/`?** They are documentation and test data, not the runtime program an agent executes. Including them would raise drift on harmless doc edits and create perpetual briefing noise. The 5 functional files are the actual "is the agent running a stale program?" surface.

- **Why CR-normalize before hashing?** `/0-uldf-setup-project` copies oracle files into projects; a copy made on Windows can acquire CRLF while the LF baseline stays LF (or vice versa). Without normalization every cross-platform-copied oracle would falsely drift. Stripping `\r` makes the comparison about *content*, not line-ending accidents.

- **Why no Verification Oracle category (`kind: "verification"`)?** This answers "what is in this project?" (does the installed pack match baseline) — project-state — not "did the last action break something?" It runs every session for orientation, not reflexively in the inner loop. Same placement as its sibling `gitignore-template-drift`.
