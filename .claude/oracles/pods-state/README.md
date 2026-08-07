# pods-state Oracle

## Synopsis

Project-state oracle answering "is a PODS session active here, who is on the roster, what is each worker's status, and what needs the LD's attention?" in one call. Come here for the frozen output schema and the monitor-parity parsing contract. Don't come here for machine-wide session discovery (`dispatchable-sessions`) or for driving the wait loop (`monitor-pods.{sh,ps1}` is the event source; this oracle is the point-in-time read).

## Question

One deterministic call replacing the 4-file agentic re-derivation (registry + status.md + alerts.md + messages.md + decisions.md) that the LD's coordination passes, converge Phase 2 readiness verification, and collab-sync each performed per consult (scrutiny 05 ADD-3; spec PODS-STATE-01).

## Frozen output schema

Programmatic consumers read exactly this shape; changing it is a spec-level event:

```json
{
  "active": true,
  "podsSession": "collab-20260702-000805",
  "sessionDir": ".claude/collaboration/collab-20260702-000805",
  "agents": [
    {
      "id": "CLAUDE-A",
      "role": "Descriptions & Trigger Surface",
      "status": "IN_PROGRESS",
      "progress": "10%",
      "isComplete": false,
      "spawned": true
    }
  ],
  "counts": { "total": 4, "complete": 1, "blocked": 0, "unspawned": 0 },
  "allComplete": false,
  "open": {
    "alerts": ["ALERT-001"],
    "messagesToLead": ["MSG-002"],
    "decisions": ["DEC-001"]
  },
  "monitor": { "pidFileExists": true, "pid": 51356, "live": true },
  "generatedAt": "2026-07-02T04:52:17Z"
}
```

Inactive form (no registry, `podsSession` null/absent, or session dir missing):

```json
{
  "active": false, "podsSession": null, "sessionDir": null, "agents": [],
  "counts": { "total": 0, "complete": 0, "blocked": 0, "unspawned": 0 },
  "allComplete": false,
  "open": { "alerts": [], "messagesToLead": [], "decisions": [] },
  "monitor": { "pidFileExists": false, "pid": null, "live": false },
  "generatedAt": "..."
}
```

## Which registry shape it reads (DEFER-096)

The registry's top-level `podsSession` is **canonically a STRING** — `"collab-YYYYMMDD-HHMMSS"`,
which is what `segments/-pods/parallelize_session.md` prescribes and what ten of this field's
eleven readers parse. A **dict** form (`{"sessionId": ..., "agents": [...]}`) also exists in the
wild: `csi_arc_conclude_eligibility` accepts it, an LD wrote it, and this oracle could not see
it — reporting `active:false` for a session the registry provably held (converge
collab-20260804-230108 Phase 2). Both shapes are read now, `sessionId` then `id`, and the
legacy shape is **never narrowed away**: registries are machine-local and are never synced or
committed, so dict-form files persist on other machines indefinitely.

Two properties of that extraction are load-bearing rather than incidental:

- **It is bounded to the `podsSession` value** by a brace walk that skips string literals. Not
  decoration: a `sessions[]` entry carries `sessionId`, `siblingGroup` and `podsWindow` values
  that name *real* collab ids, so an unbounded scan would cheerfully report a different live
  session's collab as this project's active one. `validate.{sh,ps1}` cell 7c is that control.
- **It reads the whole file, not per line.** A live registry is pretty-printed, so the key and
  its `sessionId` sit on different lines — a line-scoped matcher misses the dict form even
  after its pattern is widened.

`podsSession` in the OUTPUT is always a string, whichever shape it was read from.

Field semantics:

- `agents[]` comes from `channels/status.md` agent sections (`## CLAUDE-X | Role`); the LEAD section is excluded by design (roster = workers).
- `status` is upper-cased verbatim (`null` when the section has no Status line). `isComplete` uses the completion-synonym set `COMPLETE|COMPLETED|DONE|FINISHED`.
- `spawned` = `workers/<id>/shell.pid` exists OR status != PENDING.
- `open.alerts` = `## ALERT-*` blocks with `**Status**: ACTIVE`; `open.messagesToLead` = `## MSG-*` blocks with Status OPEN whose `**To**:` names LEAD or ALL; `open.decisions` = `## DEC-*` blocks with Status OPEN.
- `monitor` (additive, DEFER-005) reads the `<session>/monitor.pid` singleton record written by `monitor-pods.{sh,ps1}` at startup: `pidFileExists` (file present), `pid` (its numeric content, `null` if unparseable), `live` (best-effort process-existence check — advisory only; the authoritative duplicate defense is the monitor's own startup singleton check, and on Windows the PID is native so `run.ps1` is the primary reader of this field).

## Parsing parity contract

The status.md agent parser and channel scanners are behavior-locked to `scripts/monitor-pods.{sh,ps1}` (`parse_snapshot` / `scan_channel_file`): heading regex, tolerant `**Status**:` / `**Status:**` colon-inside-bold forms, upper-cased comparison, completion synonyms, spawned derivation, To:-LEAD/ALL matching. **A change to the monitor's parser changes this oracle in the same commit** (and vice versa) — they answer from the same ground truth and must not drift.

## Consumers

- The PODS LD, at any coordination pass (instead of reading 4 channel files raw).
- `/0-uldf-pods-converge` Phase 2 completion-readiness check (`allComplete`, `open.*` emptiness).
- `/0-uldf-pods-collab-sync` PODS mode Step 0 state read.

On-demand only — not wired into the session-start briefing fan-out (`typical_sessions_using` is not `"every"`): the question is PODS-session-scoped, not per-session orientation.

## Invocation

```bash
bash .claude/oracles/pods-state/run.sh
powershell -NoProfile -File .claude/oracles/pods-state/run.ps1
```

READ-ONLY; no JSON parser dependency (sed/awk / native PS string ops); gracefully absent on non-PODS projects.
