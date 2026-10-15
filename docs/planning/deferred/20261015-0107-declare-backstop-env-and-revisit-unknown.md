---
status: open
commissioned_by: ULDF, session 956946d8 (backlog drain 2026-10-15)
origin: this project's backstop has never been green (evidence .claude/session-state/evidence/finalize-check-20261013T180810-root.log, tip 006e4cf); the framework half landed as VER-54/55, VERD-26
harm: witnessed
beneficiary: product
deferred_because: unranked-idea
---

# Declare `backstop.env`, and decide whether an oracle's UNKNOWN fails `ci-local.sh`

Witnessed 2026-10-13 at tip `006e4cf`: the backstop tree had no `node_modules` in `admin-ui/` and
`widget/` (`'vitest' is not recognized`), `ci-local.sh` printed `DATABASE_URL not set — cannot run
the suite` because the value lives only in the anchor's gitignored `.env`, and the oracle suite
failed on `selfhost-compose-smoke`'s `UNKNOWN ... docker compose config did not answer within 120s`.

The framework now (VER-54, VER-55, VERD-26, `~/.claude/scripts/lib/uldf/backstop.py` `prepare`):
- installs each `package.json` beside no `node_modules` with its lockfile's frozen install (`npm ci`)
  in the backstop's and the re-proof's fresh tree, unless `backstop.setup` declares other commands;
- carries each name in the anchor's `.claude/config.json` `backstop.env` into the suite, its re-runs
  and the proof, from the runner's environment or the anchor's `.env`; a declared name neither sets
  leaves the run `unmeasured`, naming it;
- reads a run that failed naming no cell, on a missing program or an `UNKNOWN` line alone, as
  `unmeasured` rather than `red`.

Do:
1. Add `"backstop": { "env": ["DATABASE_URL"] }` to `.claude/config.json`. Postgres on `5433` must
   be up for the run to measure; with it down the suite fails for real, which is right.
2. **Your call:** `scripts/ci-local.sh` sets `fail=1` whenever `scripts/run-verification-oracles.sh`
   exits non-zero, and that script fails on any oracle's non-zero exit, so
   `selfhost-compose-smoke`'s UNKNOWN on a time bound counts as a failure. The backstop now reads
   an UNKNOWN-only failure as unmeasured, but this script, run by hand or as `test_command`, still
   says "CI-parity checks FAILED". Either keep that (CI parity: a probe that could not answer is not
   a pass), or report UNKNOWN separately with its own exit code and let the caller decide. A time
   bound only ends a process that stopped working (DEC-714), so an UNKNOWN on a loaded machine is
   not evidence of a defect.
3. Watch one backstop run reach its real result: `cargo test --workspace` and both frontends' vitest
   run, with no `unmeasured` naming `DATABASE_URL` or a missing program.

Also seen there: a wave re-proof ran `npx playwright` from outside `admin-ui/`, which fetched
`playwright@1.64.0` and found no tests. If a plan's proof command names Playwright, run it from
`admin-ui/` (`npm --prefix admin-ui exec playwright test`).
