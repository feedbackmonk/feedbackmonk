# stale-ltads-state oracle

> Reflective leg of CSI Phase 1.6's three-leg session-end cleanup defense (CSI-12 hook + CSI-13 GC coupling + **CSI-14 this oracle**). Surfaces inconsistencies between `ltads/arc-state.json` (topmost arc) and `.claude/collaboration/active-sessions.json` that slipped through the proactive (SessionEnd) and reactive (GC sweep) defenses. Briefing line is gracefully absent when state is consistent.

## Purpose & Responsibilities

Detects the case where the topmost arc in `arc-state.json` has status `ACTIVE` / `PAUSED` but the matching registry entry says otherwise (closed, expired, missing, or PID-dead). Emits a `[stale-ltads-state]` line in the session-start ORACLE BRIEFING when an inconsistency exists, so the session has explicit signal to act on it.

**How it correlates** (DISC-HOOK-01 fix, 2026-06-01): the **arc owner's CSI sessionId** — recovered from the most-recent `**Mid-arc Checkpoint**: <ts>, by <sessionId>` line within the active (topmost) arc — is the `active-sessions.json` registry key. The LTADS `**ID**:` field (e.g. `S042`) is the *arc* id and is **never** a registry key, so it is not used. An active arc with zero recoverable checkpoint id (fresh, zero-finalize, or legacy prose-only checkpoints) degrades to `stale:false` — no false positive; the file self-heals on its next mid-arc finalize, which writes a canonical `, by <sessionId>` token (writer: `~/.claude/segments/-finalize/phase9-mid-arc-checkpoint.md`). DISC-CSI-11's "committed but never closed" target always has ≥1 mid-arc finalize, so the checkpoint exists for exactly the case this oracle must catch.

Trigger incident: GitCellar S002 (DISC-CSI-11) — a worker session committed B0..B3c stages successfully but the arc state stayed `ACTIVE` for a week because no closing command was invoked.

## File Index

| File | Purpose |
|---|---|
| `oracle.json` | Manifest — output schema, freshness contract, fallback instructions, provenance |
| `run.sh` | Unix oracle entry point. Reads arc-state.json + registry, classifies inconsistency, emits JSON |
| `run.ps1` | Windows oracle entry point. Same contract as run.sh |
| `validate.sh` | Unix self-test — invokes run.sh, validates JSON shape against oracle.json schema |
| `validate.ps1` | Windows self-test |

Smoke harnesses (real-format fixtures + cross-shell parity) live in `~/.claude/scripts/csi-tests/csi-14-smoke.{sh,ps1}` (14/14 each).

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
  "briefing": "arc-state.json topmost arc: ACTIVE (arc owner stale-session-from-april) but registry shows entry as EXPIRED ..."
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
| `ltads/sessions/current-session.md` | Source of the active-arc **Status** field (canonical `**Status**:` / `- **Status**:` / `## Status:` forms, value-at-EOL anchored to exclude Arc-Terminus prose) and the **arc-owner CSI sessionId** (most-recent `**Mid-arc Checkpoint**: <ts>, by <sessionId>` within the topmost arc) |
| `.claude/collaboration/active-sessions.json` | Source of registry status + PID for liveness check; keyed by `.id` == the arc-owner sessionId |
| `awk` | Locale-independent status-field + checkpoint-id parsing (MSYS/Git-Bash `grep -P` rejects non-UTF-8/empty locales, so `-P` is avoided) |
| `kill -0` (Unix) / `Get-Process` (Windows) | PID liveness probe for the `registry-pid-dead-state-active` classification |
| `jq` (preferred) or `python` (fallback) | JSON parsing for the registry. Python fallback probe-verifies (Microsoft Store stub on Windows is silently non-functional) |

| Consumed by | Where |
|---|---|
| `~/.claude/hooks/session-start.{sh,ps1}` | Auto-discovered via oracle manifest; emitted in ORACLE BRIEFING when `briefing` field is non-empty |

## Decision Log

- **DEC-44**: ships as a standalone oracle, not as an extension to `ltads-state`. Rationale: separation of concerns (`ltads-state` reports current state; `stale-ltads-state` reports inconsistency between state and registry); cleaner gracefully-absent contract; matches existing convention of one briefing-line emitter per oracle (`coordination`, `handoff-scope`, `shared-repo-coordination` are all standalone).
- **Inconsistency taxonomy** (`inconsistency_kind` enum): `registry-closed-state-active` | `registry-expired-state-active` | `registry-pid-dead-state-active` | `registry-missing-state-active` | `none`. Frozen by CSI-14 spec; new variants require a spec extension.
- **PID liveness only checked when `registry_status=="active"`**: when the registry already shows `closed`/`expired`, the GC has already determined the PID is dead — re-probing wastes the budget. The PID-alive field is `null` in those cases.
- **Correlation key is the arc-owner CSI sessionId from the Mid-arc Checkpoint, not the LTADS `**ID**:`** (DISC-HOOK-01, 2026-06-01). The original implementation parsed a `Session:` field (status) and `^Status:` (plain) that do not exist in canonical `current-session.md` files — it read nothing on every real file and always returned `stale:false` (dead in production). The fix: (1) read the canonical `**Status**:` field in all three documented line forms, EOL-anchored to exclude Arc-Terminus prose; (2) recover the registry key from the most-recent `**Mid-arc Checkpoint**: <ts>, by <sessionId>` of the active arc — empirically the only owner-id any real file carries, and the LTADS `**ID**:` (`S042`) is the *arc* id which is never a registry key. This made a **writer-side change load-bearing**: the finalize Mid-arc Checkpoint writer must always emit the `, by <sessionId>` token (real agents had been writing prose-only checkpoints that dropped it). The design was chosen over a workDir-liveness correlation because id-correlation self-resolves the compaction case (the owner's own live registry entry → consistent) with no self-exclusion edge. Rationale chain: DISC-HOOK-01 forward-use; this oracle's `run.{sh,ps1}`; writer `~/.claude/segments/-finalize/phase9-mid-arc-checkpoint.md`; prose-era schema retired with the parser lib (ARC-04; successor: `~/.claude/templates/ARC_STATE_SCHEMA.md` § Checkpoint object).
- **awk over `grep -P` for parsing**: MSYS/Git-Bash `grep` rejects `-P` under empty/non-UTF-8 locales (`grep: -P supports only unibyte and UTF-8 locales`), so the status and checkpoint matchers use locale-independent `awk` rather than the PCRE `\K`/lookahead form used by `session-detect.sh`. Same three-form + value-at-EOL semantics, more portable.
