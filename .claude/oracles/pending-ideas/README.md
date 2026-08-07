# pending-ideas oracle

**Kind**: project-state · **Question**: *Does this project have un-triaged injected/deferred ideas (`status: PROPOSED`), or deliberately-**PARKED** open work, waiting in `docs/planning/deferred/` — and is any brief carrying a status word this oracle does not recognize, so it is being dropped without anyone seeing it?*

The feeder-side surfacing leg of `/0-uldf-inject` (INJECT-08). Deposited/deferred idea briefs land in `docs/planning/deferred/`; without this oracle they are **not** auto-surfaced, so a target project would not notice an injected idea until a human went looking. This oracle emits a `[pending-ideas]` session-start briefing line so even a *manually*-started target session notices pending ideas without the human re-driving.

Auto-admitted by the session-start briefing fan-out via `typical_sessions_using: "every"` — **no hook edit** required.

## Frozen output schema (INJECT-08, additively widened twice: DEC-281, DEC-317)

Single-line JSON:

```json
{
  "pending_count": 2,
  "items": [
    {"id": "DEFER-007", "title": "Repo picker remembers last-used sort", "origin": "inject", "injected_at": "2026-07-08T21:00:00Z", "source_project": "GitCellar"}
  ],
  "parked_count": 1,
  "parked": [
    {"id": "DEFER-003", "title": "AOR-drive re-arm at 0 of 10", "re_arm": "R2 reaches 10 tracked sessions"}
  ],
  "unknown_status_count": 1,
  "unknown_status": [
    {"id": "DEFER-table-20260806_stale-grant", "status": "OPEN"}
  ],
  "no_data": false,
  "briefing": "2 pending ideas: DEFER-007 ...; 1 parked item (triaged, open, not being built): DEFER-003 AOR-drive re-arm at 0 of 10 (re-arm: R2 reaches 10 tracked sessions); 1 item with an unrecognized status word (not surfaced; surfaced words are PROPOSED and PARKED): DEFER-table-20260806_stale-grant (OPEN)"
}
```

| Field | Meaning |
|---|---|
| `pending_count` | Count of `status: PROPOSED` items. |
| `items` | `{id, title, origin, injected_at, source_project}` for **PROPOSED items only**. `TRIAGED` / `IN-PROGRESS` / `COMPLETED` / `DISMISSED` are excluded. `id` is never `""` — the ladder is filename `DEFER-<n>` → front-matter `id:` → basename (DEC-317). |
| `parked_count` | Count of `status: PARKED` items — triaged, **still open**, deliberately not being built. Deliberately **not** folded into `pending_count`. |
| `parked` | `{id, title, re_arm}` for PARKED items. `re_arm` is the declared condition verbatim, or `""` when none was declared. Read from `re-arm:`/`re_arm:`, falling back to a `RE-ARM -- <text>` written into the status rationale (DEC-317). |
| `unknown_status_count` | Count of briefs whose status **word** is neither `PROPOSED`/`PARKED` nor a recognized *done*-word (DEC-317). **Never** folded into `pending_count` — see below. |
| `unknown_status` | `{id, status}` for those briefs, status word verbatim. |
| `no_data` | `true` when the deferred dir is unreadable, or a `DEFER-*.md` file has no parseable status — **NO-DATA honesty**, never a silent "none". An **unrecognized** word is *not* `no_data`: the status was read fine, the vocabulary disagrees. |
| `briefing` | Pre-formatted line for the session-start ORACLE BRIEFING. **Empty** when there is no `deferred/` dir, or zero PROPOSED **and** zero PARKED **and** zero unrecognized-status items (the fan-out suppresses the line — no `0`-noise). Otherwise: the pending clause, then the parked clause, then the unrecognized clause, then any NO-DATA note. |

## The status **word**, and the word nobody knows (DEC-317, `DEFER-151`)

**The status word is the first token of the status line, not the whole line.** This oracle
used to uppercase the entire line, strip **all** whitespace, and compare the result to
`PROPOSED`/`PARKED` exactly. The corpus does not write it that way: measured 2026-08-06
over 160 `DEFER-*.md`, **28 carry a rationale after the word** (`status: PARKED 2026-08-06
-- the fix is SHIPPED (...); RE-ARM -- ...`), which is house style. Those collapsed to a
single ~400-character token matching nothing, and the brief disappeared with no trace —
including `DEFER-146` and `DEFER-147`, both PARKED with a declared re-arm, i.e. exactly
what `parked[]` above exists to show. Removing the rationale and changing nothing else
surfaced them.

The cut is the leading `[A-Za-z0-9_-]+` run, which handles all three observed rationale
forms (space, em-dash, open-paren) and keeps `RESOLVED-DECLINED` / `IN-PROGRESS` /
`PHASE-1-IMPLEMENTED` whole.

**A word outside the set is reported, not dropped — and never promoted.** The trigger was a
live cross-project injection from Table carrying `**Status**: OPEN` — the framework's own
word for an open `DISCOVERIES.md` entry, so the injector was following house style. It was
invisible here **and** skipped by `dec-alloc-guard --check-defer`, so the one open injection
in the corpus was invisible to both the audit that exists to catch invisible briefs and the
surface that exists to show pending work; a hand census found it. *A file this oracle
silently drops is indistinguishable from a file that is not there* — ORACLE-COST-09/10
(DEC-292), one door over.

- **The filter is correct and is NOT widened.** `pending_count`/`items[]` stay
  PROPOSED-only (INJECT-08, frozen) and `PARKED` stays out of them (DEC-281). An
  unrecognized word joins **neither**; it rides its own fields. `validate.{sh,ps1}` case 9b
  pins that from the other side, and case 8's `RESOLVED <rationale>` control is what
  separates a leading-token parse from a match-any-prefix one that would light up 105
  finished briefs.
- **Recognized *done*-words stay silently invisible**, which is correct. The set —
  `RESOLVED`, `RESOLVED-DECLINED`, `IMPLEMENTED`, `PHASE-1-IMPLEMENTED`, `COMPLETED`,
  `TRIAGED`, `IN-PROGRESS`, `DISMISSED`, `CLOSED`, `SUPERSEDED`, `DECLINED`, `APPLIED`,
  `FOLDED` — is **derived from a census of the corpus, not from memory**. Re-derive before
  editing it: the first hand-typed version dropped `PHASE-1-IMPLEMENTED` and `case6`
  reddened on the first run.
- **Why not a frozen CSURF member table?** Asked and answered NO in DEC-317. CSURF-07 /
  DEC-275 detects drift between *declared sites*; this failure was a word arriving from
  *outside* — the writers are other projects injecting briefs. A frozen table binds a
  document, not that population. The remedy belongs at the consumer, where it is also
  falsifiable: `case9` reddens if the reporting stops working, and a member table cannot
  redden.

## `PARKED` — the inverted gap (DEC-281, `DEFER-109`)

`PROPOSED` means *un-triaged*. So surfacing only `PROPOSED` meant that **triaging a brief and parking it with a re-arm condition — the documented, correct workflow — was also what switched its visibility off, permanently.** A brief that is genuinely still open but sits at `TRIAGED` is named at no session start, ever. The corpus's own history is the evidence: 27 briefs sat at `TRIAGED` and 21 of them had shipped long ago — the label is where items go to stop being looked at.

`PARKED` = *triaged, deliberately not being built, with a stated re-arm condition.* It is **not** a spelling of done and **not** a rename of `TRIAGED`; nothing in this oracle relabels a brief.

Three design decisions worth knowing before extending it:

- **It rides its own fields, not `pending_count`.** INJECT-08 freezes `items` as PROPOSED-only and *un-triaged*, and a parked item is by definition triaged — so folding it in would silently **redefine** a frozen field where a new field merely **adds** one. The single clause deliberately widened is graceful absence: the briefing is now empty when there is nothing PROPOSED **and** nothing PARKED. Its stated purpose (*never a `0`-noise line*) is preserved exactly — a parked item is not noise, it is the thing being surfaced.
- **The re-arm condition is checked for PRESENCE and never evaluated.** Whether a condition is *satisfiable* is a judgment question; OVALID-02's precedent is that only the presence of a declaration is mechanizable, and the doctrine says so out loud. A parked brief with no declared condition is still surfaced, and reported as having none — that absence is itself worth seeing.
- **There is deliberately no stale-`PARKED` signal.** A "this has been parked too long" counter is precisely the drive-to-zero ritual OVALID's `bounded` verdict was designed to avoid: it would train agents to unpark items to clear a number. Parked-with-a-reason is a healthy steady state.

**Not done here, on purpose**: the corpus's eight spellings of *done* were **measured** to cost this oracle nothing — it tests exactly one value, so `RESOLVED`/`IMPLEMENTED`/`COMPLETED`/… are all equally "not `PROPOSED`". Normalising them is a cosmetic rename with zero machine consumers (`status:` has exactly ONE runtime consumer: this oracle). Bundling it would have blurred the one change that has a measured motivation.

**Briefing constraint**: the session-start fan-out extracts `briefing` with a no-double-quote regex, so the `briefing` string contains **no double-quote** — arrows (`<-`), parens, and semicolons only. Both `run.sh` and `run.ps1` hand-assemble JSON to stay byte-parallel.

## Behavior contract

- **Gracefully absent**: no `docs/planning/deferred/` directory, or zero PROPOSED **and** zero PARKED **and** zero unrecognized-status items ⇒ empty `briefing` (line suppressed). Works whether or not the project is otherwise ULDF-instrumented.
- **NO-DATA honesty**: a present-but-unreadable `deferred/` dir, or a `DEFER-*.md` whose status cannot be parsed ⇒ `no_data: true` + a NO-DATA briefing. Never reports "none" when it could not actually confirm none.
- **Format robustness**: recognizes both the INJECT-02 YAML front-matter (`status:`) and the legacy DEFER-era body form (`**Status**:`), so consolidating `/0-uldf-ldis-defer` into `/0-uldf-inject` introduces no regression on pre-existing deferred items. <!-- path-ok: retired skill named for lineage (DEC-149) -->
- **Freshness**: `always-fresh` — presence/mtime of `deferred/` + the `status` marker; deterministic, no cache.
- Only files matching `DEFER-*.md` are considered. Other files (e.g. a `README.md` in `deferred/`) are ignored, never counted as malformed.

## Files

| File | Purpose |
|---|---|
| `oracle.json` | Manifest (frozen output schema, `typical_sessions_using: every`). |
| `run.sh` / `run.ps1` | The detector (byte-parallel). Read-only. |
| `validate.sh` / `validate.ps1` | Self-tests, 18 checks each (PROPOSED-fires / TRIAGED-excluded / empty-suppressed / malformed-NO-DATA / PARKED-surfaces / PARKED-stays-out-of-`items` / no-re-arm-named-as-such / **11 done-spellings stay hidden** / composition ordering / briefing stays quote-free / **status-word parse** / re-arm-from-rationale / hyphenated-done-word / **unrecognized-word reported** / unrecognized-not-promoted / done-words-stay-quiet / non-empty `id`). `validate.ps1` drives `run.ps1` with the **hosting** engine, so `pwsh validate.ps1` actually exercises pwsh (TWIN-02). |

The `case6` cell is the one to preserve if any of these is ever trimmed: **a `PARKED` surface that also lights up for finished briefs is worse than no surface**, because it trains agents to ignore the lane. That is exactly what happened to `TRIAGED`, and it is why the acceptance criterion demanded that half be shown red-first.

The framework's own `dec-alloc-smoke.sh` § 9 additionally drives **both twins over one fixture set** and requires byte-identical JSON.

## Provenance

INJECT-08 + INJECT-18 (§ INJECT, `docs/specs/SPECIFICATION.md`). Decisions DEC-148 / DEC-149 / **DEC-281** (the `PARKED` surface; brief `DEFER-109`) / **DEC-317** (INJECT-20 — the status word is parsed, and an unrecognized one is reported; brief `DEFER-151`). A deliverable of the `/0-uldf-inject` feature, not a pre-build-for-workers oracle.

**Sibling gate.** This oracle's parse *defines* what "readable" means for a deferred brief, and `dec-alloc-guard.{sh,ps1}` re-implements it — to grade a brief at claim time (`rc 4`) and block an unreadable one at commit time (DEC-281 / `DEFER-110`). The duplication is deliberate: this oracle is installed **per-project** under `.claude/` while the guard runs from the framework source tree, which may not have been synced since either was edited — so *calling* this oracle would make the gate's verdict depend on which copy happened to be deployed. It is pinned against drift by `dec-alloc-smoke.sh` § 8b, which drives both over one fixture set and requires they name the same files. **If you widen this parse, that cell goes red — widen the guard's with it.**
