---
id: DEFER-010
title: The 17 project Verification Oracles were removed from the tree by the starter-oracle migration — CI job 1 and scripts/ci-local.sh are red
status: OPEN
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

## Scope note

The deleted trees are all recoverable from `5d858d2~1`; nothing was lost, only uninstalled. The
starter oracles installed alongside them are correct and should stay.
