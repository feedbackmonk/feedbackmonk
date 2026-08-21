# jig-demand oracle

**Spec**: JIG-09 · **Decision**: DEC-362 · **Discovery**: DISC-JIG-02 · **Kind**: `verification` · **Lane**: fast · **Consulted**: every session (briefing fan-out) · **Brief**: `DEFER-quiqpic-20260811`

The **conversion side** of the Development Jig doctrine. `jig-friction` (JIG-04) notices
grinding *within one session*; this oracle notices that **several separate sessions have
asked for the same capability and nobody has built it**, and it names the command that
drains the demand.

---

## Why it exists

Measured 2026-08-14 across every project on this machine: **223 logged jig/probe candidates
in 12 projects, 199 of them never dispositioned.** The collection side worked perfectly —
`log-probe-candidate` is cheap and reflexively invoked. Nothing converted its output.

The cause is structural, and worth knowing before changing anything here:

| Surface | Scope | Can it see cross-session recurrence? |
|---|---|---|
| `runtime-perception-questions` | this session (ARIA-25, **by construction**) | no |
| `jig-friction` | this session's command-usage repeats | no |
| `/0-uldf-portfolio jigs` | all projects, but a **raw line count** (pre-JIG-10; now live/total — DEC-364) | no |
| `/0-uldf-finalize` Phase 11 step 4c | this session, advisory (JIG-05/DEC-142) | no |

So the doctrine's *route, never inline* had no destination. An agent obeying it had, in
practice, chosen "nobody builds it, ever." Doctrine: `FOUNDATIONS/DEVELOPMENT_JIG_DOCTRINE.md`
§4.3.

---

## What it does

1. Reads `.claude/session-state/aria-probe-candidates.jsonl`.
2. Subtracts anything already dispositioned — a record in
   `.claude/session-state/jig-demand-dispositions.jsonl`, or a legacy in-record `triage`
   object (one project hand-invented that field and drained 24 candidates with it; the
   shape is read so that work is not orphaned).
3. Clusters the remainder by IDF-weighted token overlap of `question` + `capability`
   (single-link, default threshold 0.20).
4. Reports clusters with `>= minCandidates` members drawn from `>= minOccasions` **distinct
   occasions** — a `sessionId` where present, the `ts` calendar date where not.
5. Emits **nothing** unless something crosses both thresholds. Measured: 8 of 12 real
   project corpora are silent.

## Draining a cluster — this is the half that makes it a pipeline

```bash
scripts/aria/jig-demand-triage.sh --cluster <clusterKey> \
    --verdict built|superseded|covered|declined \
    --reason "<what was built, or what already covers it, or why not>"
```

`--reason` is mandatory: a disposition with no reason is indistinguishable from
habituation. **`declined` is a first-class verdict** — the alternative to an honest
declination is not a built jig, it is a permanent nag and the habituation that follows.

Dispositioning removes the members from the population, so the cluster disappears. If new
candidates for the same capability accumulate later, it re-fires on the remainder. That is
the intent: draining silences it, recurrence re-raises it.

---

## Three design choices, each against a measured alternative

- **Lexical grouping, not a dedup key.** Exact `capability`-string duplicates across the
  223-record corpus: **3 groups (1.3%)**. Free-text asks never repeat verbatim, so the
  brief's leading remedy — capability-keyed dedup at log time — had nothing to dedupe. The
  rejected design is pinned as mutation **M6** in `scripts/smoke-tests/jig-demand-mutations.sh`,
  which passes every cell except T9.
- **Occasions, not cluster size.** A single session enumerating its own gaps produces a
  large, tight cluster that is *one ask* (measured: clusters of 7 and 6 candidates, each
  from one session). Size alone escalates them; occasions withhold them.
- **Advisory, and finalize step 4c left alone.** Making step 4c blocking was declined in
  DEC-362, leaving JIG-05/DEC-142 intact: the demand is cross-session while finalize is
  session-scoped, so step 4c is the wrong **location** before it is the wrong **strength**.

---

## Reading the output honestly

`assertion.known_gaps` in `oracle.json` is the contract, and two entries bite in practice:

- **`clustered: false` means NOT MEASURED, never "nothing found."** Without a python
  interpreter no grouping runs at all; `status` is still `ok`. Read `clustered` first.
- **The similarity metric is corpus-size dependent.** Below roughly ten records the IDF
  weighting is degenerate and nothing merges — a small log gets no clustering regardless of
  how real its recurrence is. This is declared rather than tuned away: the loosenings that
  fix it were measured turning a coherent 9-member cluster in the largest real corpus into
  an 18-member blob.

**A passing `--self-test` is not a trust verdict** (OVALID-03). Every cell is written in the
vocabulary of what the oracle *measures*.

---

## Self-test

```bash
.claude/oracles/jig-demand/validate.sh      # 24 cells
.claude/oracles/jig-demand/validate.ps1     # 20 cells -- run under BOTH engines
```

Run the ps1 twin under **both** `powershell` and `pwsh`. A live `ConvertFrom-Json` ISO-8601
coercion defect (pwsh 6+ returns `[DateTime]`, 5.1 returns `[string]`) made the drain write
`"7/1/2026 10:00:00 AM"` into `covers`, matching no record — exit 0, a written line, and
nothing silenced, while 5.1 was green throughout (DISC-JIG-02). The sweep
`scripts/smoke-tests/jig-demand-smoke.sh` runs every available engine and SKIPs with a note
rather than silently passing.

Falsifiability: `scripts/smoke-tests/jig-demand-mutations.sh` (7 mutations, each declaring
the cells that must go RED **and** the cells that must stay GREEN). Receipt:
`docs/falsifiability/2026-08-14-jig-demand-drain.md`.
