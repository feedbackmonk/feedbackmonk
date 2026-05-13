# pid-orphan-detector Oracle

## Synopsis

Project-state oracle (cleanup category) that sweeps `ltads/execution/worker-shell-*.pid` files referencing PIDs that are no longer alive. Liveness-based — **no TTL**, per DEC-54: a worker shell that's gone is gone, age is irrelevant. Come here for the liveness-probe contract, the pre-delete `_pid-summary.jsonl` audit-trail invariant, and the `--gc` / `--gc-cheap` invocation contract. Don't come here for `active-sessions.json` registry hygiene — that's the sibling `dispatchable-sessions` oracle (CSI-05); both share the same liveness probe via `claude-template/scripts/lib/pid-liveness.{sh,ps1}`.

## Identity

**Question answered**: Are there `worker-shell-*.pid` files referencing PIDs that are no longer alive?

**Category**: cleanup
**Kind**: `project-state`
**Spec**: `docs/specs/SPECIFICATION.md` § SWEEP (SWEEP-04, SWEEP-05, SWEEP-06)
**Design lineage**: `dispatchable-sessions` (CSI-05 PID-liveness probe pattern); `archive-retention` (`--gc` / `--gc-cheap` mode shape)

## Purpose

Caps unbounded growth of `ltads/execution/` from `.pid` files left by crashed worker shells. Trigger incident: SessionHelm 2026-05-07 audit found 10 orphan `worker-shell-*.pid` files; the same pattern reproduces in this very repo because no sweep mechanism existed.

The oracle is the explicit-operator leg of the **three-leg defense** (DEC-55):

1. **SWEEP-05 SessionEnd hook** — proactive close of the current session's `.pid` on natural exit.
2. **SWEEP-06 session-start `--gc-cheap`** — reactive cross-session sweep that catches `.pid` files from sessions that crashed without firing SessionEnd.
3. **SWEEP-04 `--gc` mode** (this oracle) — explicit operator-initiated sweep for ad-hoc cleanup.

All three legs share the same liveness probe (`pid_is_alive` / `Test-UldfPidAlive` in `claude-template/scripts/lib/pid-liveness.{sh,ps1}`).

## Files

| File | Purpose |
|---|---|
| `oracle.json` | Manifest with output schema, freshness contract, and `gcMode` semantics |
| `run.sh` | Bash entry point (Unix + Git Bash on Windows) |
| `run.ps1` | PowerShell entry point (Windows native) |
| `validate.sh` | Sandbox self-test (Unix) |
| `validate.ps1` | Sandbox self-test (Windows) |
| `test-fixtures/` | Static fixtures consumed by validate scripts |

## Modes

| Mode | Invocation | Purpose | Budget |
|---|---|---|---|
| (default) | `run.sh` / `run.ps1` | Briefing path: emit `{swept[], alive[], malformed[], briefing}` reflecting current state without action | ~120ms |
| `--gc-cheap` | `run.sh --gc-cheap` | Session-start hygiene sweep, defers if over budget; deletes orphans + writes JSONL audit | ~500ms |
| `--gc` | `run.sh --gc` | On-demand full sweep with summary output; deletes orphans + writes JSONL audit | unbounded |

**Key semantic**: in `--gc` and `--gc-cheap`, a `.pid` file is swept iff its referenced PID is **not alive**. There is no age threshold (DEC-54).

In default (briefing) mode, `.pid` files with dead PIDs appear in `swept[]` but are NOT deleted; `liveness_at_sweep` reflects current liveness without action. The mode is read-only.

## Frozen Output Schema

```json
{
  "swept": [
    {
      "pid_file": "ltads/execution/worker-shell-20260315-123456-789.pid",
      "referenced_pid": 12345,
      "liveness_at_sweep": false,
      "mtime": "2026-05-01T10:00:00Z"
    }
  ],
  "alive": [
    {"pid_file": "ltads/execution/worker-shell-20260508-100000-001.pid", "referenced_pid": 67890}
  ],
  "malformed": ["ltads/execution/corrupt.pid"],
  "briefing": "[pid-orphans] N stale worker-shell PIDs, run /0-uldf-oracle pid-orphan-detector --gc to clean"
}
```

| Field | Type | Meaning |
|---|---|---|
| `swept` | array | Entries whose PID is not alive. In `--gc`/`--gc-cheap` modes the underlying file has been deleted; in default (briefing) mode the file still exists. |
| `swept[].pid_file` | string | Repo-relative path to the `.pid` file. |
| `swept[].referenced_pid` | integer | The PID number read from the file content. |
| `swept[].liveness_at_sweep` | boolean | Always `false` for entries in `swept[]` (alive PIDs are never swept). |
| `swept[].mtime` | string | ISO-8601 UTC modification time of the `.pid` file at sweep evaluation time. |
| `alive` | array | Entries whose PID is currently alive (preserved). |
| `alive[].pid_file` | string | Repo-relative path. |
| `alive[].referenced_pid` | integer | The PID number read from the file. |
| `malformed` | array | Repo-relative paths to `.pid` files whose content is not a positive integer. **In `--gc` modes these are NOT deleted** — failure-open per Probandurgy (operator must triage manually). |
| `briefing` | string | One-line ORACLE BRIEFING summary. **Empty string when `swept[]` is empty** — session-start hook uses this empty-string contract to suppress the line. |

**Schema is frozen at author-time.** Programmatic consumers (session-start hook, smoke harnesses, downstream tools) read the array shapes directly. Field additions in future arcs append without removing or renaming existing keys.

## Sweep Criteria

A `worker-shell-*.pid` file is swept (in `--gc` / `--gc-cheap`) iff **all** hold:

1. The file is under `ltads/execution/` and its basename matches `worker-shell-*.pid` OR is the legacy single `worker-shell.pid`.
2. The file content reads as a positive integer (PID).
3. The liveness probe returns "not alive" for that PID.

Malformed `.pid` files (non-integer content) are NEVER swept — they surface in `malformed[]` for operator triage. Files with alive PIDs are NEVER swept regardless of age.

## Pre-Delete Audit Trail

Before deleting a `.pid` file, the oracle appends one JSON line per swept entry to `ltads/execution/_pid-summary.jsonl`:

```json
{"pid_file":"ltads/execution/worker-shell-20260315-123456-789.pid","referenced_pid":12345,"liveness_at_sweep":false,"mtime":"2026-05-01T10:00:00Z","sweptAt":"2026-05-08T06:00:00Z"}
```

**Invariant (SWEEP-08)**: summary write must succeed before delete. If the summary write fails (verified by re-reading the last line), the file is preserved and a warning is logged. Generalizes RETENTION-05's `_summary.jsonl` invariant.

`_pid-summary.jsonl` is gitignored but persists locally for forensic recall (~1KB per entry, append-only, bounded forever).

## No KEEP-Pin

Unlike `archive-retention` and `handoff-retention`, this oracle has **no KEEP-pin mechanism**. A `.pid` file is a process pointer, not a historical record — once the PID is dead the artifact is unambiguously waste, with no audit value to retain past liveness. KEEP-pin would be cargo-cult symmetry.

## Failure Modes (Graceful Absence)

| Condition | Behavior |
|---|---|
| `ltads/execution/` does not exist | Empty briefing JSON (`swept:[], alive:[], malformed:[], briefing:""`); `--gc` reports zero |
| No `.pid` files in dir | Same as above |
| Summary write fails | File is NOT deleted; warning to stderr |
| Delete fails after summary write | Summary entry persists; warning to stderr (rare; no data loss) |
| `--gc-cheap` exceeds 500ms budget mid-loop | Sweep aborts cleanly; defers to next session-start |
| Malformed `.pid` content | Listed in `malformed[]`; never deleted |
| Liveness probe fails (rare; e.g., powershell.exe missing on Windows) | Treated as "alive" (failure-closed): file preserved |

## Surfaces (where the sweep fires)

1. **SessionEnd hook (SWEEP-05)** — proactive close on natural exit; deletes the current session's matching `.pid` file. See `claude-template/hooks/session-end.{sh,ps1}`.
2. **Session-start hook (SWEEP-06)** — `--gc-cheap` runs alongside CSI-05's hygiene sweep. See `claude-template/hooks/session-start.{sh,ps1}`.
3. **`/0-uldf-oracle pid-orphan-detector --gc`** — explicit on-demand full sweep.

All three share the same liveness probe via `claude-template/scripts/lib/pid-liveness.{sh,ps1}`.

## Testing

```bash
bash .claude/oracles/pid-orphan-detector/validate.sh   # Unix / Git Bash
pwsh .claude/oracles/pid-orphan-detector/validate.ps1  # Windows PowerShell
```

Validates:

- T1: Default mode lists `.pid` files in `swept[]`/`alive[]`/`malformed[]` partitions without deleting.
- T2: `--gc` deletes only dead-PID files; alive PIDs preserved.
- T3: Malformed `.pid` content is reported in `malformed[]` and NEVER deleted.
- T4: `--gc` is idempotent (second run sweeps zero).
- T5: `_pid-summary.jsonl` receives one JSON line per swept file BEFORE delete.
- T6: `--gc-cheap` is silent on success and performs the sweep.
- T7: Empty `ltads/execution/` produces empty `briefing` field (gracefully absent).

Smoke harness at `claude-template/scripts/hygiene-tests/sweep-pid-orphan-detector-smoke.{sh,ps1}` covers oracle cases plus SessionEnd hook integration across all 6 SessionEnd matchers and session-start `--gc-cheap` integration.

## Constraints

- Hardcodes `ltads/execution/` path. Refuses to operate outside this prefix.
- Pattern `worker-shell-*.pid` is required; `shell.pid` files under `.claude/collaboration/<session>/workers/<AGENT-ID>/` (PODS shell.pid) are out of scope (different lifecycle: cleaned by `close-pods-sessions` and `kill-worker`).
- Liveness probe is the only sweep criterion. **No TTL**: rejected per DEC-54.

## Decision Log

- **Liveness, not TTL** (DEC-54): a `.pid` file's value is binary — alive or not. Adding an mtime cutoff would over-retain orphans during long-paused arcs and under-retain just-spawned workers whose registration outlived a brief crash.
- **No KEEP-pin**: process pointers carry no audit value past liveness. Symmetry with `archive-retention` would be cargo-cult.
- **Pre-delete `_pid-summary.jsonl` is mandatory** (SWEEP-08): Probandurgy invariant — never lose audit data without recovery surface. Summary write failure halts the delete.
- **Three-leg defense** (DEC-55): SessionEnd hook + session-start `--gc-cheap` + operator `--gc` mirror CSI Phase 1.6's pattern (CSI-12/13/14). Each leg covers a failure mode the others can't.
- **Liveness helper extracted** to `scripts/lib/pid-liveness.{sh,ps1}` rather than duplicated three times. `dispatchable-sessions` keeps its inline probe — no migration this arc; flagged for a future arc if a fourth caller appears.

## Cross-References

- Manifest: `oracle.json`
- Spec: `docs/specs/SPECIFICATION.md` § SWEEP (SWEEP-04..06, SWEEP-08)
- Decisions: `docs/specs/DECISIONS.md` DEC-54 (liveness, no TTL), DEC-55 (three-leg defense)
- Discovery: `docs/specs/DISCOVERIES.md` DISC-HYGIENE-03
- Liveness probe lib: `claude-template/scripts/lib/pid-liveness.{sh,ps1}`
- Sibling oracle: `claude-template/oracles/dispatchable-sessions/` (registry hygiene; same probe pattern, different substrate)
- Sibling oracle: `claude-template/oracles/archive-retention/` (`--gc` / `--gc-cheap` mode shape; KEEP-pin substrate)
- Probandurgy: `FOUNDATIONS/PROBANDURGY_MECHANISMS.md` audit-trail subsection (third instance after RETENTION-05 and handoff-retention)
