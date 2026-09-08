---
id: DEFER-011
title: Two oracles this project cites as live — translation-gap-status and feedback-parity-status — are not in the tree
status: OPEN
origin: defer-local
source-project: feedbackmonk
source-session-id: 2136918b-b63c-44a3-bdd5-4c19fd3c1f05
injected-at: 2026-09-08T07:40:00Z
autonomy-hint: checkpoint
suggested-entry-point: intake
scope-estimate: single-session
content-hash: fbm-cited-oracles-absent-v1
---

# DEFER-011: two cited oracles do not exist

> Surfaced by the `.claude/project-oracles/` migration (`1ac27a6`), which swept every tracked file
> for oracle references on both path separators and by bare name. These two came back cited but
> absent. Not caused by that move — they were already gone.

## The state

`ls .claude/project-oracles/` is 17 directories. Neither of these is among them, and neither is in
`.claude/oracles/` (the framework starter pack) either. `git log` shows no removal in the
starter-oracle migration window, so they predate it.

### `translation-gap-status` — the more urgent of the two

Cited as a live consumer by:

- `.claude/skills/1-translate/SKILL.md:125` — as the source of the same MISSING/DRIFTED reporting
  the skill drives from.
- `scripts/i18n/README.md:74` — as one of three oracles consuming `check-gaps.py --json`, described
  as wrapping it.

**This blocks a live release gate.** `CLAUDE.md` § Pending Follow-Ups carries *"Trigger: before the
next release — run `/1-translate`"*, and that entry states the `translation-gap-status` oracle
"reads DUE". It cannot read anything; it is not there. Whoever runs the release-gate translation
pass will invoke a skill that references a directory that does not exist.

`scripts/i18n/check-gaps.py --json` **does** exist, so the underlying measurement is available —
what is missing is only the oracle wrapper.

### `feedback-parity-status` — a spec that says BUILT

`docs/specs/SPECIFICATION.md:210` records it as **BUILT (collab-20260602-123000, Stage 1
pre-spawn)** with a full description: per-gap CLOSED/OPEN detection, a CUTOVER GATE line, exit 0 =
gate open, "GATE OPEN 4/4 at convergence". `docs/specs/DECISIONS.md:669` describes it as gating
GitCellar's Path-C cutover, detection-from-code-state as the anti-reward-hacking leg.

The tree does not have it. Two readings, and the tree alone cannot distinguish them:

1. It was built, converged, and lost in some later cleanup — recoverable from history.
2. The spec was "reconciled from implementation" (its own words) against an implementation that
   never actually landed, and has been claiming BUILT since 2026-06.

Reading (2) is the one that matters, because a spec asserting a guard exists is worse than a spec
that never claimed one.

## What to do

1. `git log --all --diff-filter=D --name-only -- '*feedback-parity-status*'` and the same for
   `translation-gap-status`. If either is in history, restoring it is a `git checkout <sha>^ --`
   away and this becomes a twenty-minute job.
2. If `translation-gap-status` is not recoverable, either build the wrapper over
   `scripts/i18n/check-gaps.py --json` (it is advisory-only and never blocks, so it is small) or cut
   the two citations and the `CLAUDE.md` release-gate claim that it "reads DUE". Do not leave the
   citations pointing at nothing.
3. If `feedback-parity-status` is not recoverable, the SPECIFICATION.md and DECISIONS.md entries
   need correcting to what is true — an unbuilt intention, not a BUILT gate.

## Related

`host-tenant-binding` is the third oracle this project's prose names and does not have. That one is
already covered by DEFER-006 and is deliberately not-yet-installed, so it is out of scope here.

All three are flagged in `CLAUDE.md` § Oracles as unbuilt intentions rather than guards, so no
session should now mistake them for coverage.
