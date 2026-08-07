# overseer-wake oracle (CSO Phase 1, component C1)

**Kind**: `verification` (briefing-emitting) -- answers a *"is there anything worth acting on right now?"* question in the inner loop, but the action is *waking an agent*, not blocking a commit.
**Question**: *Is there anything in the airspace worth waking the LLM Overseer for?*
**Spec**: `docs/specs/SPECIFICATION.md` § CSO-09 (cost discipline), CSO-04 (the four Phase-1 detectors), CSO-21 (the fifth, `resource-contention`), CSO-01 (Overseer tier). Resolves Q-CSO-01. Contracts: `~/.claude/templates/CSO_CONTRACTS.md` § 1.1 (the frozen detector roster).

## What it does

This is the **cheap deterministic mechanic** of CSO's three-role split (DEC-98) and the **cost defense** of the whole design (CSO-09 / Q-CSO-01). It is a PURE-SHELL aggregator over the five already-deterministic detectors (CSO-04 + CSO-21) — **no LLM is in this path**. The expensive LLM Overseer is woken **only** when this oracle raises `wake: true`; it never runs on a hot always-on poll (the documented 15× multi-agent cost trap, Anthropic). Routine watching costs ≈0 LLM tokens; the Overseer's token spend is proportional to the number of *real* flagged events, not to wall-clock.

The five sources (each gracefully absent — a missing source is **NO-DATA**, omitted, **never** a fabricated all-clear; `wake` fires only on positive evidence):

| Source | detectorId | What fires |
|---|---|---|
| (d) stall | `stall-monitor` | a CSI-registry `sessions[]` entry still marked `active` whose `claudeShellPid` is **dead** (the one-shot form of the DEC-69 pods-monitor PID-dead/terminal signal) |
| (a) concurrent-mutation | `concurrent-mutation` | the sibling CSI-10 oracle reports `external_mutation: true` |
| (b) shared-foreign-claim | `shared-foreign-claim` | another live session holds a non-empty, unexpired `sharedClaim` on a discovered shared repo |
| (c) touches pre-screen | `touches-conflict` | a PODS `touches.json` path is claimed by ≥2 distinct **live** sessions (a **cheap pre-screen**; the full line-overlap classification is the Overseer-run C8 pass, Q-CSO-04). Liveness (DEC-215): a claimant counts only when backed by a live PID in the claiming collab dir's `workers/<id>/shell.pid` or an active registry entry mentioning that collab id — claims from archived/absent sessions are stale by construction (fixes the 2026-07-22 phantom where 5-month-dead agents signalled every session start) |
| (e) resource-contention | `resource-contention` | ≥2 live sessions hold a `resourceClaim` on the **same** resource string, **or** a lone live claim coexists with a `running` tracked job owned by a *different* session (CSO-21, § SACT). Both inputs are relayed, never re-detected: claim liveness is the CSI-07 stance verbatim, and job liveness is `background-job-status`'s own `pending[]` rule minus its own `stalled` heartbeat rule (`GC_JOB_STALL_SECONDS`, default 90) |

**Bounds on (e)**, declared because they shape what its silence means: a job whose `session_id` is `null` is excluded (*"a different session"* is not establishable from null); there is **no `cmd`→resource classifier** — whether a running `docker build` collides with a claim on `dev-stack` is judgment the woken Overseer does, and the detector relays both rows verbatim so it can; and **two unclaimed heavy jobs are not contention to this leg** — the claim is the anchor, the unclaimed case is `machine-quiescence`'s (anonymous by design, QUIESCE-02). So no signal here is *not* evidence of a quiet machine. Severity stays `warn` and the actuation ceiling stays `nudge`: pausing the second claimant would award the resource to the first, which is the winner arbitration DEC-96 D2 / DEC-240 rejected for advisory claims.

## Output schema (frozen)

```json
{
  "wake": true,
  "signals": [
    { "detectorId": "stall-monitor", "severity": "critical", "sessions": ["session-x"], "summary": "active worker session-x claudeShellPid 12345 is dead (terminal/stall)" }
  ],
  "summary": "1 airspace signal(s) -- wake Overseer",
  "briefing": "overseer-wake: 1 airspace signal(s) [stall-monitor(1)] -- run the Overseer (perceive -> advise) or /0-uldf-oracle overseer-wake"
}
```

- `wake` — boolean; `true` iff `signals` is non-empty (positive evidence only).
- `signals[]` — a compact preview `{detectorId, severity, sessions, summary}`. This is **not** the full Signal contract (schema 1 in `CSO_CONTRACTS.md`); full Signal normalization is the Stage-2 detector intake (C5). The wake oracle only decides *whether* to wake and gives a one-line preview of *why*.
- `summary` — one-line verdict. When `wake=false` it records which sources were *observed* vs. unavailable (so an all-clear is never confused with NO-DATA).
- `briefing` — **empty string when `wake=false`** (the session-start hook then emits no `[overseer-wake]` line — gracefully absent, same convention as `concurrent-mutation` / `stale-ltads-state`). Non-empty only when `wake=true`.

## Read-only contract

`run.sh` / `run.ps1` **never write**. The oracle reads the registry, the sibling oracles' stdout, shared-repo registries, and `touches.json` files. No baseline, no state mutation. (Contrast CSI-10, whose baseline is written by a *separate* `update-baseline` script — overseer-wake needs no baseline at all; every pass re-reads ground truth, honoring CSO-02 level-triggering.)

## Cross-shell parity

`run.sh` (bash) and `run.ps1` (PowerShell) produce **byte-identical** JSON on the same project tree. Parity is asserted by `validate.{sh,ps1}` and the `cso-overseer-wake-smoke.sh` harness (bash+PS legs). PowerShell gotcha guarded during the port: `$pid` aliases the read-only automatic `$PID`, silently breaking the stall PID probe — the port uses `$shellPid` / `$ProcId`.

## Files

| File | Purpose |
|---|---|
| `oracle.json` | Manifest (frozen output schema, cost provenance, fallback). |
| `run.sh` / `run.ps1` | The read-only aggregator (cross-shell parity). |
| `validate.sh` / `validate.ps1` | Self-test: schema-field presence + fixture-driven wake/clear behavior. |
| `README.md` | This file. |

## Lineage

CSO Phase 1, component C1. Authored 2026-06-26. Resolves Q-CSO-01 (wake economics). Cites DEC-98 (three-role split), DEC-99 (advise-only MVP), DEC-100 (disposable singleton). The aggregated detectors are pre-existing: CSI-10 (`concurrent-mutation`), SHARED-CSI-08 (`csi_shared_foreign_claims`), DEC-69 (`pods-monitor` stall), and the PODS `touches.json` conflict tiers.
