# stale-ltads-state oracle

> Reflective leg of CSI Phase 1.6's three-leg session-end cleanup defense (CSI-12 hook + CSI-13 GC coupling + **CSI-14 this oracle**). Surfaces inconsistencies between `ltads/sessions/current-session.md` and `.claude/collaboration/active-sessions.json` that slipped through the proactive (SessionEnd) and reactive (GC sweep) defenses. Briefing line is gracefully absent when state is consistent.

## Purpose & Responsibilities

Detects the case where `current-session.md` Status is `ACTIVE` / `PAUSED` / `IN_PROGRESS` but the matching registry entry says otherwise (closed, expired, missing, or PID-dead). Emits a `[stale-ltads-state]` line in the session-start ORACLE BRIEFING when an inconsistency exists, so the session has explicit signal to act on it.

Trigger incident: GitCellar S002 (DISC-CSI-11) — a worker session committed B0..B3c stages successfully but `current-session.md` stayed `Status: ACTIVE` for a week because no closing command was invoked.

## File Index

| File | Purpose |
|---|---|
| `oracle.json` | Manifest — output schema, freshness contract, fallback instructions, provenance |
| `run.sh` | Unix oracle entry point. Reads current-session.md + registry, classifies inconsistency, emits JSON |
| `run.ps1` | Windows oracle entry point. Same contract as run.sh |
| `validate.sh` | Unix self-test — invokes run.sh, validates JSON shape against oracle.json schema |
| `validate.ps1` | Windows self-test |

## Public API & Usage

```bash
# Unix
.claude/oracles/stale-ltads-state/run.sh

# Windows
powershell -ExecutionPolicy Bypass -File .claude/oracles/stale-ltads-state/run.ps1
```

Emits single-line JSON:

```json
{
  "stale": true,
  "details": {
    "current_session_status": "ACTIVE",
    "current_session_id": "stale-session-from-april",
    "registry_status": "expired",
    "registry_pid_alive": null,
    "inconsistency_kind": "registry-expired-state-active"
  },
  "briefing": "current-session.md Status: ACTIVE (session stale-session-from-april) but registry shows entry as EXPIRED ..."
}
```

When `stale=false`, the `briefing` field is the empty string and the session-start hook emits no line for this oracle (parallel to `dispatchable-sessions`'s empty-result silence).

## Constraints & Business Rules

- **Always-fresh, no caching**: each invocation reads both files at briefing-time (~60ms budget). Caching would defeat the purpose — the inconsistency may have been introduced since the last cache.
- **Read-only**: the oracle never mutates state. Reconciliation is the job of CSI-12 (SessionEnd hook) and CSI-13 (GC sweep). This oracle only *reports*.
- **Graceful absence on every input failure path**: missing current-session.md, missing registry, unreadable file, no JSON parser available — all emit a consistent `stale=false` payload rather than throwing.
- **Status filter**: only `ACTIVE` / `PAUSED` / `IN_PROGRESS` warrant the inconsistency check. `CONCLUDED` / `BROKEN` / etc. are terminal states and not subject to staleness.

## Relationships & Dependencies

| Depends on | Why |
|---|---|
| `ltads/sessions/current-session.md` | Source of state Status + Session id |
| `.claude/collaboration/active-sessions.json` | Source of registry status + PID for liveness check |
| `kill -0` (Unix) / `Get-Process` (Windows) | PID liveness probe for the `registry-pid-dead-state-active` classification |
| `jq` (preferred) or `python` (fallback) | JSON parsing for the registry. Python fallback probe-verifies (Microsoft Store stub on Windows is silently non-functional) |

| Consumed by | Where |
|---|---|
| `claude-template/hooks/session-start.{sh,ps1}` | Auto-discovered via oracle manifest; emitted in ORACLE BRIEFING when `briefing` field is non-empty |

## Decision Log

- **DEC-44**: ships as a standalone oracle, not as an extension to `ltads-state`. Rationale: separation of concerns (`ltads-state` reports current state; `stale-ltads-state` reports inconsistency between state and registry); cleaner gracefully-absent contract; matches existing convention of one briefing-line emitter per oracle (`coordination`, `handoff-scope`, `shared-repo-coordination` are all standalone).
- **Inconsistency taxonomy** (`inconsistency_kind` enum): `registry-closed-state-active` | `registry-expired-state-active` | `registry-pid-dead-state-active` | `registry-missing-state-active` | `none`. Frozen by CSI-14 spec; new variants require a spec extension.
- **PID liveness only checked when `registry_status=="active"`**: when the registry already shows `closed`/`expired`, the GC has already determined the PID is dead — re-probing wastes the budget. The PID-alive field is `null` in those cases.
