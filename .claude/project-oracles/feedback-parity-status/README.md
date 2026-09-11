# Oracle: `feedback-parity-status`

## Synopsis

Integration Verification Oracle reporting which of the four GitCellar customer-#1 parity gaps (attachments, crash-correlation, full-text search, end-user reads) are closed in this codebase, and whether the GitCellar cutover gate is OPEN. The convergence gate for GitCellar's Path-C adoption of feedbackmonk. Come here before asserting GitCellar can cut over.

**Kind**: Verification Oracle · **Category**: integration · **Gate exit codes**: 0 open / 3 closed / 2 error

## Question

*Which of the GitCellar customer-#1 parity gaps (1–4) are closed in this codebase, and is the
GitCellar cutover gate OPEN?*

This is the **convergence gate** for GitCellar's Path-C adoption of feedbackmonk (see
`docs/integrations/gitcellar-adoption.md` §8 and GitCellar's adoption intake PARITY CHECKLIST).
GitCellar will not retire its internal feedback backend until this reports **GATE OPEN**.

## What it checks (detected from actual code state)

| Gap | Detector | CLOSED when |
|---|---|---|
| #1 Attachments | `migrations/` | `CREATE TABLE attachments` present (widget redaction reported as secondary signal) |
| #2 Crash correlation | `migrations/` | `crash_event_id` column present |
| #3 Admin full-text search | handlers + `migrations/` | `/admin/feedback/search` route OR `tsvector` migration |
| #4 End-user my-feedback API | handlers | `handlers/me_feedback.rs` OR `/me/feedback` route registered |
| #5 Forge bridge | — | **N/A** — GitCellar drops it (DEC-FBR-06); excluded from the gate |

**Gate OPEN** iff all four of #1–#4 are CLOSED.

**Anti-reward-hacking**: parity is read from the tree (migrations, handlers, routes, widget), never
from a self-reported flag a worker could flip. A gap cannot be marked done without the artifact
existing. `multi-tenant-isolation-check` + `pii-scrub-audit` + `widget-bundle-size` provide the
quality legs for the specific gaps (isolation, PII, bundle cap).

## Usage

```bash
python .claude/project-oracles/feedback-parity-status/oracle.py           # human-readable
python .claude/project-oracles/feedback-parity-status/oracle.py --json    # machine-readable (GitCellar gate script)
```

GitCellar's cutover script can gate on the exit code:
`python .claude/project-oracles/feedback-parity-status/oracle.py >/dev/null && proceed_with_cutover`.

## Current state

4/4 closed — CUTOVER GATE OPEN (exit 0), verified 2026-09-11 after the restore. The 2026-06-02
cold start read 0/4 (exit 3) by design, before the Stage-2 PODS workers landed their gaps.

## History

Built 2026-06-02 under `.claude/oracles/`. Deleted by `5d858d2` (2026-09-07, the framework's
shape-based retirement of everything without `schema: oracle/2` and without a script consumer);
missed by the `1ac27a6` restore because it was never on `scripts/run-verification-oracles.sh`'s
list. Restored here 2026-09-11 as `oracle.py` only — the `.sh`/`.ps1` shims were pure delegators
and were not brought back.

## Lineage

- Surfaced as an oracle candidate in both the GitCellar adoption intake and feedbackmonk's
  `docs/planning/intakes/20260602T120000-ready-feedbackmonk-as-gitcellar-backend.md`.
- Scheduled in `docs/planning/plans/20260602T121500-gitcellar-customer-1-enablement.md`
  (Oracle Pre-Build Plan — build-first per the PARALLEL oracle rule).
