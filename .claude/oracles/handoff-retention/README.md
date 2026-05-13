# handoff-retention Oracle

## Synopsis

Project-state oracle (cleanup category) that lists handoff briefs in `.claude/handoff/handoff-*.md` and sweeps files older than the configured threshold (default 30 days, DEC-54). Come here for the threshold-config protocol, the sibling-`.KEEP` exemption (HANDOFF-01 substrate), the pre-delete `_summary.jsonl` audit-trail invariant (SWEEP-08), and the `--gc` / `--gc-cheap` invocation contract. Don't come here for `.claude/collaboration/archived/` cleanup — that's the sibling `archive-retention` oracle (different substrate, different default TTL, shared shape per DEC-52).

## Identity

**Question answered**: Which handoff briefs older than the configured TTL exist, and which are KEEP-pinned for permanent retention?

**Category**: cleanup
**Kind**: `project-state`
**Spec**: `docs/specs/SPECIFICATION.md` § SWEEP-01, § SWEEP-07, § SWEEP-08
**Design lineage**: `claude-template/oracles/archive-retention/` (RETENTION-01..06 substrate per DEC-52); HANDOFF-01 (sibling-`.KEEP` substrate consumed verbatim)
**Discoveries closed**: DISC-HYGIENE-03 (retention-without-sweep asymmetry)

## Purpose

Caps unbounded growth of `.claude/handoff/handoff-*.md` by sweeping briefs older than the configurable threshold (default: 30 days). Without this oracle, every `/0-uldf-proceed` HANDOFF authoring leaks a brief on disk indefinitely; the trigger audit (SessionHelm 2026-05-07: 50 handoffs / 1 KEEP'd; this repo: 14 handoffs / 1 KEEP'd) showed accumulation under the existing HYGIENE-01 gitignore baseline plus HANDOFF-01 KEEP-pin retention model — disk pressure without a sweep surface.

## Files

| File | Purpose |
|---|---|
| `oracle.json` | Manifest with output schema, freshness contract, and `gcMode` semantics |
| `run.sh` | Bash entry point (Unix + Git Bash on Windows) |
| `run.ps1` | PowerShell entry point (Windows native) |
| `validate.sh` | Sandbox self-test (Unix) |
| `validate.ps1` | Sandbox self-test (Windows) |
| `test-fixtures/` | Static fixture scenarios consumed by validate scripts |

## Modes

| Mode | Invocation | Purpose | Budget |
|---|---|---|---|
| (default) | `run.sh` / `run.ps1` | Full inventory JSON with `briefing` field | ~100ms |
| `--gc-cheap` | `run.sh --gc-cheap` | Silent no-op (read-only per SWEEP-01); symmetry with archive-retention wiring | ~100ms |
| `--gc` | `run.sh --gc` | Destructive sweep with `_summary.jsonl` audit; emits sweep summary JSON | unbounded |

## Frozen Output Schema

### Default mode (and `--gc` mode shares structure; `swept_at` differs)

```json
{
  "swept": [
    {
      "file": ".claude/handoff/handoff-20260101-120000-old-routing.md",
      "swept_at": null,
      "age_days": 35,
      "brief_first_line": "# Phase 2 landing-site migration"
    }
  ],
  "retained_keep_pinned": [
    ".claude/handoff/handoff-20260507-175000-skill-consol-converge.md"
  ],
  "retained_under_ttl": [
    {"file": ".claude/handoff/handoff-20260507-103845-arc2-critic-followup.md", "age_days": 1}
  ],
  "threshold_days": 30,
  "threshold_source": "default",
  "briefing": "[handoff-retention] 1 brief older than 30d, run /0-uldf-oracle handoff-retention --gc to sweep"
}
```

**Field semantics**:

- `swept[]` — in default mode lists **would-be-swept candidates** (`swept_at: null`). In `--gc` mode lists **actually-swept entries** (`swept_at: ISO-8601 UTC`). The shape is identical so consumers don't branch on mode.
- `retained_keep_pinned[]` — paths exempt because a sibling `<file>.KEEP` file exists.
- `retained_under_ttl[]` — paths younger than threshold (would not be swept).
- `threshold_days` — resolved TTL in days.
- `threshold_source` — `"default"` (no config) or `"config"` (from `.claude/config.json`).
- `briefing` — frozen-format briefing line. **Empty string suppresses the session-start line gracefully** (parallel to `stale-ltads-state` pattern).

**Briefing-line format (frozen)**:

```
[handoff-retention] N brief(s) older than 30d, run /0-uldf-oracle handoff-retention --gc to sweep
```

Empty string when `swept[]` (would-be-swept candidates) is empty.

### `--gc` mode summary (single-line JSON on stdout)

```json
{
  "swept": 3,
  "before": 14,
  "after": 11,
  "threshold": "P30D",
  "thresholdSource": "default",
  "summarized": 3,
  "sweptFiles": "handoff-20260101-120000.md,handoff-20260105-093000.md,handoff-20260110-141500.md"
}
```

`sweptFiles` omitted when zero swept. `--gc` does NOT emit the inventory JSON of the default mode — only the sweep summary.

## Sweep Criteria

A `handoff-*.md` file is swept if **all** hold:

1. Filename matches `^handoff-.+\.md$` (defensive — refuses non-conforming names).
2. **No sibling `<file>.KEEP` present** (HANDOFF-01 KEEP-pin substrate consumed verbatim).
3. `mtime` is older than `now - threshold`.

Files with companion `.KEEP` files are NEVER swept regardless of age.

## KEEP-Pin Substrate (HANDOFF-01)

Consumes the existing convention without any new mechanism. To exempt a brief from sweep:

```bash
touch .claude/handoff/handoff-XXX.md.KEEP
```

By convention, write a one-line rationale into the file:

```bash
echo "Keep because: load-bearing for ARIA migration plan; cross-reference from DEC-25" > \
    .claude/handoff/handoff-XXX.md.KEEP
```

The oracle reads file existence, not content. Empty `.KEEP` files work fine. `git add -f <handoff-file>` is the second leg of the KEEP-pin (force-tracking the brief past gitignore); the oracle's sweep behavior is independent of git tracking — only the sibling `.KEEP` file controls exemption.

## Pre-Delete Audit Trail (SWEEP-08 invariant)

Before deleting a brief, the oracle appends one JSON line per swept entry to `.claude/handoff/_summary.jsonl`:

```json
{"file":".claude/handoff/handoff-20260101-120000-old.md","swept_at":"2026-05-08T06:30:00Z","age_days":127,"brief_first_line":"# Phase 2 landing-site migration"}
```

**Invariant**: summary write must succeed before delete. If the summary write fails (verified by re-reading the last line), the file is preserved and a warning is logged.

`_summary.jsonl` is gitignored (added to `.gitignore` baseline as part of SWEEP-08) but persists locally for forensic recall. ~300 bytes per entry, append-only, bounded forever.

This generalizes the RETENTION-05 pattern (archive-retention's `_summary.jsonl`) into the SWEEP-08 cross-mechanism Probandurgy invariant: never delete without a recovery surface.

## Threshold Configuration

Configurable via `.claude/config.json`:

```json
{
    "handoffRetention": {
        "threshold": 30
    }
}
```

Or as ISO-8601 duration:

```json
{
    "handoffRetention": {
        "threshold": "P30D"
    }
}
```

Default is 30 days if config absent or malformed (DEC-54). Rationale:

- Handoffs are routing artifacts; value decays rapidly past a few weeks
- Typical handoff lifetime in this repo is single-session-to-handful-of-sessions
- 30 days is conservative (covers any realistic resumption window)
- KEEP-pin (HANDOFF-01) is the audit-value retention path, not TTL

## Atomic Semantics

Per-file delete is sequential. `_summary.jsonl` writes use POSIX-atomic append (each entry < PIPE_BUF / 4KB, atomic across concurrent appenders). No directory-level lock — concurrent sweeps from different sessions are race-tolerant:

- Each sweep iterates files that exist at its scan time.
- If two sweeps target the same file, the second's `rm` is idempotent (no-op).
- `_summary.jsonl` may receive duplicate entries in the rare double-sweep case. Acceptable: audit trail favors over-recording.

## Failure Modes (Graceful Absence)

| Condition | Behavior |
|---|---|
| `.claude/handoff/` does not exist | Empty briefing JSON (`swept:[]`, `briefing:""`); `--gc` reports zero with `note:"no handoff dir"` |
| Summary write fails | File is NOT deleted; warning to stderr |
| Delete fails after summary write | Summary entry persists; warning to stderr (rare; no data loss) |
| `.claude/config.json` malformed | Falls back to default 30 days; `threshold_source:"default"` |
| Filename without `.md` extension | Skipped (failure-open per Probandurgy) |

## Surfaces (where the sweep / detection fires)

1. **Session-start hook iteration loop** — default-mode JSON read; `briefing` field surfaced when drift detected.
2. **Session-start hook hygiene block** — `--gc-cheap` fired alongside `archive-retention --gc-cheap` and `dispatchable-sessions --gc-cheap`; silent on success per the symmetric wiring contract.
3. **`/0-uldf-oracle handoff-retention --gc`** — explicit operator-initiated sweep; the only destructive path.

The agent does NOT auto-invoke `--gc` from the briefing line. Handoff content may have audit value the agent shouldn't auto-delete; the explicit operator command IS the safety valve (per SWEEP-07 design rationale).

## Testing

```bash
bash .claude/oracles/handoff-retention/validate.sh   # Unix
pwsh .claude/oracles/handoff-retention/validate.ps1  # Windows
```

Validates (≥5 cases each, per SWEEP-01 acceptance):

- T1: `--gc` deletes briefs older than threshold (no `.KEEP` file).
- T2: `--gc` does NOT delete briefs younger than threshold.
- T3: Sibling `<file>.KEEP` exempts brief from sweep regardless of age.
- T4: `--gc` is idempotent (re-run on post-sweep dir sweeps zero).
- T5: `--gc` emits JSON summary `{swept, before, after, threshold, thresholdSource, summarized}`.
- T6: `.claude/config.json` `handoffRetention.threshold` is honored (both numeric and `PnD` form).
- T7: `--gc-cheap` is silent on success; default mode emits inventory JSON.
- T8: `_summary.jsonl` receives one valid JSON line per swept brief BEFORE delete (SWEEP-08 invariant).
- T9: Malformed config falls back to default 30 days with `threshold_source:"default"`.

## Constraints

- Oracle hardcodes `.claude/handoff/` path. Refuses to operate outside this prefix.
- Filename pattern `handoff-*.md` is required (filenames without `handoff-` prefix or without `.md` extension are ignored).
- KEEP-pin sibling files (`<file>.KEEP`) are themselves NOT swept by this oracle (the `.KEEP` filename does not match the `handoff-*.md` glob).
- Threshold cannot be lower than 1 day in practice (mtime precision is per-second; same-day briefs cannot reliably be distinguished by age).

## Decision Log

- **30-day default TTL** (DEC-54), not archive-retention's 90 days: handoffs are routing artifacts with faster value decay; uniform 90d would over-retain by ~3x.
- **mtime-based age**, not filename-timestamp parsing: mtime reflects last meaningful edit; filename timestamps may be stale if a brief was authored, then later edited or amended. mtime is the load-bearing signal.
- **Sibling `.KEEP` substrate consumed verbatim from HANDOFF-01**: zero new mechanism — KEEP-pin is the established escape hatch for default-gitignored briefs (HYGIENE-01 baseline + HANDOFF-01 retention).
- **Pre-delete `_summary.jsonl` is mandatory** (SWEEP-08): Probandurgy invariant — never lose audit data without a recovery surface. Summary write failure halts the delete.
- **`--gc-cheap` is silent / read-only** (per SWEEP-01 spec): handoff content may have unrecognized audit value; no autonomous sweep at session-start. The drift surfaces via the iteration-loop briefing line; only explicit `/0-uldf-oracle handoff-retention --gc` performs the delete.
- **No directory-level lock**: race-tolerant by idempotent delete; audit-tolerant by duplicate-OK summary appends.
- **Independent of archive-retention** (DEC-52, rule-of-three): the substrate similarity is acknowledged but extraction is deferred until a 4th application appears.

## Cross-References

- Manifest: `oracle.json`
- Spec: `docs/specs/SPECIFICATION.md` § SWEEP (SWEEP-01, SWEEP-07, SWEEP-08)
- Decisions: DEC-52 (three-independent-oracles substrate model), DEC-54 (30d surface-specific TTL), DEC-55 (three-leg defense — handoff-retention is the single-leg / no-sessionEnd case)
- Discovery: DISC-HYGIENE-03 (retention-without-sweep asymmetry)
- Substrate precedent: `claude-template/oracles/archive-retention/` (RETENTION-01..06)
- KEEP-pin substrate: `CLAUDE.md` § "Handoff Brief Lifecycle — KEEP-Pin Convention"; HANDOFF-01 + HANDOFF-02 in spec
- Foundation: `FOUNDATIONS/ORACULURGY_DESIGN.md` (oracle authoring discipline); `FOUNDATIONS/PROBANDURGY_MECHANISMS.md` audit-trail subsection (SWEEP-08 invariant)
- Probandurgy positioning: this is NOT a Probandurgy mechanism per se — it's a project-state oracle whose `--gc` mode satisfies the SWEEP-08 audit-trail invariant. The invariant itself is the Probandurgy generalization.
