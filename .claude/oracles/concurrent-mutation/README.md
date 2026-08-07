# concurrent-mutation oracle (CSI-10, CSI Phase 3)

**Kind**: `verification` (ORACULURGY_DESIGN.md Part 11) -- the artifact-time leg of CSI's three-leg defense.
**Question**: *Have LTADS lifecycle files or `ltads/execution/<arc>/` been externally mutated since this session's last known-good snapshot?*

This is the Phase 3 (Meta-Verification) mechanism of Cross-Session Integrity. Phase 1 surfaces peers at the prompt level; Phase 2 (CSI-07/08/09) gates writes at runtime; this oracle detects an external mutation **independently of any in-session decision the agent made** -- the property the three-leg defense (DISC-PRO-08) requires.

## What it does

Two signals, either one fires (hybrid baseline, DEC-22):

| Signal | Catches | How |
|---|---|---|
| **mtime delta** | uncommitted concurrent writes | current file mtime vs the per-file last-observed-mtime recorded in the baseline |
| **git-log delta** | committed external mutations | `git diff --name-only <baseline.headSha>..HEAD` over the tracked paths |

Tracked path set (CSI-10, config-extensible in `oracle.json` `config`; ARC-03/DEC-199 re-anchor): `ltads/arc-state.json`, `ltads/arc-state.archive.json`, `ltads/sessions/session-history.md`, `ltads/sessions/blockers.md`, and every file under `ltads/execution/`.

## Output schema (frozen -- CSI-10 acceptance)

```json
{
  "external_mutation": true,
  "mutations": [
    {"path": "ltads/arc-state.json", "source": "mtime", "since": "2026-06-25T21:00:00Z", "by_session": null}
  ],
  "baseline_age_seconds": 1234,
  "summary": "1 external LTADS mutation(s) since 2026-06-25T20:39:26Z",
  "briefing": "1 external LTADS-state mutation(s) since ... -- another session may be editing this arc; reconcile before committing"
}
```

- `external_mutation` -- true when `mutations` is non-empty.
- `mutations[].source` -- `git-log` | `mtime`. `mutations[].since` -- when the mutation was observed (commit committer-date / file mtime). `mutations[].by_session` -- `null` in practice: uncommitted writes carry no session attribution and git commits are human-authored; the field is reserved for future enrichment. It is never *this* session (that is why the entry is reported).
- `baseline_age_seconds` -- seconds since the baseline was captured; `null` when no baseline exists for this session (graceful absence).
- `summary` -- one-line headline (CSI-10 fixed field).
- `briefing` -- empty when `external_mutation=false` (the session-start hook then emits no line -- gracefully absent, same convention as `stale-ltads-state` / `dispatchable-sessions`); the one-liner otherwise. The hook prepends the `[concurrent-mutation]` tag.

**Schema note**: this oracle emits the CSI-10 **domain schema** (`external_mutation`/`mutations`/`baseline_age_seconds`/`summary` per SPECIFICATION CSI-10 acceptance) plus the `briefing` field, mirroring the CSI briefing-oracle convention rather than the generic Part 11 `status`/`details` envelope. The finalize Phase 1a verification-oracle runner (`run-verification-oracles`) discovers it (kind:`verification`), runs it, and -- because the output carries no `status` and the script exits 0 -- records a harmless `pass`; CSI-10 is **advisory** and never blocks a commit. The authoritative surfaces are the session-start briefing line and the finalize Phase 0 check (`segments/-finalize/phase0-prerequisites.md` Sec 0.7).

## Read-only contract + the baseline writer

`run.{sh,ps1}` are **strictly read-only** (Part 11 Sec 11.3.4). They never write. The per-session baseline they compare against is persisted by the sibling **`update-baseline.{sh,ps1}`**, which is invoked:

- by the **session-start hook** (establish/refresh at session start -- fire-and-forget, cheap, fail-open), and
- by **`/0-uldf-finalize`** Phase 0 (refresh before commit prep).

The baseline lives in `.claude/session-state/this-session.json` under the `csi10Baseline` key (`{sessionId, sessionStart, capturedAt, headSha, mtimes:{<relpath>:<epoch>}}`), written read-merge-write so no other top-level key is clobbered. Keying by `sessionId` makes the baseline per-session: a snapshot authored by a different session is *not mine*, so the oracle reports graceful absence rather than attributing a peer's edits to me. Concurrent sessions sharing the workdir may overwrite each other's baseline (last-writer-wins) -- at worst a missed detection (failure-open), never a false block.

## Response semantics (autonomy-tiered)

| Autonomy | On `external_mutation: true` |
|---|---|
| `collaborative` | warning in the briefing / finalize summary (default) |
| `supervised` | prompt with the summary |
| `autopilot` | halt with a structured prompt directing to `/0-uldf-ltads-admin decision` (DEC-12 G6) |

## Boundaries

- **mtime leg compares only files present in the baseline.** A file *created* after the baseline by a peer is caught by the git-log leg once committed, not by the mtime leg (avoids false-positives on this session's own new files). The git-log leg requires a captured `headSha` (always present in a git repo).
- **git-log `--since` self-commits.** Commits this session itself authored mid-session can appear in the `headSha..HEAD` range. This is advisory noise on a warn-only oracle; the mtime leg (refreshed on this session's own writes) is the precise per-session signal.
- **Failure-open everywhere.** Absent / unparseable / foreign-session baseline, no JSON parser, no git -> `external_mutation:false`, `baseline_age_seconds:null`, empty briefing. The oracle never blocks editing or committing.

## Performance

200ms cold design budget (CSI-10 acceptance). The fast-lane timeout floor (2000ms) covers Git Bash `git` latency on win32 (the slowest supported environment).

## Files

| File | Role |
|---|---|
| `oracle.json` | manifest (kind:`verification`, `every`-session, tracked-path config) |
| `run.sh` / `run.ps1` | read-only detector |
| `update-baseline.sh` / `update-baseline.ps1` | baseline writer (the only writers) |
| `validate.sh` / `validate.ps1` | self-test (valid JSON + schema fields present) |

Smoke: `~/.claude/scripts/csi-tests/csi-10-smoke.sh` (real git sandbox, python-controlled mtimes, bash + PS parity -- 22 assertions).

## Lineage

CSI-10; DEC-19 (CSI as the eighth Probandurgy mechanism), DEC-22 (hybrid baseline). Trigger incidents DISC-CSI-01 / DISC-CSI-02. Full design: `FOUNDATIONS/CSI_DESIGN.md` Sec 6.
