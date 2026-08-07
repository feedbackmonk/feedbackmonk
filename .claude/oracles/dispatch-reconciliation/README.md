# `dispatch-reconciliation` — did those unconfirmed dispatches ever land?

**Kind**: Verification Oracle · **Pack**: starter · **Spec**: DISPATCH-12 · **Decision**: DEC-230
**Not in the session-start briefing** — on-demand only.

## The question

`/0-uldf-dispatch` reports `DELIVERED-UNCONFIRMED` when a target did not stamp a submission receipt. Until DEC-230 that verdict was **permanent NO-DATA**: `dispatch-log.jsonl` recorded the attempt, and nothing ever came back to ask *"did it eventually land?"*

On 2026-08-02 a session had to settle exactly that question mid-incident, and did it by **hand-reading `.claude/session-state/turn-state/*.json`** — a deterministic join over two files already on disk, performed by an agent burning context. That is the textbook Oraculurgy trigger, and this oracle is the answer.

## What it measures

For each non-confirmed entry in `dispatch-log.jsonl` inside the lookback window, it compares the logged dispatch `ts` against the target's current `turn-state/<resolvedId>.json` stamp.

| Verdict | Condition |
|---|---|
| `ACTIVITY-AFTER-DISPATCH` | the target's newest stamp is **newer** than the dispatch |
| `NO-ACTIVITY-SINCE` | no turn transition since the dispatch |
| `NO-DATA` | no turn-state file for that target |

## Why not `CONFIRMED-LATE`

The originating brief (DEFER-058) proposed exactly that label. **It overclaims, and the weaker vocabulary is a deliberate decision.**

TSTATE stores only the target's **latest** transition. A stamp newer than the dispatch therefore proves the target took *a* turn afterwards — it can never prove the turn was *ours*. A user typing something unrelated in that window is indistinguishable. The brief was itself honest about this in its evidence table ("a user typing something unrelated would look identical"); adopting `CONFIRMED-LATE` would have thrown that honesty away at the exact point it got encoded into a machine-readable label that other agents then trust.

`ACTIVITY-AFTER-DISPATCH` says what was measured. Read it as *consistent with consumption*, never as proof.

This is the OVALID-01 discipline in its normal form: `asserts` and `measures` are written adjacent in `oracle.json`, and every place they diverge is a declared `known_gap` rather than silence. There are eight of them. Read them before citing a number from this oracle.

## Why FAIL is narrow

`status: fail` requires **all four**:

1. `reason == receipt-timeout` — an **idle** target that did not submit. Never `queued-behind-active-turn`: a non-receipt from a mid-turn target is the *expected* result (DISPATCH-11), and grading it would re-import the very busy/silent conflation this whole change removed.
2. Inside the lookback window (`ULDF_DISPATCH_RECONCILE_HOURS`, default 24).
3. **The target is still live** in the registry, with a live PID.
4. `NO-ACTIVITY-SINCE`.

Legs 2 and 3 are what make the lane **self-clearing**. A prompt cannot strand a session that has exited, and a gate that can never go green is a gate agents learn to skip — the QUIESCE lesson, applied before it could bite here.

The consequence, stated plainly because it bounds every reading of the number: `stranded_actionable` is an **actionability** count, not an **incidence** count. It systematically under-reports historical stranding, on purpose.

## Not in the briefing

Deliberate, same judgment as `retirement-candidates`. This lane is empty on almost every run; a briefing line that is usually empty and occasionally stale trains agents to skip it. Consult it when you have an unconfirmed dispatch you care about:

```
/0-uldf-dispatch --reconcile
/0-uldf-oracle dispatch-reconciliation
```

## Exit codes

`0` pass · `1` at least one actionable stranding · `2` no dispatch log (NO-DATA)

## Scope caveat worth repeating

A **cross-project** dispatch (`--project=<path>`) logs into the **target** project's `session-state`. Reconciling it means running this oracle **there**. Run here, it will not see the attempt — and cannot say so, because an absent row is indistinguishable from a dispatch that was never sent.

## Tests

`~/.claude/scripts/smoke-tests/dispatch-reconciliation-smoke.sh` — 8 cells, both twins over one shared sandbox (TWIN-01).

Two properties the harness deliberately enforces:

- **Both directions.** A one-directional harness ("stranded → fail") is satisfiable by a constant `exit 1`. Every grading cell has a partner that must stay green.
- **The FAIL cell needs a genuinely OS-visible live PID.** On Windows a bash `$$` is invisible to `Get-Process`, so using it would leave the fail path silently unprovable while the suite printed all-passed. The harness resolves a real PID and **SKIPs loudly** if it cannot — a skip is not a pass.
