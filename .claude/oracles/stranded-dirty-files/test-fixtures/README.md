# stranded-dirty-files fixtures

> Synopsis — Five representative scenarios covering the strand classes the oracle must classify correctly. Inputs are git working-tree state + registry contents — not check-inable as static trees, so the `validate.{sh,ps1}` scripts build sandboxes inline using the patterns documented here.

## Purpose

Anchor the FROZEN output schema (`{has_stranded, count, oldest_mtime, sample, live_peer_count, last_finalize_at, briefing}`) and detection semantics (mtime-vs-HEAD-commit + live-peer ownership filter) against the failure modes a real adopting project can land in.

## Per-case scenarios

| Fixture | Setup | Expected | Why this case matters |
|---|---|---|---|
| `no-stranded/` | git repo with one HEAD commit; one dirty file with mtime AFTER HEAD's commit timestamp | `count==0`, `briefing==""` | Empty-briefing case — line MUST suppress in hook briefing |
| `small-stranded/` | git repo with one HEAD commit; three dirty files with mtimes BEFORE HEAD's commit timestamp; no live peer claims | `count==3`, briefing references "no live owner" | Trigger-pattern shape (typical post-DISC-HYGIENE-01 strand) |
| `large-stranded/` | git repo with one HEAD commit; 55 dirty files with mtimes BEFORE HEAD's commit timestamp | `count==55`, briefing references "significant accumulation" | Threshold case — count >= 50 switches briefing to "significant accumulation" form, pointing at `/0-uldf-oracle stranded-dirty-files` for the full sample |
| `detection-skipped-too-many/` | git repo with one HEAD commit; 2001 dirty files | `count==-1`, briefing references "detection skipped" | Scope-guard case — protects briefing budget by sentinelling at 2000 dirty files |
| `live-peer-owns-file/` | git repo with two old-dirty files (`peer-claimed.txt`, `unclaimed.txt`); registry has one live entry whose `dirtyFiles[]` claims `peer-claimed.txt` | `count==1`, `live_peer_count==1`, `sample` contains only `unclaimed.txt` | Forward-compatible ownership-filter case — when peers DO publish ownership claims, claimed files MUST drop out of the strand classification |

## Public API

The fixtures are inline-built by `validate.sh` and `validate.ps1`. Each scenario is constructed by:

1. Creating a fresh sandbox under `$TMPDIR` (or `$env:TEMP` on Windows).
2. Running `git init` + a single seed commit dated `2026-04-01T00:00:00Z` (the finalize boundary).
3. Touching dirty files with the relevant mtimes.
4. (T5 only) writing a `.claude/collaboration/active-sessions.json` registry pointing at the validate process's own PID (always alive, portable across `kill -0` and `Get-Process`).
5. Running `bash .claude/oracles/stranded-dirty-files/run.sh` (or the `.ps1`) inside the sandbox.
6. Asserting the JSON output's shape + key fields.

The full hygiene smoke harness at `claude-template/scripts/hygiene-tests/hygiene-csi15-stranded-smoke.{sh,ps1}` reuses these patterns AND adds three FINALIZE-04 flag-wiring smoke cases (without-flag, with-flag, composable-with-shared).

## Constraints

- Fixtures must produce git working-tree + registry shapes that exercise the FROZEN output schema's edge cases. Any addition that doesn't cover a new schema-edge is noise.
- The seed commit's date is `2026-04-01T00:00:00Z` (locked via `GIT_AUTHOR_DATE` / `GIT_COMMITTER_DATE` env vars in the validate scripts) so old-dirty files at `2026-03-15T00:00:00Z` deterministically predate the finalize boundary regardless of when the test runs.
- The live-peer fixture (T5) MUST use the validate process's own PID for portable liveness — using a hardcoded PID would either be dead (false-strand) or owned by an unrelated process (registration accident).

## Decision log

- **Why inline sandboxes rather than checked-in fixture trees?** Same precedent as `dispatchable-sessions/test-fixtures/` (which holds reference JSON only — actual sandboxes are inline). Static dirty git working trees aren't checkable-in; the inline-build pattern keeps the validate harness self-contained and the fixture-data minimal (this README).
- **Why no per-case subdirectories with reference data?** Unlike `gitignore-template-drift` (where the inputs are static gitignore text files), this oracle's inputs are dynamic: mtime sentinels, dirty-file enumeration, and a registry that must reference the live-PID at run-time. Static reference data per case would only duplicate the inline-build code — keeping it inline reduces drift risk.
