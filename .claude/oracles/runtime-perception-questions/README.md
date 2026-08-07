# runtime-perception-questions oracle (ARIA-09)

**Kind**: project-state · **Lane**: fast · **Spec**: ARIA-09, ARIA-10, ARIA-22 · **Doctrine**: `FOUNDATIONS/AGENT_OPERABLE_RUNTIME_DOCTRINE.md`

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
| `sessionId` | opt | Writer's session id (from `CLAUDE_SESSION_ID`). Enables session-scoping at read time; `null`/absent ⇒ unscoped. |
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

The helper stamps `ts` + `sessionId` and guarantees a well-formed line. Writing the raw JSON line yourself is equally valid (the oracle skips malformed lines gracefully).

## Output

```json
{ "count": 2,
  "questions": [ { "question": "...", "category": "async", "aria_could_answer": true, "capability": "command-invoke:interpretLive", "surface_present": false } ],
  "briefing": "runtime-perception: 2 human-relay probe candidate(s) logged (2 AOR-answerable) -- surfaced at /0-uldf-finalize Phase 11.5" }
```

- Empty / missing log ⇒ `{"count":0,"questions":[],"briefing":""}` (the empty briefing suppresses any line). **NO-DATA, never a fabricated finding.**
- When `CLAUDE_SESSION_ID` is set, the read is scoped to that session's rows (plus rows with no `sessionId`); unset ⇒ all rows.
- `briefing` is ≤200 chars. Advisory only — this oracle **never** blocks a commit (CSI/Probandurgy advisory convention).

## Consumers

- **`/0-uldf-finalize` Phase 11.5** (ARIA-10) — the ARIA-Candidate Audit. Reads this oracle, classifies each candidate (ARIA-answerable / expansion candidate / out-of-scope), surfaces them, optionally records `[PROPOSED]` `type: aria-expansion-candidate` entries in `docs/specs/DISCOVERIES.md` at supervised+ autonomy.
- **`/0-uldf-oracle runtime-perception-questions`** — on-demand.
- **`/verify` and `/0-uldf-test`** — surface logged candidates after a run (the relay gap is a finding worth seeing where verification happens).

Not an every-session-start oracle (`consultation.typical_sessions_using` is `finalize`, so the briefing fan-out excludes it) — its surfacing home is finalize/verify, not session-start.

## Validation

`validate.sh` / `validate.ps1` build throwaway sandboxes and assert NO-DATA, empty-file, two-candidate parsing (with capability + question passthrough), malformed-line tolerance, and session-scoping. Both prefer a working python for faithful parsing; the bash oracle degrades to a count-only result when no python is present (count honest, per-question detail NO-DATA).
