# `hook-verification-coverage` Oracle

> **Synopsis**: Verification Oracle enforcing EPP-03 — *"no hook ships without its own verification surface."* Walks the hook scripts registered in `settings.json` and verifies each has a test file under `hooks/tests/` or a smoke-harness reference under `scripts/csi-tests/` **or** `scripts/smoke-tests/`. New uncovered hooks `fail`; grandfathered legacy hooks `warn`.

## Purpose & Responsibilities

A buggy Tier-1 enforcement mechanism fails **confidently and invisibly** — the system believes the behavior is enforced while the mechanism misfires (DEC-71 / DISC-CSI-12: the SessionEnd hook concluded the orchestrator's arc four times in one autopilot run; the 2026-06-11 sync chmod stall: the deploy step silently hung ~45 minutes). EPP-03 (`FOUNDATIONS/ENFORCEMENT_PLACEMENT_PRINCIPLE.md` § 2.4) therefore requires every hook to ship with its own verification surface. This oracle is the deterministic inventory check for that clause — and the clause's own verification surface (a clause demanding verification surfaces that had none would be self-refuting).

## File Index

- `oracle.json` — manifest (`kind: "verification"`, frozen output schema, always-fresh)
- `run.sh` — Unix check (grep settings.json for `hooks/<name>.{sh,ps1}`, test coverage per hook; searches BOTH `scripts/csi-tests/` and `scripts/smoke-tests/` — searching only the former produced three false EPP-03 violations, fixed 2026-07-13)
- `run.ps1` — Windows PowerShell parity implementation
- `validate.sh` / `validate.ps1` — five-scenario self-tests (fail / tests-file pass / smoke-reference pass / baseline warn / graceful absence)
- `README.md` — this file

## Public API & Usage

### Output Schema (frozen)

```json
{
  "status": "warn",
  "details": {
    "applicable": true,
    "checked": 8,
    "covered": ["session-start", "session-end", "pre-bash-pwsh-guard", "stop-recommendation-gate"],
    "uncovered_new": [],
    "uncovered_legacy": ["session-detect", "session-commit", "command-usage-tracker", "pre-compact"]
  },
  "briefing": "EPP-03: legacy hook(s) still lack a verification surface (grandfathered): [...]"
}
```

Status semantics: `fail` (exit 1) iff `uncovered_new` is non-empty; `warn` when only grandfathered hooks are uncovered; `pass` otherwise.

### Coverage Definition

A hook basename `<name>` (extracted from `hooks/<name>.{sh,ps1}` references in `settings.json`) is **covered** when any of:

1. `hooks/tests/<name>.test.sh` or `hooks/tests/<name>.test.ps1` exists
2. Any file under `scripts/csi-tests/` references `<name>.sh` or `<name>.ps1`

The smoke-reference leg is a *reference heuristic*, not an execution proof — a smoke that names the hook is assumed to exercise it. That is deliberate (the oracle stays <2s and read-only); the smoke's own pass/fail discipline lives with the smoke.

### The Grandfathered Baseline

Hooks shipped before EPP-03 (2026-06-11) without a surface: `session-detect`, `command-usage-tracker`, `pre-compact` (`session-commit` left the baseline 2026-06-11 when `oracle-consultation-log-smoke.sh` landed). Hardcoded identically in `run.sh` and `run.ps1`. **Remove an entry when its test or smoke lands** — the baseline only shrinks. Adding an entry is an EPP-03 violation by definition (it would grandfather a *new* hook).

### Direct Invocation

```bash
.claude/oracles/hook-verification-coverage/run.sh        # Unix
```

```powershell
.claude\oracles\hook-verification-coverage\run.ps1       # Windows
```

Self-test: `bash validate.sh` / `powershell -File validate.ps1` in this directory.

## Constraints & Business Rules

- **Read-only, always-fresh, <2s** per the Verification Oracle contract (`ORACULURGY_DESIGN.md` Part 11 § 11.3).
- **Scan-root resolution**: `claude-template/settings.json` (framework repo) first, else `.claude/settings.json` (instrumented project); neither → `pass` with `applicable: false` (graceful absence). <!-- path-ok: framework-repo scan-root case -->
- **`HVC_ROOT`** env var overrides the scan root (used by the self-tests).

## Relationships & Dependencies

- **Enforces**: EPP-03 (`docs/specs/SPECIFICATION.md` § EPP; `FOUNDATIONS/ENFORCEMENT_PLACEMENT_PRINCIPLE.md` § 2.4)
- **Decision**: DEC-74
- **Consumed by**: `/0-uldf-finalize` Phase 1a when hook files or `settings.json` are in the diff; framework-development sessions touching hooks

## Decision Log

- **2026-06-11 — Baseline-in-scripts, not in manifest**: the grandfather list is hardcoded in both run scripts rather than parsed from `oracle.json`, because the scripts must stay dependency-free (no `jq` guarantee on Unix) and the list is small and shrink-only. WHY: parse-robustness over single-sourcing for a four-entry, delete-only list; README + manifest `baseline_note` document the mirror obligation.
- **2026-06-11 — `warn`, not `fail`, for legacy hooks**: failing on day one would block every finalize until four legacy test harnesses are written, training reflexive bypass — the exact Goodhart failure EPP-02's false-positive criterion warns about. The `fail` lane is reserved for the regression direction (new uncovered hooks).
