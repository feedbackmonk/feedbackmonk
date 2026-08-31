---
id: DEFER-003
title: The repo is publishing the developer's Windows account name on origin/main, from four framework-baseline files nobody scrubbed
status: PROPOSED
origin: inject
source-project: ULDF
source-session-id: interactive-20260823T042549Z-400731
injected-at: 2026-08-24T00:15:00Z
autonomy-hint: supervised
suggested-entry-point: implement
scope-estimate: single-session
content-hash: fbm-shipped-identifier-20260824
---

# DEFER-003: The repo is publishing the developer's Windows account name on origin/main

> ## 2026-08-30 — re-measured: the blocker CLEARED, and step 2 is DONE
>
> **The blocking dependency named below has been satisfied.** The ULDF fix has been synced to
> `~/.claude/`: all five deployed baseline files (`dispatchable-sessions/run.{sh,ps1}`,
> `env-preflight/run.{sh,ps1}`, `stranded-dirty-files/validate.ps1`) measure **0** occurrences of
> either spelling. So `/0-uldf-migrate-oracles` will now carry a clean baseline rather than
> re-applying the old files — step 1 is unblocked and is no longer a no-op-or-regression.
>
> **Consequence for PF-UNPIN-01: its trigger has FIRED.** Its one-command test now returns `0`. The
> `.local-customized` pin on `stranded-dirty-files` is, as of today, the only thing holding that
> oracle back from every future upstream fix — which its own note says is the real cost of leaving
> the pin on.
>
> **Step 2 is done.** `CLAUDE.md`'s two occurrences — the ones no upstream change can reach, and the
> ones this brief warned were pasted back *by the repair note for this very problem* — are scrubbed.
> Both were the same verification command; it now assembles the long form from the live account and
> derives the 8.3 truncation from it, so the file names neither spelling. Verified: `grep` of
> `CLAUDE.md` for both spellings returns **0**, and the rewritten command still returns the right
> answer (`0`) when run.
>
> **Steps 1, 3 and 4 remain, and are ALL `.claude/` writes** — the oracle refresh, the pin removal,
> and the re-check. **DEC-84 (CSI-08) hard-defers those for a subordinate-worker session**, which is
> what the 2026-08-30 session was, so it did the one step it could and stopped. This is the same
> block as `DEFER-006`; one grant clears both.
>
> **Remaining exposure, measured today** (`git grep` for both spellings, tracked files):
> `.claude/oracles/dispatchable-sessions/run.{ps1,sh}` and
> `.claude/oracles/env-preflight/run.{ps1,sh}` — **4 files, down from 6**. All four are framework
> baseline files that a refresh now fixes.


> **Counts and paths below are snapshots taken 2026-08-23 from the commands named inline.
> Re-measure before acting** — this repo has moved since, and the upstream fix has landed in the
> meantime. The fix shape in § Success Criterion is the filer's hypothesis from outside your repo;
> re-derive it against your live tree.

## Idea

This repository is **public** (`github.com/feedbackmonk/feedbackmonk`) and currently carries the ULDF
developer's Windows account name in **six places on `origin/main`**. Four of them are framework
baseline files that arrived through an oracle-pack refresh and that nobody has ever scrubbed; two are
in this repo's own `CLAUDE.md`. The upstream defect that keeps re-introducing them has now been fixed
in ULDF, so a refresh will finally carry a clean baseline — but the refresh has to be run here, and
two of the six are this repo's own text, which no upstream fix can reach.

## Originating Context

You are the only public repo among 21 projects on this machine that carry an installed oracle pack.
That census was run specifically to size this exposure, and the answer was *one* — this one.

**Measured 2026-08-23 on `origin/main`:**

| where | spelling |
|---|---|
| `.claude/oracles/dispatchable-sessions/run.ps1`, `run.sh` | `the 8.3 form` |
| `.claude/oracles/env-preflight/run.ps1`, `run.sh` | `the 8.3 form` |
| `CLAUDE.md:144`, `CLAUDE.md:245` | `the long form` |

**Three things about this that are easy to get wrong:**

1. **`the 8.3 form` is the same identifier**, not an adjacent string — it is the DOS 8.3 truncation of
   `the long form`. Your own commit `5bf9878` already treated it that way (it replaced `the 8.3 form` →
   `the scrubbed 8.3 placeholder` alongside `the long form` → `the scrubbed long placeholder` ahead of the first public push). An upstream
   census that grepped only the long form measured **2 tracked files**; grepping both spellings
   measured **10**. If you grep for one spelling you will conclude this is already fixed.

2. **`stranded-dirty-files/validate.ps1` — the file this repo defended by hand — is CLEAN here.**
   Your `.local-customized` pin held through the 2026-08-21 refresh (`de297c3`, hits=0). **The pin
   worked. It guarded one of five doors**, and the other four were never pinned because nobody knew
   they carried the identifier.

3. **Your `CLAUDE.md`'s two occurrences were pasted back by the repair note for this very problem** —
   its verification command is `grep -c the long form ~/.claude/...`, so the note documenting the fix
   re-published the string it fixed. Whatever you write to record this repair, **do not embed the
   literal**: assemble it at runtime, or use an obviously synthetic name and assert on shape.

**Severity, stated honestly**: every occurrence is a **comment**. Zero executable occurrences,
measured. Nothing breaks for anyone, and no project's oracle misbehaves for a different user. This is
a privacy/hygiene issue in a public repo, not a functional defect — size the work accordingly.

**Also true and not fixable by any change you make now**: the identifier is in this repo's public git
**history** at `d71c35a` and `dbbe04a` (measured per-blob). Removing it there means a history
rewrite. The upstream filer and the ULDF lead both recommend **against** one for a comment; that is
the repo owner's call and it is explicitly out of scope for this brief.

## Success Criterion

A grep of `origin/main` for **both** spellings returns zero, and a subsequent
`/0-uldf-migrate-oracles` refresh does not re-introduce them.

Hypothesised route — re-derive it, do not follow it literally:

1. **Pull the upstream baseline fix.** ULDF commit `ff3d4684` (2026-08-24) landed the scrub of all
   ten tracked occurrences plus `HYGIENE-07` — a refresh that names what it is discarding rather than
   silently overwriting a deliberate local edit. **This has NOT been synced to `~/.claude/` yet**
   (owner-gated), so `/0-uldf-migrate-oracles` here will not carry the clean baseline until it has.
   Check before running the refresh, or you will re-apply the old files.
2. **Fix your own `CLAUDE.md`:245 / :144** — no upstream change can reach these. Rewrite the
   verification command so it does not contain the literal.
3. **Remove the `.local-customized` pin on `stranded-dirty-files`** once the baseline is clean — its
   own note says to remove it when the upstream fix lands. Leaving it is a hand-maintained defence
   against a baseline that is no longer wrong.
4. **Re-check both spellings**, not one.

## Dependencies

**Blocking**: the ULDF → `~/.claude/` sync of commit `ff3d4684`. Until the owner runs it, the
deployed baseline still carries the identifier and step 1 is a no-op or a regression.

## Related Artifacts

- This repo: `.claude/oracles/{dispatchable-sessions,env-preflight}/run.{ps1,sh}`, `CLAUDE.md`,
  the `.local-customized` pin on `.claude/oracles/stranded-dirty-files/`
- This repo's history: `5bf9878` (the original scrub, the model for the fix), `dc11003` (the refresh
  that reverted it), `d71c35a` / `dbbe04a` (the history occurrences)
- ULDF: `DEC-420`, `HYGIENE-07/08`, `DISC-SYNC-04`, commit `ff3d4684`,
  `docs/falsifiability/2026-08-23-defer221-shipped-identifier.md`,
  `docs/planning/deferred/DEFER-221_shipped-username-repropagated-by-pack-refresh.md`

## A note on why this brief does not spell the identifier

**Deliberate.** This repository is public, and a brief that embeds the string is another published
copy of it -- which is precisely the mechanism that put it in this repo's own CLAUDE.md (the repair
note for this problem used the literal in its verification command). So:

- **long form** = the ULDF developer's Windows account name.
- **8.3 form** = its DOS 8.3 truncation: first six characters, tilde, the digit one, upper-case.

Both spellings, verbatim, are in ULDF's docs/planning/deferred/DEFER-221_shipped-username-repropagated-by-pack-refresh.md
and in ULDF DEC-420 -- a repo on a local remote, where naming them is safe. Derive them there, grep
here, and **do not paste either spelling into anything this repo publishes**, including the commit
message that fixes this.
