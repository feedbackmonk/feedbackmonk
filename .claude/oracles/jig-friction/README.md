# jig-friction oracle

**Spec**: JIG-04 · **Decision**: DEC-142 · **Kind**: `verification` (Oraculurgy over an existing substrate) · **Lane**: fast · **Consulted**: every session (briefing fan-out)

The **"manufactured boredom" signal** of the Development Jigs arc — a deterministic
detector that gets *annoyed on the agent's behalf*. It spots **grinding**: an agent
hand-repeating N structurally-similar invocations within one session instead of
building a **Development Jig** (a Fabricator, scenario-replayer, oracle, …) that
would do the work once. Each firing is surfaced as an advisory, evidence-carrying
briefing line so the agent can judge in a glance and route the recognition to the
demand log (JIG-06) — never an inline build (JIG-07).

**Advisory only.** This oracle never blocks a commit or a session (DEC-142). It
informs; the finalize retrospective jig audit (JIG-05) is the hindsight backstop.

---

## Substrate — no new store (RECENCY precedent)

Reads the **existing** `command-usage/*.jsonl` registry written by
`hooks/command-usage-tracker.{sh,ps1}` — one JSONL record per Skill / Task / MCP
invocation:

```json
{"at":"2026-07-05T12:00:08","cmd":"mcp:playwright/browser_take_screenshot","type":"mcp","session":"<id>","project":"<name>","machine":"<host>","arg1":"<subaction?>","flags":["<flag?>"]}
```

> **Note on the signature.** `command-usage` records the invocation **NAME**
> (`mcp:playwright/browser_take_screenshot`, `/0-uldf-ltads-admin`, …), **not a raw
> Bash command line** — the tracker instruments Skill/Task/MCP tool calls only. So
> the canonical grinding signature is **N identical/similar NAMED invocations in
> one session**: a screenshot loop (`browser_take_screenshot` ×6), a repeated
> navigate→evaluate sequence (each step recurs ≥ threshold), a hand-repeated skill
> subaction. (Fuzzy content-similarity over hand-authored Write/Edit inputs is a
> *different*, fuzzier signal — deferred to the catalog + finalize audit per
> Q-JIG-01, where hindsight judgment is cheap.)

## Data-directory resolution (first hit wins)

Friction is a **live-session** question, so the machine's live registry is
**primary** — this deliberately diverges from `review-recency` (a *historical*
question that reads the `./claude-usage` repo aggregate first):

1. `--dir <path>` (explicit; the smoke uses this)
2. `~/.claude/command-usage` (this machine's live registry — the current session lives here)
3. `./claude-usage` (repo cross-machine aggregate — dogfood fallback)

## Scope — the "session window" (first hit wins)

1. `--session <id>` (explicit)
2. `$CLAUDE_SESSION_ID` (the **current** live session — the production briefing path)
3. most-recent session in the data (deterministic fallback; used for single-session fixtures)

`windowMinutes` (config, default `0` = whole session) optionally tightens scope to
the last N minutes of that session's activity.

> At session **start** the current session has ~no records yet → the oracle is
> silent (correct: no grinding *yet*). It becomes useful **mid-session** — consult
> on demand via `/0-uldf-oracle jig-friction` once tool calls have accumulated, or
> let the finalize audit (JIG-05) catch it in hindsight.

---

## Frozen output schema (GUIDE § 6.1)

Single-line JSON. **Consumers (CLAUDE-C audit, CLAUDE-G inventory) read this verbatim.**

```json
{
  "status": "ok | signal",
  "signals": [
    {
      "pattern": "<normalized shape signature>",
      "count": 6,
      "evidence": ["<at> <cmd> [<arg1>] [<flags>]", "..."],
      "suggestedArchetype": "<frozen catalog slug>"
    }
  ],
  "briefing": "<string>"
}
```

- **`pattern`** — the normalized shape signature: `type|cmd|arg1|flags`, lowercased,
  digit-runs → `N`, trailing empty fields trimmed. Shape-equality clustering (not
  edit-distance) — sufficient for v1 (Q-JIG-01); `cmd` is already a clean name so
  volatile-token stripping mostly matters for `arg1`.
- **`count`** — cluster size (the number of structurally-similar invocations).
- **`evidence`** — the **verbatim** invocations (what lets the agent judge in one
  glance). Each truncated to ~200 chars; the list is capped at 20 with a
  `"...(+K more)"` tail when the cluster is larger (`count` is still the true total).
- **`suggestedArchetype`** — a slug from the **frozen** jig-catalog list
  (`fabricator, state-fabricator, scenario-replayer, log-distiller,
  diff-summarizer, environment-resetter, corpus-harvester`), chosen by a keyword
  heuristic on the representative `cmd`; when unsure it emits the closest match
  (default `scenario-replayer` for a generic replayed loop) — **never an invented slug**.
- **`briefing`** — the ORACLE-BRIEFING line (the session-start hook prepends the
  `[jig-friction]` tag). **QUIET-PATH INVARIANT: `status:"ok"` ⇒ `briefing == ""`**
  — an empty briefing is suppressed by the fan-out (RECENCY-04 precedent; no
  session-start hook edit needed). The briefing string carries no embedded
  double-quotes (the hook's extraction regex is `"[^"]*"`).

---

## Q-JIG-01 resolution (v1 detector set + thresholds)

Resolved at Stage-1 build against the fixture corpus in
`~/.claude/scripts/smoke-tests/fixtures/jig-friction/`:

- **v1 detector = command-similarity clustering only** — normalized **shape-equality**
  (chosen over edit-distance: shape-equality passes the whole corpus, so edit-distance
  would be over-engineering). Covers the screenshot-loop and repeated-multi-step-sequence
  candidates (each sequence step recurs ≥ threshold → clusters).
- **`minRepeats: 5` (HIGH by design), session-scoped, briefing-line surfacing.**
  The **high-threshold bias** is deliberate: **EPP-02 bounded-false-positive applies
  to the recognizer itself** — a nagging detector is *attention pollution*, the very
  resource Oraculurgy defends. Prefer **missed-detection over false-fire**; the
  finalize audit (JIG-05) is the backstop for what this misses. The negative corpus
  MUST stay silent (a trivially-always-firing detector fails the smoke's negative leg).
- **Deferred**: fuzzy content-similarity over Write/Edit (hand-authored test-input
  detection) — fuzzier, and hindsight-cheap in the finalize audit.

## Config (`.claude/config.json` → `jigFriction`)

| Key | Default | Meaning |
|-----|---------|---------|
| `minRepeats` | `5` | Cluster size at/above which a repetition fires. Lower **only** with evidence that real grinding is being missed. |
| `windowMinutes` | `0` (whole session) | Optional tightening — when `>0`, only invocations within the last N minutes of the scoped session count. |

Precedence: CLI (`--min-repeats` / `--window-minutes`) > `.claude/config.json` `jigFriction.*` > default.

## Graceful absence & read-only invariant

- Missing `jq`, missing registry (NO-DATA), or any internal failure → the oracle
  emits the quiet JSON `{"status":"ok","signals":[],"briefing":""}` and exits `0`.
  A briefing oracle must **never** crash the session-start fan-out (a NO-DATA note
  goes to stderr for on-demand visibility).
- **Read-only** (ORACULURGY Part 11 § 11.3.4): `run.{sh,ps1}` never writes — no
  baseline, no store. Detection is pure function of the existing telemetry.

## Files

| File | Purpose |
|------|---------|
| `oracle.json` | Manifest (kind `verification`, every-session, fast lane, frozen schema + config block) |
| `run.sh` / `run.ps1` | The detector (byte-parity output across shells) |
| `validate.sh` / `validate.ps1` | Self-test — asserts the signal schema + the quiet-path invariant on a synthesized in-temp corpus |
| `../../scripts/smoke-tests/jig-friction-smoke.sh` | Behavioral smoke (positive fires, **negative stays silent**, boundary both directions, config override, malformed robustness, session isolation, NO-DATA, PS parity) |
| `../../scripts/smoke-tests/fixtures/jig-friction/*.jsonl` | The fixture corpus |
