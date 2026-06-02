# Oracle: `feedback-parity-status`

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
.claude/oracles/feedback-parity-status/oracle.sh           # human-readable
.claude/oracles/feedback-parity-status/oracle.sh --json    # machine-readable (GitCellar gate script)
python .claude/oracles/feedback-parity-status/oracle.py    # canonical, direct
```

GitCellar's cutover script can gate on the exit code: `oracle.sh >/dev/null && proceed_with_cutover`.

## Current state (2026-06-02, pre-build)

0/4 closed — GATE CLOSED (exit 3). This is the expected cold-start before the Stage-2 PODS workers
land their gaps. As each worker merges its gap, the corresponding row flips to CLOSED.

## Lineage

- Surfaced as an oracle candidate in both the GitCellar adoption intake and feedbackmonk's
  `docs/planning/intakes/20260602T120000-ready-feedbackmonk-as-gitcellar-backend.md`.
- Scheduled in `docs/planning/plans/20260602T121500-gitcellar-customer-1-enablement.md`
  (Oracle Pre-Build Plan — build-first per the PARALLEL oracle rule).
