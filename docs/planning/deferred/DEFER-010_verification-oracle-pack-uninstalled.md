---
id: DEFER-010
title: The 17 project Verification Oracles were removed from the tree by the starter-oracle migration — CI job 1 and scripts/ci-local.sh are red
status: RESOLVED
origin: defer-local
source-project: feedbackmonk
source-session-id: 25658496-44d6-4c58-85b9-81c18294aa02
injected-at: 2026-09-07T22:20:00Z
autonomy-hint: checkpoint
suggested-entry-point: plan
scope-estimate: single-session
content-hash: fbm-verification-oracle-pack-uninstalled-v1
---

# DEFER-010: this project's Verification Oracles are no longer in the tree

> Found while restoring `CLAUDE.md` against the instruction-surface standard. Measured, not
> assumed — the commands and their output are below.

## The one-line state

Commit `5d858d2` (2026-09-07, *"retire the pre-rebuild oracle pack and install the ten starter
oracles"*) deleted **every** directory under `.claude/oracles/`, including all 17 of this
project's own Verification Oracles, and installed the ten framework starter oracles in their
place. Nothing re-installed the project pack. Every code-level invariant this project defends is
therefore currently unguarded, and the CI job that ran them fails.

## What is broken, right now

```
$ bash scripts/run-verification-oracles.sh ; echo $?
::error::verification oracle missing: .claude/oracles/multi-tenant-isolation-check/oracle.py
   … 17 lines …
❌ verification-oracle suite FAILED: multi-tenant-isolation-check (missing) …
1
```

- **CI is red on `main`.** `.github/workflows/ci.yml` job `verification-oracles` runs exactly that
  script (line 47).
- **`bash scripts/ci-local.sh` cannot go green** — its first step is the same script (line 48). It
  is the gate `CLAUDE.md` and `docs/dev-notes/rust-ci-parity.md` tell every session to run before
  pushing Rust changes.
- **The invariants are unguarded.** The 17: `multi-tenant-isolation-check`, `pii-scrub-audit`,
  `cors-allowlist-enforcement`, `approval-gate-enforcement`, `public-board-moderation-gate`,
  `solicitation-invariant-check`, `translation-egress-q24-isolation`,
  `feedback-erasure-completeness`, `tier-enforcement-status`, `widget-bundle-size`,
  `selfhost-compose-smoke`, `feedback-as-data-audit`, `public-route-ceiling`,
  `public-id-as-capability`, `submission-idempotency`, `i18n-catalog-integrity`,
  `i18n-literal-ratchet`. `host-tenant-binding` and `translation-gap-status` are gone too, though
  the runner script never listed them.

`CLAUDE.md` § Oracles keeps the one-line record of what each one defends; that table is the
inventory to rebuild against.

## Why it happened, and the fork that has to be decided

The migration's own commit message states the cause: every one of the 73 manifests lacked the
`schema` key the v2 oracle runner requires, so the runner could answer only `unknown` and the
session-start batch spent its budget saying so. Retiring the pack made the briefing honest. What
it did not do is notice that `scripts/run-verification-oracles.sh` invokes `oracle.py` **directly**
— not through the v2 runner — so those 17 were working fine in CI while reading `unknown` at
session start.

So a plain `git checkout 5d858d2~1 -- .claude/oracles/<name>` restores CI immediately and
reinstates 17 `unknown` rows in the briefing. The fork:

1. **Restore, then add `schema` to each manifest** — the oracles are good; only the manifest
   envelope was stale. Most work, best end state.
2. **Restore as-is now, migrate the manifests later** — CI green today, briefing still noisy.
3. **Rebuild each against the v2 runner contract** — cleanest, largest.

Recommendation: (1), sequenced as (2) then the manifest pass, so `main` stops being red on the
first commit rather than the last.

## UPDATE 2026-09-08 — step (2) is done; the fork is now a different one

The session that caused this restored all 17 directories from `5d858d2^` and verified them:
`bash scripts/run-verification-oracles.sh` prints `verification-oracle suite: all 17 PASS` and
exits 0, so **CI on `main` and `scripts/ci-local.sh` are green again**. The briefing cost this
buys back is real and measured: `oracles: 27 pass=9 warn=0 fail=1 unknown=17`.

**The fork above needs a fourth option, and it is probably the right one.** Options (1) and (3)
both assume these belong to the framework's oracle contract. They do not. They carry
`manifest.json` + `manifest.toml` + `oracle.py` + `oracle.sh`, they take a `--full` flag, they are
invoked *directly* by `scripts/run-verification-oracles.sh`, and CI is their consumer. The
framework's contract is `oracle.json` carrying `"schema": "oracle/2"` plus a `run.py` exposing
`run(ctx) -> verdict`. Adding a `schema` key (option 1) would make the v2 runner *try to load a
`run.py` that is not there* and report `unknown` anyway — option (1) does not actually work.

4. **Move them out of `.claude/oracles/`** — say `.claude/verification-oracles/` — so the two
   contracts stop sharing one namespace. The framework runner then never sees them, the 17
   `unknown` rows go away, CI is untouched in behaviour, and nothing has to be rewritten.

Measured blast radius for (4): the 17 directories, one line in `scripts/run-verification-oracles.sh`
(`script=".claude/oracles/$o/oracle.py"`, line 63), and four live references outside the
directories themselves — `CLAUDE.md`, `.claude/skills/1-translate/SKILL.md`,
`admin-ui/src/pages/settings/README.md`, and each oracle's own README. `.github/workflows/ci.yml`
and `scripts/ci-local.sh` call the script and need no edit. Archived collaboration files reference
the old path historically and should not be rewritten.

I did not take (4) unattended: it changes this project's layout, and which home is right is your
call, not the framework's. Everything needed to do it in twenty minutes is above.

## RESOLVED 2026-09-08 — option (4) taken

All 17 directories now live under `.claude/project-oracles/`. The two contracts no longer share a
namespace: the framework runner reports **no `unknown`**, and `bash scripts/run-verification-oracles.sh`
prints `verification-oracle suite: all 17 PASS`, exit 0.

The consumer sweep ran wider than the blast radius estimated above — `git grep -lE
"[.]claude[/\\]oracles[/\\]"` plus a bare-name `git grep -lw <dirname>` per directory — and found
**57 files** carrying a path reference, not four: `scripts/run-verification-oracles.sh`, seven Rust
source and test files (including `crates/feedbackmonk-tracing/tests/scrubber_patterns.rs`, which
reads `expected_hash.txt` through a hard relative path and would have failed to compile-and-pass on
a missed rewrite), eleven module READMEs, `widget/vite.config.ts`, `CLAUDE.md`, `docs/specs/*`,
`docs/operations/{LOCAL_DEV,SELFHOST}.md`, `docs/brand/BRAND.md`, a GitHub tree link in
`marketing/src/pages/blog/show-hn-draft.astro`, and each oracle's own manifests. Dated plans,
handoffs, intakes, scrutiny sets, test-modification records, the observations ledger and `ltads/`
were left alone — they are records of what was true then, not consumers.

Nothing was deleted. One piece of litter went with the move: a tracked
`widget-bundle-size/__pycache__/oracle.cpython-312.pyc`, already covered by `.gitignore` but
committed before that rule existed.

## Scope note

The deleted trees are all recoverable from `5d858d2~1`; nothing was lost, only uninstalled. The
starter oracles installed alongside them are correct and should stay.
