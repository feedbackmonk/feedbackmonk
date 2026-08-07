# code-graph Oracle

## Synopsis

Read-only query oracle over the repo's **module-level dependency graph**. Answers four verbs — `--deps`, `--consumers`, `--impact`, `--cycles` — over the module DAG, each with a **mandatory `coverage` field** carrying M6 edge-data honesty. It does **not** derive the graph itself: it reuses the *same* pluggable `dependencyDrift.edgeExtractor` seam the dependency-drift check defines, by shelling `dependency-drift.py --emit-edges` and computing the verbs over the result. **On-demand** (NOT in the every-session briefing fan-out) — invoked directly (`.claude/oracles/code-graph/run.{sh,ps1}`), via `/0-uldf-oracle`, and by Arc 3 A4 impact-injection. Don't come here for README §5 declared-vs-observed drift (that's the `dependency-drift` check) or for co-change coupling (that's the `change-coupling` oracle) — scope is graph *queries* over the observed edge set.

> **Category**: module | **Kind**: project-state | **Lane**: on-demand

Scrutiny ADD proposal **A1** (verdict **AMEND**), Arc 3 pilot: `docs/planning/scrutiny-hierarchical-modularity-20260707.md` §6 A1, §5 M6, §9 Arc 3. The AMEND direction, bound to verbatim: **wrap existing per-stack tooling behind one envelope + the pluggable per-project edge-extractor contract (M6)** — no bespoke parser fleet — with a mandatory `coverage` field and `unparsed files = NO-DATA never absence-of-edges`.

## What it is / is NOT

- **IS** a query layer over the module graph: efferent deps, afferent consumers, transitive impact set, and dependency cycles. The cycle-detection that lived only as prose is now **mechanized** (Tarjan SCC over the module DAG).
- **IS** advisory: exit `0` always; the `briefing` line fires **only** when cycles are present. It never blocks and never gates a commit.
- **IS NOT** an edge-derivation engine. It reuses the dependency-drift `--emit-edges` graph wholesale (see the seam below). If you are tempted to write an import parser here, **stop** — plug an extractor into `.claude/config.json` `dependencyDrift.edgeExtractor` instead (M6).
- **IS NOT** a per-language parser fleet. The grep-grade default observer + the pluggable extractor *are* the coverage story — the same one dependency-drift documents.

## The reuse-of-dependency-drift seam (M6 — the load-bearing design decision)

The module-edge derivation — grep observer + pluggable `dependencyDrift.edgeExtractor`, module ownership by nearest-README, lateral-only edges, per-module coverage/NO-DATA — **already lives** in `dependency-drift.py`. Re-deriving it here would fork the coverage story and drift. So dependency-drift gained a purely additive `--emit-edges` mode that prints the derived module graph as JSON:

```json
{ "schemaVersion": "1.0",
  "edges":   [ { "from": "<module>", "to": "<module>", "evidence": "<file>:<line>" } ],
  "modules": [ { "path": "<module>", "coverage": "full|grep-only|none", "noDataReason": null } ],
  "extractor": { "configured": false, "mode": null, "coveredModules": [], "note": null } }
```

`--emit-edges` is byte-for-byte additive: the dependency-drift default / `--changed` / `--scope` behavior and its 38/38 smoke stay green. code-graph shells it (locating `../../scripts/dependency-drift.py` relative to the oracle dir — the same hop in the template repo and in a deployed `.claude/`), then computes the verbs. **A configured edge-extractor promotes a module's coverage to `full` for BOTH tools at once** — one seam, two consumers. This is the "Arc 3 A1 pilot consumes this same seam — the extractor is the forward-compatible boundary" line the dependency-drift README promised.

## Query verbs (mutually exclusive; default = full-graph summary)

| Verb | Answer | `--transitive`? |
|---|---|---|
| `--deps <module>` | modules `<module>` depends on (efferent / out-edges) | yes — extends to the transitive closure |
| `--consumers <module>` | modules that depend ON `<module>` (afferent / reverse edges) | yes — transitive closure of consumers |
| `--impact <module>` | transitive closure of consumers — everything that could break if `<module>`'s interface changes | always transitive (that is the point) |
| `--cycles` | all dependency cycles / strongly-connected components of size ≥ 2 | n/a |
| *(none)* | summary `{ modules, edgeCount, cycleCount }` | n/a |

`--impact` is the set **Arc 3 A4** injects into worker briefs + finalize Phase 3 as an advisory "Impact Context" block (with the coverage header — anchoring-risk mitigated by "run `--impact` for the full set").

## Coverage honesty (MANDATORY on every answer — M6)

Every answer carries `coverage` = the **weakest (min)** coverage of the modules **involved** in the answer (the target plus every module in the result set):

| `coverage` | Meaning |
|---|---|
| `full` | Every involved module was authoritatively extractor-covered. `coverageNote` empty. |
| `grep-only` | At least one involved module was only grep-observed → **the answer is a FLOOR (lossy), not a proven-complete set.** An impact set over grep-only edges says so. |
| `none` | Nothing observable for the involved module(s) → an empty result is **inability-to-observe, NOT proven absence** (NO-DATA floor). |

Unparsed files are NO-DATA, **never** a silent absence-of-edges — this is the M6 invariant carried end-to-end from the edge source. `coverageNote` is an honest degrade string (empty only when `full`).

## NO-DATA honesty (never a silent "no dependencies")

`status: "no-data"` with a `reason` — never an empty-but-`"ok"` result that reads as "no dependencies" — is emitted when the query cannot be answered:

| Reason | Trigger |
|---|---|
| `not a git repository` | outside a git work tree (the edge source signals it) |
| `no modules (no README-anchored directories found)` | a git repo with no ULADP modules |
| `module '<t>' not found` | `--deps`/`--consumers`/`--impact` target is not a module of this repo |
| `verbs are mutually exclusive (…)` | more than one verb flag passed |
| `edge source dependency-drift.py not found` / `edge source …` | the `--emit-edges` provider is missing or errored |
| `no working python interpreter` | the wrapper found no usable python (PW-005) |

A genuine clean result (a real module with zero deps under `full` coverage) is `status: "ok"` with an empty `result` and `coverage: "full"` — the oracle **did** check.

## Output schema (FROZEN, schemaVersion `1.0`)

```json
{
  "status": "ok",                       // "ok" | "no-data"
  "schemaVersion": "1.0",
  "query": { "verb": "impact", "target": "src/modA", "transitive": true },
  "result": [ "src/modB", "src/modC" ], // deps/consumers/impact: module paths (sorted, capped 500)
                                        // cycles:  [ { "members": ["a","b"], "size": 2 } ]
                                        // summary: { "modules": [...], "edgeCount": 12, "cycleCount": 1 }
  "coverage": "grep-only",              // full | grep-only | none — MANDATORY (weakest of involved modules)
  "coverageNote": "at least one involved module is only grep-observed; this answer is a FLOOR …",
  "truncated": false,                   // true when a result list was capped at 500 (no silent drops)
  "reason": "…",                        // present ONLY on no-data
  "extractor": { "configured": false, "coveredModules": [] },  // passthrough edge-source honesty
  "briefing": ""                        // non-empty ONLY when cycles present (quiet otherwise)
}
```

Emitted with sorted keys + deterministic separators, so `run.sh` and `run.ps1` (which share this one canonical `code-graph.py` behind probe-a-working-python wrappers) produce **byte-identical** JSON. `--compact` is a single line; default is pretty-printed.

## Configuration

code-graph adds **no config of its own** — it inherits the shared `dependencyDrift` edge-extractor contract:

```json
{ "dependencyDrift": { "edgeExtractor": "npx dependency-cruiser … | your-adapter.sh", "edgeExtractorMode": "union" } }
```

Configure a per-stack extractor (dependency-cruiser / madge / import-linter / jdeps behind a `<from>\t<to>` adapter) to promote coverage from `grep-only` to `full`. Absent extractor ⇒ honest `grep-only` (a FLOOR), never a silent full pass (M6). See `scripts/DEPENDENCY_DRIFT_README.md` § *Pluggable edge-extractor contract*.

## Invocation

```bash
.claude/oracles/code-graph/run.sh                                  # summary (modules/edges/cycles)
.claude/oracles/code-graph/run.sh --cycles --compact               # all dependency cycles (SCCs >= 2)
.claude/oracles/code-graph/run.sh --deps path/to/modA              # direct out-edges
.claude/oracles/code-graph/run.sh --deps path/to/modA --transitive # transitive deps
.claude/oracles/code-graph/run.sh --consumers path/to/modA         # who depends on modA
.claude/oracles/code-graph/run.sh --impact path/to/modA            # transitive consumer closure (A4)
```

Flags: `--root DIR`, `--config FILE` (both forwarded to the edge source), `--compact`. Advisory exit `0` always.

## Verification

```bash
bash .claude/oracles/code-graph/validate.sh
powershell -NoProfile -ExecutionPolicy Bypass -File .claude/oracles/code-graph/validate.ps1
```

Assert valid JSON, the frozen schema fields, a legal `status` and the MANDATORY `coverage` value, and the NO-DATA path on a non-git directory. Full behavioral coverage — a synthetic git fixture with a KNOWN module edge set (a fake `edgeExtractor` injected via a temp `.claude/config.json` so `coverage: full` is exercised deterministically without real dependency-cruiser/madge), each of the four verbs, `--transitive`, a KNOWN cycle, coverage honesty (full vs grep-only vs NO-DATA), target-not-found NO-DATA, and **bash↔PowerShell byte-identical JSON parity** — lives in `~/.claude/scripts/smoke-tests/code-graph-oracle-smoke.sh`.

## Decision Log

- **Why reuse `dependency-drift.py --emit-edges` instead of deriving edges here** — M6 makes language-agnosticism + one pluggable extractor load-bearing. Forking the derivation would give two coverage stories that drift; a project that plugs an extractor must raise coverage for *both* tools at once. One seam, two consumers. The `--emit-edges` mode is additive so the drift check's 38/38 smoke stays green.
- **Why python-canonical + thin `.sh`/`.ps1` wrappers** — Tarjan SCC, transitive closure, and deterministic JSON emission would drift between a bash and a PowerShell reimplementation. One canonical `code-graph.py` behind probe-a-working-python wrappers gives *byte-identical* cross-shell JSON — the same pattern as `dependency-drift.py` and `scripts/companion/`.
- **Why `coverage` is the weakest-of-involved, not the target's alone** — a transitive answer traverses intermediate modules' out-edges; if any was only grep-observed the whole chain is a floor. Reporting the target's coverage alone would over-claim. The min rule is the honest one.
- **Why on-demand, not every-session** — this is a *query* oracle (answer a verb for a named module) + an A4 impact-injection dependency, not an orientation briefing. An every-session run would compute a graph nobody asked about. It lives beside the other on-demand module oracles.
- **Why advisory + cycles-only briefing** — dependency cycles are the one finding a graph query surfaces unprompted (the prose cycle mandate never mechanized); everything else is a caller's question, not a finding. Blocking on cycles would invite split-for-the-metric gaming (the same perverse incentive the `module-size` oracle documents); detection-first per the scrutiny's enforcement-posture adjudication.

## Relationships & Dependencies

- **Consumes**: `~/.claude/scripts/dependency-drift.py` (the `--emit-edges` additive mode — the edge source + the shared `dependencyDrift.edgeExtractor` seam, M6). This is the load-bearing dependency.
- **Consumed by**: `/0-uldf-oracle` (audit/query); Arc 3 **A4** impact-injection into worker briefs + `/0-uldf-finalize` Phase 3 consumer note (downstream of this + `change-coupling` union).
- **Sibling detection surfaces** (same scrutiny, do not conflate): `~/.claude/scripts/dependency-drift.{sh,ps1}` (README §5 declared-vs-observed drift), `~/.claude/oracles/change-coupling/` (git co-change coupling), `~/.claude/oracles/module-size/` (per-module token weight).
- **Verified by**: `~/.claude/scripts/smoke-tests/code-graph-oracle-smoke.sh`.
- **Runtime**: a working python (PW-005 — the WindowsApps `python3` shim is broken; the wrappers probe `python`/`python3`/`py`), `git`.

## Lineage

Scrutiny `docs/planning/scrutiny-hierarchical-modularity-20260707.md`: proposal **A1** (AMEND — wrap tools + M6 extractor + mandatory coverage + NO-DATA), **A4** (impact-set injection, downstream consumer), **§5 M6** (pluggable extractor / language-agnostic floor / honest NO-DATA), **§9 Arc 3** (pilot before fleet). The field's largest measured impact-context wins come from code-graph machinery (RepoGraph +32.8% rel. SWE-bench; LocAgent 92.7% file-localization) — this is ULDF's confirmed-absent leg, now present behind the M6 honesty envelope.
