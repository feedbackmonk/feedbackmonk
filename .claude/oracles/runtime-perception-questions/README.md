# runtime-perception-questions oracle (ARIA-09)

**Kind**: project-state · **Lane**: fast · **Spec**: ARIA-09, ARIA-10, ARIA-22, ARIA-25 · **Doctrine**: `FOUNDATIONS/AGENT_OPERABLE_RUNTIME_DOCTRINE.md`

## What it answers

> *In this session, did the agent ask a human to run-and-report a runtime-perception question that an AOR surface should have answered itself?*

This is the **Leg-D substrate** of ARIA's four-leg detection machinery — the deterministic reader behind the finalize-time **Human-as-I/O-Relay** detection backstop (`/0-uldf-finalize` Phase 11.5, ARIA-10). The Leg-C *prevention* reflex (the standing rule in `~/.claude/CLAUDE.md` § "Agent-Operable Runtime (AOR)") asks the agent to **log** a runtime-probe candidate whenever it hits the anti-pattern; this oracle is what **reads** that log and surfaces the candidates.

## The marker log (the deterministic input)

Source of truth: a per-session append-only JSONL log at

```
.claude/session-state/aria-probe-candidates.jsonl
```

(gitignored — under `**/.claude/session-state/`). Each line is one runtime-probe candidate:

```json
{"schemaVersion":"1","ts":"2026-06-29T18:10:00Z","sessionId":"session-...","category":"async","question":"Did the live interpret round-trip compute the aggregate?","capability":"command-invoke:interpretLive","aria_could_answer":true,"surface_present":false}
```

| Field | Req | Notes |
|---|---|---|
| `schemaVersion` | rec | `"1"` at freeze. |
| `ts` | rec | ISO-8601 UTC when the relay occurred. |
| `sessionId` | opt | Writer's session id, from the `scripts/lib/aria-session-id.{sh,ps1}` ladder — the **same ladder this oracle reads with** (ARIA-25). `null`/absent ⇒ the writer could name no session, and the record is **out of scope for every identified session** (see below). |
| `sessionIdSource` | opt | Which rung named the writer: `env` \| `harness` \| `claude-pid` \| `none`. Makes the join method legible at triage time; added by ARIA-25. |
| `category` | req | `navigation` \| `errors` \| `async` \| `other` (anything else ⇒ `other`). |
| `question` | req | The runtime question the agent relayed through a human. Rows without a non-empty `question` are skipped. |
| `capability` | opt | The **concrete AOR verb** the gap names, e.g. `command-invoke:interpretLive`, `state-dump:lens`, `await-condition:aggregate`. This is what makes the candidate actionable (ARIA-22 acceptance: "naming a concrete capability"). |
| `suggested_endpoint` | opt | Proposed endpoint/handler name for an ARIA-expansion candidate. |
| `aria_could_answer` | opt | `true` when the question maps to an existing AOR/ARIA-07 capability; `false` when deeper instrumentation is needed. Defaults `false`. |
| `surface_present` | opt | Whether *any* AOR surface existed when the relay occurred (false ⇒ the app has no AOR surface at all). |

### How a line gets written (Leg-C reflex)

When the AOR reflex fires (the agent is about to ask a human to run-and-report and AOR lacks the capability), append one line — directly, or via the helper:

```bash
bash ~/.claude/scripts/aria/log-probe-candidate.sh \
  --category async \
  --question "Did the live interpret round-trip compute the aggregate?" \
  --capability "command-invoke:interpretLive" \
  --aria-could-answer true --surface-present false
```

The helper stamps `ts` + `sessionId` + `sessionIdSource` and guarantees a well-formed line. Writing the raw JSON line yourself is equally valid (the oracle skips malformed lines gracefully) — but then **you** own the labelling, and an unlabelled line is invisible to its own session (below).

## Session scoping — an unlabelled record is UNKNOWN, never MINE (ARIA-25)

Until 2026-08-12 the filter read *"when this session's id is known and the record carries a **non-null** sessionId, surface only this session's rows"* — so a record whose writer could not name itself matched **every** session. The writer could not name itself in any session where the framework had not exported `CLAUDE_SESSION_ID`, which on SessionHelm's 18-record specimen log is **every record since 2026-07-03** (11 of 18 — indices 1 and 9–18). Phase 11.5 was handed a growing pile of other sessions' relay events labelled as its own, correctly declined to promote them across four autopilot finalizes in nine days, and the mechanism promoted nothing. This is the textbook `asserts` / `measures` gap: the oracle asserted *"candidates this session"* and measured *"null-or-this-session candidates"*, and its self-test — written in the measurement's own vocabulary — **had no unlabelled-record cell at all**.

The rules now:

| This session's identity | An unlabelled (`null`) record | Output |
|---|---|---|
| resolves (any rung) | **excluded** — counted in `skippedUnlabelled` | `scoped: true` |
| resolves nothing | no filter runs at all | `scoped: false` + a briefing suffix naming it |
| `--all` / `ARIA_PROBE_SCOPE=all` | included (whole log) | `scoped: false`, `idSource: unscoped (--all)` |
| no python available (bash path) | included (count only, no parsing) | `scoped: false` |

Identity comes from `scripts/lib/aria-session-id.{sh,ps1}`, found on three search paths (the project's own `.claude/scripts/lib/`, the framework repo's template tree one level up, and `~/.claude/scripts/lib/`) — the same lib the writer uses, which is the only reason the two ends agree. The reader compares against the **whole alias set**, not one string (`self is a set`, DEC-337). If the lib is missing, both ends degrade to `CLAUDE_SESSION_ID` alone and the reader says so in `idSource`; a second inline ladder was deliberately not written, because two copies drifting apart *is* the defect.

**The legacy null backlog is descoped, not deleted.** `--all` reports it.

## Output

```json
{ "count": 2,
  "questions": [ { "question": "...", "category": "async", "aria_could_answer": true, "capability": "command-invoke:interpretLive", "surface_present": false } ],
  "scoped": true,
  "idSource": "env",
  "skippedUnlabelled": 11,
  "briefing": "runtime-perception: 2 human-relay probe candidate(s) logged (2 AOR-answerable) -- surfaced at /0-uldf-finalize Phase 11.5" }
```

- Empty / missing log ⇒ `{"count":0,"questions":[],"scoped":<bool>,"idSource":"...","briefing":""}` (the empty briefing suppresses any line). **NO-DATA, never a fabricated finding.**
- `skippedUnlabelled` appears only when non-zero. On a log written entirely after ARIA-25 it should be 0; a non-zero value means some writer could not name itself.
- `briefing` is ≤200 chars and carries an explicit `[WHOLE LOG — not scoped to this session: <idSource>]` suffix whenever `scoped` is false. **A consumer that reads the list while ignoring `scoped` re-creates DEFER-179.**
- Advisory only — this oracle **never** blocks a commit (CSI/Probandurgy advisory convention).

## Consumers

- **`/0-uldf-finalize` Phase 11.5** (ARIA-10) — the ARIA-Candidate Audit. Reads this oracle, classifies each candidate (ARIA-answerable / expansion candidate / out-of-scope), surfaces them, optionally records `[PROPOSED]` `type: aria-expansion-candidate` entries in `docs/specs/DISCOVERIES.md` at supervised+ autonomy.
- **`/0-uldf-oracle runtime-perception-questions`** — on-demand.
- **`/verify` and `/0-uldf-test`** — surface logged candidates after a run (the relay gap is a finding worth seeing where verification happens).

Not an every-session-start oracle (`consultation.typical_sessions_using` is `finalize`, so the briefing fan-out excludes it) — its surfacing home is finalize/verify, not session-start.

## Validation

`validate.sh` / `validate.ps1` build throwaway sandboxes and assert NO-DATA, empty-file, two-candidate parsing (with capability + question passthrough), malformed-line tolerance, and session-scoping — including the ARIA-25 cells: an unlabelled record must **not** surface to an identified session (must-NOT-fire), a real record from that session must **still** surface in the same run (the must-STILL-fire control that separates the fix from "stop reporting"), an unlabelled record must surface when nothing identifies this session, and `--all` must report the whole log.

Cross-twin, cross-engine and writer↔reader round-trip coverage lives one level up in `scripts/smoke-tests/aria-probe-session-scope-smoke.sh`, which drives both oracle twins and both logger twins over one shared fixture under every available PowerShell engine. **A passing self-test is not a trust verdict** (OVALID-03) — the pre-ARIA-25 self-test passed every one of its cells while the oracle it graded was reporting other sessions' rows as this session's.
