# archive-retention Oracle

## Synopsis

Project-state oracle (cleanup category) that lists archived PODS sessions under `.claude/collaboration/archived/` and sweeps `collab-*` dirs older than the configured threshold (default 90 days). Come here for the threshold-config protocol, the `KEEP`-pin escape hatch for permanent retention, the pre-delete `_summary.jsonl` audit-trail invariant, and the `--gc` / `--gc-cheap` invocation contract. Don't come here for live registry hygiene of `active-sessions.json` — that's the sibling CSI-05 mechanism in `dispatchable-sessions/` (different substrate, different lifecycle, shared skeleton).

## Identity

**Question answered**: Which archived PODS sessions exist, and which are old enough to sweep under the retention threshold?

**Category**: cleanup
**Kind**: `project-state`
**Spec**: `docs/specs/SPECIFICATION.md` § Archive Retention (RETENTION-01..06)
**Design lineage**: CSI-05 registry hygiene oracle (`claude-template/oracles/dispatchable-sessions/`)

## Purpose

Caps unbounded growth of `.claude/collaboration/archived/` by sweeping archived PODS sessions older than a configurable threshold (default: 90 days). Mirrors CSI-05's `--gc` / `--gc-cheap` invocation pattern but operates on filesystem dirs instead of registry entries.

Without this oracle, every converged PODS session leaks an archive dir indefinitely. After ~50 archived sessions per year, the directory accumulates without bound. Eventually a maintainer `rm -rf`'s the whole tree and loses audit data they would have wanted.

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
| (default) | `run.sh` / `run.ps1` | List archived dirs with metadata + sweepability flags | ~120ms |
| `--gc-cheap` | `run.sh --gc-cheap` | Session-start hygiene sweep, defers if over budget | ~100ms |
| `--gc` | `run.sh --gc` | On-demand full sweep with summary output | unbounded |

## Sweep Criteria

A `collab-*` dir is swept if **all** hold:

1. Basename matches `^collab-\d{8}-\d{6}$` (defensive — never sweeps non-conforming dirs).
2. **No `KEEP` file present** at `<dir>/KEEP` (operator-controlled pin).
3. `createdAt` parsed from basename is older than `now - threshold`.

Unparsable dirs and KEEP-pinned dirs are NEVER swept regardless of age.

## Pin Mechanism

To exempt a session from sweep:

```bash
touch .claude/collaboration/archived/collab-XXXX/KEEP
```

By convention, write a one-line rationale into the file so future readers understand the pin:

```bash
echo "Keep because: forensic reference for the 2026-04-09 ConPTY incident" > \
    .claude/collaboration/archived/collab-20260409-XXXX/KEEP
```

The oracle reads file existence, not content. Empty `KEEP` files work fine.

## Pre-Delete Audit Trail

Before deleting a dir, the oracle appends one JSON line per swept session to `.claude/collaboration/archived/_summary.jsonl`:

```json
{"sessionId":"collab-20260225-004014","sweptAt":"2026-04-30T12:00:00Z","createdAt":"2026-02-25T00:40:14Z","ageDays":64,"sizeBytes":119808,"workerCount":3,"taskCount":7,"criticVerdict":"PASS","hasOverrideVeto":false,"guideHeadline":"PODS coordinated work on …"}
```

**Invariant**: summary write must succeed before delete. If the summary write fails (verified by re-reading the last line), the dir is preserved and a warning is logged.

`_summary.jsonl` is gitignored (under `.claude/collaboration/`) but persists locally for forensic recall. ~1KB per entry, bounded forever.

## Threshold Configuration

Configurable via `.claude/config.json`:

```json
{
    "archiveRetention": {
        "threshold": 90
    }
}
```

Or as ISO-8601 duration:

```json
{
    "archiveRetention": {
        "threshold": "P90D"
    }
}
```

Default is 90 days if config absent or malformed. Rationale (DEC-30 in DECISIONS.md):

- 2026-05-18 Phase 2 dogfooding review reads archived data → minimum ~21 days
- Quarterly retrospectives → 90 days
- Annual audits → covered by KEEP-pin
- Default 90 days bounds growth at ~12-15 archived dirs at current rates (~3MB)

## Atomic Semantics

Per-dir delete is sequential. `_summary.jsonl` writes use POSIX-atomic append (each entry < PIPE_BUF / 4KB, atomic across concurrent appenders). No directory-level lock — concurrent sweeps from different sessions are race-tolerant:

- Each sweep iterates dirs that exist at its scan time.
- If two sweeps target the same dir, the second's `rm -rf` is idempotent (no-op).
- `_summary.jsonl` may receive duplicate entries in the rare double-sweep case. Acceptable: audit trail favors over-recording.

## Failure Modes (Graceful Absence)

| Condition | Behavior |
|---|---|
| `.claude/collaboration/archived/` does not exist | Empty briefing JSON; `--gc` reports zero with `note:"no archived dir"` |
| Summary write fails | Dir is NOT deleted; warning to stderr |
| Delete fails after summary write | Summary entry persists; warning to stderr (rare; no data loss) |
| `--gc-cheap` exceeds 100ms budget mid-loop | Sweep aborts cleanly; defers to next session-start |
| Unparseable basename | Skipped (failure-open) |

## Surfaces (where the sweep fires)

1. **Session-start hook** — `--gc-cheap` runs alongside CSI-05's hygiene sweep.
2. **`/0-uldf-oracle archive-retention --gc`** — explicit on-demand full sweep.
3. **`/0-uldf-pods-converge` Phase 7 (planned)** — opportunistic full sweep right after a new archive lands. Filed as Pending Follow-Up; deferred to a future commit.

## Testing

```bash
bash .claude/oracles/archive-retention/validate.sh   # Unix
pwsh .claude/oracles/archive-retention/validate.ps1  # Windows
```

Validates:

- T1: Sweep deletes dirs older than threshold (no KEEP file).
- T2: Sweep does NOT delete dirs younger than threshold.
- T3: KEEP file exempts dir from sweep regardless of age.
- T4: Sweep is idempotent (re-running on post-sweep dir sweeps zero).
- T5: `--gc` emits JSON summary `{swept, before, after, threshold, thresholdSource, summarized}`.
- T6: `.claude/config.json` `archiveRetention.threshold` is honored.
- T7: `--gc-cheap` is silent on success and performs the sweep.
- T8: `_summary.jsonl` receives one JSON line per swept dir BEFORE delete.

## Constraints

- Oracle hardcodes `.claude/collaboration/archived/` path. Refuses to operate outside this prefix.
- Basename pattern `collab-YYYYMMDD-HHMMSS` is required (PODS naming convention).
- Threshold cannot be lower than 1 day in practice (timestamp precision is per-second; same-day sessions cannot reliably be distinguished by age).

## Decision Log

- **Threshold default = 90 days**, not 24h (CSI-05's value): different substrate, different lifecycle. Long-form forensic data has a different timescale than live coordination state. See DEC-30.
- **Pure age + KEEP-pin**, not size cap or count cap: simplest sweep semantic; safety valve is structural not heuristic. See DEC-30.
- **Pre-delete `_summary.jsonl` is mandatory**: Probandurgy invariant — never lose audit data. Summary write failure halts the delete.
- **No directory-level lock**: race-tolerant by idempotent delete; audit-tolerant by duplicate-OK summary appends.
- **Stand-alone in spec**, not under § CSI: different scope (filesystem dirs vs. registry), different consumers (forensic readers vs. dispatch). Cross-references CSI-05 as design lineage.

## Cross-References

- Manifest: `oracle.json`
- Spec: `docs/specs/SPECIFICATION.md` § Archive Retention
- Design lineage: `claude-template/oracles/dispatchable-sessions/` (CSI-05)
- Foundation: `FOUNDATIONS/ORACULURGY_DESIGN.md` Part 11 (Verification Oracles, sibling category) and § 2.12 of `PRINCIPLES_OF_LLM_AGENT_ORCHESTRATION.md`
- Probandurgy positioning: this is NOT a Probandurgy mechanism per se — it's a project-state oracle that protects the substrate (archive dirs) which downstream Probandurgy mechanisms (PODS Critic dogfood reviews) depend on. CSI-05 is its closest sibling.
