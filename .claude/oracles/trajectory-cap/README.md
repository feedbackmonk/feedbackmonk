# trajectory-cap Oracle

## Synopsis

Verification Oracle (`kind: "verification"` per Oraculurgy Part 11) that catches `docs/PROJECT_TRAJECTORY.md` breaching its documented size caps — the signature of a `/0-uldf-finalize` Phase 12 run that **appended instead of reshaping**. Checks four signals (total lines, total bytes, longest single-line length, mojibake) and is **advisory** (`status: warn`, exit 0) because the breach is self-correcting within the same finalize via Phase 12's REPAIR path. Fires at `/0-uldf-finalize` Phase 1a (§1a.6). Don't come here for *content* quality of the trajectory, staleness of `Last Updated`, or any other doc — scope is the one file's size invariant only.

> **Category**: documentation | **Kind**: verification | **Strategy**: always-fresh

## Why this oracle exists

Phase 12 (`SPECIFICATION.md` TRAJECTORY-02) maintains `docs/PROJECT_TRAJECTORY.md` as a bounded rolling summary — 5 sections, ~2k tokens, ~250 lines. The size caps were specified as **prose only**: agent-executed instructions in `segments/-finalize/phase12-trajectory.md` with no deterministic enforcement. A run that appended a multi-thousand-character "Last updated" line and skipped the prune step therefore accumulated silently, run after run.

Trigger incident **DISC-TRAJ-01**: bloated trajectories were found across nearly every instrumented project — SessionHelm **262 KB** (with a single **57,372-char** line), quiqpic **277 KB**, GitCellar **198 KB**, quiqpic-baseline 119 KB, AriaScope 49 KB. Worse, Phase 12's original non-blocking rule said *"do not overwrite a file we can't parse"* — so once bloated, the file was preserved forward forever and could never self-heal.

This oracle makes the breach deterministic and every-finalize; the companion fix is Phase 12's new **REPAIR mode** (step 4b), which rebuilds a clean capped file rather than preserving the bloat. The oracle is **detection**; Phase 12 REPAIR is the **actuator**.

## What it checks

Against `docs/PROJECT_TRAJECTORY.md` (if absent → `pass`, nothing to check):

| Signal | Cap | Why |
|---|---|---|
| Total lines | `max_lines` (250) | Accumulation — mirrors the spec's ~250-line hard cap. |
| Total bytes | `max_bytes` (32768) | Accumulation by content weight; ~4× the ~2k-token target as headroom. |
| Longest single line | `max_line_chars` (2000) | **The giant-append signal.** A line-count check alone MISSES this — SessionHelm was only 173 lines but had a 57 K-char line. Load-bearing. |
| Mojibake markers | presence | UTF-8↔CP1252 round-trip corruption (`â€"`, lone `Ã`) from a re-encoding write pipeline. |

Any one breached → `status: warn`. Caps are defined in `oracle.json` `config` (advisory) and as constants at the top of `run.sh` / `run.ps1` (authoritative — edit both to change scope).

## What it deliberately does NOT check

- **Content quality / accuracy** of the trajectory — out of scope; this is a size invariant only.
- **`Last Updated` staleness** — handled by TRAJECTORY-04 read-path consumers, not here.
- **Any file other than `docs/PROJECT_TRAJECTORY.md`** — single-target by design.

## Output

```json
{
  "status": "warn",
  "details": {
    "file_exists": true,
    "path": "docs/PROJECT_TRAJECTORY.md",
    "lines": 173,
    "bytes": 262463,
    "longest_line_chars": 57372,
    "longest_line_number": 5,
    "lines_over_char_cap": 37,
    "mojibake_lines": 0,
    "violations": ["bytes:262463>32768", "longest_line:57372>2000(line 5)"],
    "caps": { "max_lines": 250, "max_bytes": 32768, "max_line_chars": 2000 },
    "scan_duration_ms": 15
  },
  "briefing": "TRAJECTORY BLOAT: ..."
}
```

Output schema is **frozen** — programmatic consumers may read `status`, `details.violations[]`, and the numeric `details` fields.

## Why advisory (not blocking)

`/0-uldf-finalize` runs Phase 1a (this oracle) **before** Phase 12 (the writer/repairer). If the oracle exited non-zero and blocked the finalize at Phase 1a, Phase 12 would never run to repair the file — recreating a deadlock. So a breach is `status: warn` / exit 0: it surfaces the problem and Phase 12's REPAIR path fixes it in the same run; the next finalize's Phase 1a then sees it clean. Exit 1 is reserved for `--self-test` failure.

## Validation

```bash
bash .claude/oracles/trajectory-cap/run.sh --self-test
powershell -NoProfile -ExecutionPolicy Bypass -File .claude/oracles/trajectory-cap/run.ps1 -SelfTest
```

The self-test asserts the detector fires on a synthetic giant-single-line sample and stays quiet on a clean one (anti-silent-breakage check). Exit 0 = the gate is live.

## Lineage

DISC-TRAJ-01 (trigger). Spec: TRAJECTORY-02 (REPAIR + UTF-8 acceptance), TRAJECTORY-05 (this oracle). Decision: DEC-64 (repair-over-preserve; advisory-not-blocking; multi-signal). Sibling pattern: `tracked-docs-not-clobbered` (Pattern C project-wide invariant guard).
