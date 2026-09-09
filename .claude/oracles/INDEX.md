# Oracle Index — the framework starter pack

This directory is where a **ULDF framework starter oracle** would live if this project pinned one.
It installs none.

Run one: `python ~/.claude/oracles/_runner/run.py --project . --only <name>`
Run all: `python ~/.claude/oracles/_runner/run.py --project .`

---

## The starter set — served from the home pack, not installed here

The runner serves every starter whose `install_when` matches from `~/.claude/oracles/` **in place**,
each batch row reading `source: "home"`, so the session-start ORACLE BRIEFING is unchanged and
`finalize.py check` still takes its oracle proofs.

A tracked copy of a template oracle is a twin that drifts from the day it lands, so setup copies
none (DEC-538). The ten copies this project carried until 2026-09-09 — `arc-state`, `git-state`,
`markdown-link-validity`, `module-readme-parity`, `peers`, `pending-followups`, `pending-ideas`,
`project-index`, `project-type`, `workflow-position` — were retired for that reason; every one of
them is a name the pack ships, so no question disappeared.

To see the live roster and each starter's own question, cost and `install_when`, run the batch or
read the pack:

```
python ~/.claude/oracles/_runner/run.py --project .
ls ~/.claude/oracles/
```

A project that wants to pin its own version of a starter installs that one directory here
deliberately; a directory carrying `"schema": "oracle/2"` shadows the pack, whoever wrote it.

---

## This project's own oracles are NOT here

The seventeen **Verification Oracles** that defend feedbackmonk's code-level invariants live in
**`.claude/project-oracles/`**. They are on an older, different contract — `manifest.json`, a
canonical `oracle.py`, `oracle.sh`/`oracle.ps1` shims, a `--full` flag — and are invoked directly by
`scripts/run-verification-oracles.sh` (CI job `verification-oracles`, and `scripts/ci-local.sh`
step 1), never by the session-start runner. See `.claude/project-oracles/README.md`, and
`CLAUDE.md` § Oracles for the one-line table of what each defends.

Do not put one contract's directory in the other's home: the framework runner can answer a
directory of the project-oracle shape only `unknown`, which is what the 2026-09-08 split ended.
