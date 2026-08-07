# `retirement-candidates` Oracle (CTXY-04)

**Question**: which passages inside this project's *living* durable artifacts carry a
deterministic signal that they may no longer earn their place?

**Kind**: `verification` · **Lane**: slow · **Status**: `pass` | `warn` — never `fail`.

---

## Why it exists

Every other cleanup mechanism in this framework removes **whole files on mechanical
signals**: `handoff-retention` (TTL), `planning-doc-staleness` + finalize Phase 8.7
(commit slug / spec `[DONE]`), `pid-orphan-detector` (PID liveness), `trajectory-cap`
(byte caps — whose own manifest says *"this oracle bounds bloat, it does not judge
content"*).

The bloat that actually accumulates lives **inside a file that must itself stay alive**.
The trigger case: a 160 KB `docs/PENDING_FOLLOW_UPS.md` that project CLAUDE.md instructs
every session to read at startup, roughly 39% of which was narration of already-finished
work. No whole-file sweep will ever touch that file, correctly — and none of them look
inside it.

This oracle is the content-level sibling of SWEEP-02. It detects; it never acts.

## The contract — worklist, not delete list

**Every signal here is a proxy, and the thing being proxied is not machine-visible.**

A `DONE` marker measures that somebody typed DONE. It does not measure whether the
surrounding text still changes a future decision. A post-mortem of a bug fixed nine months
ago and a trap note that is the *only* thing preventing a repeat are textually
indistinguishable — the discriminator is *"does a mechanical guard cover this trap?"*,
which requires reading the codebase.

So the output is a worklist for an agent that then applies `segments/_retirement-test.md`.
The oracle never deletes, never archives, and never recommends deletion. Both action legs
(finalize Phase 8.8, `/0-uldf-context-audit --retire`) are required to run the retention
test per candidate rather than trusting the signal.

**A `pass` is not an all-clear.** Utility decay leaves no textual trace: most of the 45 KB
recovered from the trigger corpus was *accurate* prose carrying no marker at all. `pass`
means "nothing flagged", never "nothing dead". Read `assertion.known_gaps` in
`oracle.json` before treating a green run as evidence of anything.

**Accuracy decay is out of scope, including the SUPERSEDE-IN-PLACE class.** A document
whose factual claims contradict the system — a proven-then-fixed cipher description a spec
still cites — reads as ordinary confident prose. Detecting that needs a claim checked
against the system, not a marker. `self-supersession` fires only where someone already
wrote the banner; it never finds a document that *needs* one.

## Signals

| Signal | Fires on | Disposition |
|---|---|---|
| `exit-condition-satisfied` | a `Remove/Delete … once/when/after …` line whose ISO date is now past | candidate — highest confidence, verify against the *system* not the entry's claim |
| `exit-condition-declared` | an exit condition present but not machine-evaluable | **`surfaced[]`**, not a candidate |
| `done-marker` | uppercase `DONE`/`COMPLETE`/`FIXED`/`RESOLVED`/`SUPERSEDED`/`DISCHARGED`/`SHIPPED`/`LANDED`/`CLOSED`, or ✅ | candidate |
| `correction-strata` | ≥2 `CORRECTION`/`UPDATE`/`PROGRESS`/`AMENDMENT`… layers under one heading | candidate (CTXY-08 shape — collapse to current truth) |
| `provisional-no-exit` | a **file** matching `provisional_paths` with no exit condition anywhere | candidate (CTXY-07 authoring gap — usually *add the condition*, not delete) |
| `self-supersession` | a supersession banner in the first 15 lines of a file that continues 40+ lines | candidate — the banner invalidates a body still being read |
| `no-inbound-refs` | basename mentioned nowhere else in the tree | candidate — **verify by hand**, see below |

Lowercase `done`/`fixed` are deliberately not matched: they appear in ordinary prose
constantly and are pure noise. Fenced code blocks are skipped entirely — markers inside
them are sample output, not claims about the document.

**Exit conditions have two strengths, and the split is load-bearing.** Only a *directive*
(`Remove`/`Delete`/`Retire`/`Drop`/`Prune` within 60 chars of `once`/`when`/`after`/`upon`),
carrying a past ISO date, outside a markdown table, can be reported as `satisfied`. The bare
phrase "exit condition" is a *mention* and can only ever reach `declared`. Without that
split, prose **about** exit conditions that happens to carry any date fires as satisfied —
dogfooding produced three such hits and all three were false, including the CLAUDE.md
section describing this oracle. `DEFER-044` records the identical defect in
`governing-doc-consistency` ("a stale date appears **anywhere on the line**"), where the
prescribed remedy would have written a lie into the docs. That trap was documented before
this oracle existed; validate T11/T12 are the regression, so the prose no longer has to be
the guard.

`provisional-no-exit` is a **file-level** verdict, not per-heading. Firing it per heading
turned a 74-file scan into 312 "candidates" during development; a deferred brief is one
artifact with one authoring gap.

## Reading `no-inbound-refs` safely

This is the signal most likely to get someone hurt. A survey using the same heuristic once
nominated a 985 KB corpus for deletion before discovering a pre-commit gate depended on the
directory merely existing. The matcher is a substring pass over one tree-wide grep: it
cannot see references from outside the repo, references by title rather than filename, or
a dependency on a *path* rather than a file. Before acting, run Step 4.1 of
`segments/_retirement-test.md` by hand — including source files and hook scripts.

**And the signal points the wrong way on sole copies.** The files it flags — spent briefs,
orphaned notes — are precisely the ones most likely to hold the *only* copy of a step some
live procedure still performs, because that dependency is on **content** and the consuming
document names a destination ("follow Phase 0") rather than the source. No signal here can
see it. Run Step 4.5 (rescue before delete) on every `no-inbound-refs` hit: grep for the
thing itself — a distinctive command, a path, a threshold, a phrase from the instruction —
and confirm the destination that a live document names actually contains it.

## Consumers

- **`/0-uldf-finalize` Phase 8.8** (CTXY-05) — the inline leg. Its primary question is
  *what did this change just obsolete?*; this oracle's worklist is its secondary input.
- **`/0-uldf-context-audit --retire`** (CTXY-06) — the sweep leg / backstop.
- Session-start briefing — not wired by default. The worklist is not per-session
  actionable, and a standing "175 candidates" line would train agents to ignore it.

## Configuration

`config.corpus` in `oracle.json` documents the contract; the authoritative values live in
`run.{sh,ps1}`. The default list is deliberately conservative — a wide corpus drowns the
worklist. `CLAUDE_RETIREMENT_CORPUS` (colon-separated globs) overrides it; the validators
use this.

**Widen it on adoption, and check the coverage ratio before believing a `pass`.** The
default matched **11 of 414** doc files on the second project dogfooded (2026-08-02), and
its 82 KB follow-ups file was invisible for two independent reasons — a kebab-cased
filename the literal glob missed, and a level-1-only heading (fixed in v1.1.0: spelling-
tolerant globs, and blocks now open on any heading level 1–6). A `pass` describes the
corpus, never the project; if a project keeps a living artifact class somewhere else, the
oracle will say nothing about it and say it in green.

## Validation

```bash
bash .claude/oracles/retirement-candidates/validate.sh          # 22 cases incl. twin parity
powershell -NoProfile -File .../validate.ps1                    # 20 cases, PS twin standalone
bash .claude/oracles/retirement-candidates/run.sh --self-test   # inline detector check
```

`validate.sh` T10 drives **both** twins over one sandbox and asserts an identical candidate
set (TWIN-01..03). T8 is the load-bearing case: a clean document containing a trap note and
a rejected-alternative note must yield **zero** candidates. A detector that fires on
guardrails is worse than no detector — it teaches agents to delete exactly the text the
discipline exists to protect.

## Performance note

The obvious implementation ran 30s on a 74-file corpus. Two shapes caused it, both worth
avoiding in any oracle on Windows: one `git grep` **per file** for the inbound probe, and
one `awk` **per file** for the scan. Process spawn under MINGW costs ~150–250ms, so 74
files paid ~15s in fork overhead alone. Both were folded into a single tree-wide pass;
the result is byte-identical and runs in 7.8s (bash) / 3.2s (PowerShell).
