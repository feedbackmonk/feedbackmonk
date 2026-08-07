# disk-artifact-bloat oracle

## Synopsis

Cheap every-session tripwire for regenerable build-artifact disk bloat. Reads the dev drive's free space and emits a `[disk-artifact-bloat]` session-start line ONLY when free space crosses a warn/alert threshold or has dropped sharply since the last sweep. Silent (empty briefing) in the normal all-clear case, and gracefully absent (no line) on any machine where the drive can't be read.

## Why this exists

Trigger: the 2026-06-30 migration of the `SourceControlled` dev tree from a 4 TB HDD to a 447 GB-usable NVMe SSD. The tree's *source* is ~14 GB, but the regenerable artifact footprint (`node_modules` = file-count monster, Rust `target/` = byte monster, plus `.svelte-kit`/`.next`/`.turbo`/`.vite`) historically reached ~500 GB — it **exceeds the new SSD** and cannot all coexist. The risk is silent drift as projects rebuild over the coming weeks with no tripwire. This oracle is that tripwire; the companion `scripts/disk-artifact-sweep.{ps1,sh}` is the report-and-reclaim tool.

## What it checks (cheap tier only)

- Free space of the **drive of the current working directory** by default (machine-agnostic — on this machine projects live on `S:`, so it watches `S:`). Override with config `root` to force a specific drive/path.
- `warn` threshold (default 80 GB) and `alert` threshold (default 40 GB).
- **Drift**: if `~/.claude/session-state/disk-artifact-baseline.json` exists (written by the sweep script), reports how much free space has dropped since that scan; a drop `>= driftGb` (default 50 GB) trips a line even when still above the warn threshold (a rapid-fill tripwire).

It does NOT enumerate artifact directories — that's the expensive deep scan, which lives in the sweep script (on-demand only). Keeping the oracle to a single `Get-PSDrive` / `df` read holds it well inside the session-start briefing budget.

## Configuration

Priority (later overrides earlier): built-in defaults -> global `~/.claude/config.json` `diskArtifactBloat` -> project `.claude/config.json` `diskArtifactBloat` -> env (`ULDF_BLOAT_ROOT` / `ULDF_BLOAT_WARN_GB` / `ULDF_BLOAT_ALERT_GB` / `ULDF_BLOAT_DRIFT_GB`).

```json
{ "diskArtifactBloat": { "root": "S:\\SourceControlled", "warnGb": 80, "alertGb": 40, "driftGb": 50 } }
```

For a machine-wide threshold without editing every project, set it once in global `~/.claude/config.json`.

## Output

Single-line JSON, frozen schema (see `oracle.json`). The `briefing` field is the session-start line (empty = suppressed). Key fields: `level` (`ok`/`warn`/`alert`), `free_gb`, `drive`, `drift_gb`, `baseline_age_days`.

## Companion

`scripts/disk-artifact-sweep.{ps1,sh}` — deep artifact enumeration (ranked report), dormant-`target/` `cargo clean`, stale `node_modules`, `pnpm store prune`, and a per-repo git-tracking audit. **Dry-run / report by default**; nothing deletes without `-Execute` AND an interactive confirm. It writes the baseline this oracle reads.

## Self-test

```
bash .claude/oracles/disk-artifact-bloat/validate.sh
powershell -NoProfile -File .claude/oracles/disk-artifact-bloat/validate.ps1
```

Five scenarios (ok / warn / alert / graceful-absence / drift), bash + PowerShell parity. The verdict is driven by env-forced thresholds against the live free value (the oracle reads a real drive, so it can't be fully sandboxed).
