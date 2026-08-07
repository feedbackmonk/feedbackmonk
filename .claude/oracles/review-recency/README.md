# review-recency oracle

**Question**: *Did I already run a heavy periodic review/refactor skill on THIS project recently — such that re-running it now would be redundant?*

Kind: project-state (workflow). Runs every session (fast lane). Emits a
`[review-recency]` ORACLE BRIEFING line **only** when a covered skill ran on
this project within `recentDays` (default 14) — otherwise the `briefing` field
is empty and the session-start fan-out shows nothing (parallel to
`stale-ltads-state`). Trigger: 2026-07-03 — the failure mode is re-running an
expensive project-wide review shortly after already running it, having
forgotten. DEC-139; spec RECENCY-03/04.

## The subsystem (one reader, three consumers)

This oracle is the **per-project briefing** consumer. All aggregation lives in
the single source of truth `scripts/review-recency.{sh,ps1}` (RECENCY-01) — a
deterministic reader over the `command-usage/*.jsonl` invocation registry
(written by `hooks/command-usage-tracker.{sh,ps1}`). No new store: this is a
*surfacing* layer over data that already exists.

| Consumer | Scope | Surface |
|---|---|---|
| **this oracle** | current project | `[review-recency]` briefing line (RECENCY-03/04) |
| **in-skill guard** | current project | pre-flight notice in `/0-uldf-scrutinize`, `/0-uldf-uladp-compliance` (RECENCY-05) |
| **portfolio recency** | all projects | `/0-uldf-portfolio recency` at-a-glance table (RECENCY-06) |

## Covered-skill set

Default: `0-uldf-scrutinize`, `0-uldf-uladp-compliance`. Override per-project via
`.claude/config.json`:

```json
{ "reviewRecency": { "skills": ["0-uldf-scrutinize", "0-uldf-uladp-compliance"], "recentDays": 14, "staleDays": 30 } }
```

## Output schema (frozen)

```json
{
  "recent": true,
  "details": {
    "project": "ULDF",
    "recentDays": 14,
    "recentSkills": [ { "skill": "0-uldf-scrutinize", "lastUsed": "2026-07-01", "lastArg1": null } ]
  },
  "briefing": "reviewed on this project within 14d — 0-uldf-scrutinize 2026-07-01; a re-run may be redundant (see /0-uldf-portfolio recency)"
}
```

`recent:false` **always** carries an empty `briefing` (quiet-path invariant,
asserted by `validate.{sh,ps1}`).

## Honesty floor (NO-DATA discipline)

- **"never"** means *no record in the registry* — and the registry only reaches
  back to `recordsSince` (surfaced by the reader). A fresh registry is NO-DATA,
  never "never reviewed".
- **Scope detail** (`lastArg1` — which part was reviewed) only accrues from
  `argFidelitySince` = **2026-07-02** (the `arg1` telemetry floor). An older run
  is recorded, but its scope may be `null`. The invocation timeline itself
  reaches back further (to `recordsSince`).
- Missing registry / reader-not-found / jq-absent → graceful absence (empty
  briefing), never a fabricated verdict.

## Self-test

```bash
bash validate.sh        # Unix
pwsh -File validate.ps1 # Windows
```

Asserts valid JSON, the schema fields, and the `recent:false ⇒ empty briefing`
invariant.
