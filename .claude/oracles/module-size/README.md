# module-size Oracle

## Synopsis

Verification Oracle (`kind: "verification"` per Oraculurgy Part 11) that estimates every ULADP module's token weight (byte-based, no file parsing) and reports modules over a **soft band** (default ~4000 tokens, M2) plus directories that look like undocumented modules (`>=3` code files, no `README.md`). **Advisory** (`status: warn`, exit 0) by design — a hard cap invites split-for-the-metric micro-modules. Fires at `/0-uldf-finalize` Phase 1a; also consumable on demand and by `/0-uldf-uladp-compliance` / `/0-uldf-ldis-plan`. Don't come here for boundary/coupling analysis (that's the `change-coupling` oracle + `/0-uldf-uladp-compliance --architecture`) or for content quality — scope is per-module size only.

> **Category**: module | **Kind**: verification | **Strategy**: always-fresh | **Lane**: fast (`<2s`)

## Why this oracle exists (P0-7)

The framework's own doctrines jointly mandate this oracle and it was conspicuously absent. The **Enforcement Placement Principle** says a check that is *pure arithmetic* (a byte count vs a threshold, near-zero false positives) belongs in **code, not prose**; **Oraculurgy Part 11** defines exactly this "did the last action violate a constraint?" shape. Yet `trajectory-cap` shipped a deterministic size-cap oracle for **one markdown file** while the doctrinal per-module token cap — the framework's cornerstone modularity constraint — had **no deterministic detection surface at all** (it lived as a self-graded prose bullet and an ADVISORY classification in `/0-uldf-uladp-compliance`). Scrutiny finding **P0-7**; ADD proposal **A6** (build first — cheapest, EPP-mandated).

## What a "module" is here

- **Module** = a directory containing a `README.md` (the ULADP module unit, M1). Its token estimate = `sum(bytes of files directly in the directory) / 4` — one divisor everywhere, mirroring the existing convention in `~/.claude/segments/-uladp/compliance_architecture.md` (~:49). Non-recursive: a hierarchical parent is sized by its *own* files, not its children's.
- **`no_readme` candidate** = a directory with **no** `README.md` but **>= 3 code files** (by extension). Surfaced, never silently skipped — it may be an undocumented module. (`< 3` code files → treated as incidental, not a module: the M1 "incidental directory" exemption, so a per-dir README obligation isn't invented for junk-drawer dirs.)
- **Excluded** (never a module): `.git`, `node_modules`, `dist`, `build`, `coverage`, `__pycache__`, `.next`, `.expo`, `target`, `.cargo`, `vendor`, `.claude/collaboration`, `.claude/session-state`. The `git ls-tree` fast path already drops `.gitignore`'d paths; this list is the builder-level belt-and-suspenders (and the parity contract with the non-git walk).

## The band is a soft band, not a cliff (M2)

The `~4000` number is **restated** from M2's adjudication. Its original justification — "fits an agent's context window" — is **obsolete at ~1M-token contexts**. The band survives as a **tripwire proxy** for *"one session can hold and reliably reason about this leaf"* (Team-Topologies cognitive-load framing; cross-ref CTD-16's reliable-completion horizon — "one cheap worker can own this leaf"), derived from Principle 2.9's capacity formula rather than asserted as a context-fit fact.

The **better diagnostic** M2 promotes is the **500-token summary-compressibility test** (already in Principle 2.9, previously wired to nothing): *can the module be faithfully summarized in ~500 tokens?* If not, it is doing too much regardless of byte size. That test is **agentic** (it requires judgment) and is therefore **not this oracle's job** — this oracle is the deterministic tripwire that says "look here"; the compressibility test is the qualitative follow-up an agent runs on what the tripwire surfaces.

### Perverse-incentive note (do not "harden" this into a gate)

A **hard** size cap invites the exact failure it is meant to prevent: agents **split-for-the-metric** into micro-modules that satisfy the byte count while *increasing* coupling and fragmentation. That is why this oracle is **advisory — report, don't refuse** (mirroring the `trajectory-cap` precedent and the scrutiny's detection-first posture). A future maintainer tempted to make it blocking should first read scrutiny §5 M2 + §5's enforcement-posture adjudication: blocking is EPP-defensible only for the deterministic-and-cheap subset once a **pre-registered escalation trigger** fires (e.g. a module crossing the band commit-by-commit while every finalize stays green — finding P1-10). Until then, the block would produce gate fatigue and reward-hacking, not better modules.

## Output schema (frozen)

```json
{
  "status": "warn",
  "modules_scanned": 69,
  "over_band": [ { "path": "FOUNDATIONS", "est_tokens": 176789, "band": 4000 } ],
  "over_band_total": 52,
  "no_readme": [ { "path": "claude-template/scripts", "est_tokens": 9331, "code_files": 12 } ], <!-- path-ok: sample output data from a run on the ULDF source repo -->
  "no_readme_total": 22,
  "no_data": [ { "path": "some/unreadable/path", "reason": "Permission denied" } ],
  "no_data_total": 1,
  "band": { "softTokens": 4000, "source": "default" },
  "enum_mode": "git",
  "scan_duration_ms": 1011,
  "briefing": "module-size: 52 modules over the ~4000-token soft band ..."
}
```

Programmatic consumers may read `status`, the numeric totals, and the `over_band[] / no_readme[] / no_data[]` arrays. `over_band` and `no_readme` are **sorted by `est_tokens` descending and capped at 50 entries** — `*_total` fields carry the true pre-cap count so a truncated list never reads as complete. A clean scan carries `status: "pass"` with an **empty `briefing`** (suppressed-line convention).

## Advisory + NO-DATA contract

- **Advisory**: over-band modules → `status: "warn"`; the script **always exits 0** on a real run. Non-zero exit is reserved for execution error. `warn` never blocks a commit.
- **NO-DATA honesty** (report rubric G2/G4): an unreadable project root, a missing root, or a tree with no analyzable files → `status: "no-data"` (**never a silent `pass`**). Unreadable individual paths land in `no_data[]` with a reason. "Could not check" is always distinguished from "checked and clean."

## Enumeration: fast path + fallback

- **git fast path** (`enum_mode: "git"`): `git ls-tree -r -l HEAD` returns per-file sizes in **one process** (no per-file `stat`) and is **tracked-only**, so it honors `.gitignore` by construction. Both `run.sh` and `run.ps1` parse the **same** git output, which is what makes their JSON byte-equivalent. Measured ~1.0s on this repo (1276 files) — under the Part 11 fast-lane `<2s` contract.
  - *Caveat*: `ls-tree HEAD` reflects **committed** state; uncommitted working-tree deltas are not reflected. Acceptable for an advisory size tripwire; the next finalize sees the committed sizes.
- **find fallback** (`enum_mode: "find"`): non-git tree, or a git repo with no commit yet → `find ... -printf '%s\t%p\n'` (one process; prunes the big dirs). Does not honor arbitrary `.gitignore`, so the exclusion list above does the filtering.

## Configuration

Override the band per-project in `.claude/config.json`:

```json
{ "moduleSize": { "softBandTokens": 4000 } }
```

Absent / non-numeric → default 4000 (`band.source: "default"`); a valid integer → `band.source: "config"`.

## Validation

```bash
bash .claude/oracles/module-size/validate.sh
powershell -NoProfile -ExecutionPolicy Bypass -File .claude/oracles/module-size/validate.ps1
```

Assert valid JSON, the frozen schema fields, a legal `status`, and the `pass ⇒ empty briefing` invariant. Behavioral fixtures (over/under-band, `no_readme`, excluded dir, config override, find fallback, NO-DATA, PS parity) live in the smoke: `~/.claude/scripts/smoke-tests/module-size-oracle-smoke.sh` (27/27).

## Lineage

Scrutiny `docs/planning/scrutiny-hierarchical-modularity-20260707.md`: finding **P0-7** (trigger), **P0-2 / P1-10** (the enforcement-absence this corrects), **§5 M2** (soft-band re-derivation), **§6 A6** (KEEP, build first), **§9 Arc 1**. Corrects **COVERAGE_MATRIX DEF-01** (context-window-limits row) from an over-claimed COVERED to an honest status citing this oracle as the sizing-leg verification surface. Sibling deterministic detection surfaces in the same arc: `change-coupling` oracle (A2) + the §5 declared-vs-observed dependency-drift check. Advisory-not-blocking precedent: `trajectory-cap`.
