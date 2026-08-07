# doctrine-footprint oracle

**Kind**: `project-state` · **Lane**: fast · **Consultation**: on-demand (NOT a session-start briefing oracle) · **Spec**: DAUD-01/03/07 (§ DOCTRINE-AUDIT) · **DEC**: DEC-153

## Purpose

Deterministic **presence check**: for each ULDF doctrine, which prescribed instrumentation artifacts does *this* project have installed? Answers the recurring *"is Tectonurgy / are Development Jigs integrated here?"* question that otherwise forces a manual multi-doc sweep. It is the presence engine consumed by the **`/0-uldf-doctrine-audit`** skill (DAUD-02), which renders verdicts and routes remediation.

**Presence only.** This oracle reports which artifacts exist; it does *not* render final verdicts, judge `not-applicable`, or route fixes — those are the skill's judgment layer. The `verdict_hint` field is a naive present/absent rollup (`integrated` | `partial` | `missing`) the skill may override (e.g. → `not-applicable` for RSPD on a flat spec, or ARIA on a no-surface project). A fourth value, **`pilot-gated`**, marks a doctrine the framework has authored but does not yet prescribe instrumentation for — empty `present[]`/`absent[]`, **0** contribution to `missing_total`, and a `note` explaining the gate. It exists so a project can *discover* the doctrine, never to pressure adoption of artifacts the framework does not yet ship per-project (DAUD-07).

## File Index

- `oracle.json` — manifest (schema, invocation, freshness, provenance).
- `run.sh` / `run.ps1` — the presence checker (byte-identical output across platforms; hand-assembled JSON).
- `validate.sh` / `validate.ps1` — self-test: runs the oracle in each fixture, exact-matches the golden `expected-output.json`, asserts determinism.
- `test-fixtures/fully-instrumented/` — a project with every shipped artifact, including a finalize-skill stub carrying the jig-retrospective wire (→ all doctrines `integrated`; JIG-05 shipped, DEC-154).
- `test-fixtures/greenfield/` — a bare project (→ everything `missing`).

## Output Schema

Single-line JSON: `{ schemaVersion, has_claude_dir, framework_baseline, doctrines[], missing_total, no_data, briefing }`. Each `doctrines[]` entry is `{ doctrine, present[], absent[], verdict_hint, note? }` (`note` appears only on `pilot-gated` doctrines). Artifact tokens are tagged: `oracle:<name>`, `spec:<section>`, `file:<name>`, `log:<name>`, `manifest:<name>`, `wire:<phase>`, `coverage:<marker>`, `dir:<path>`.

## Doctrine Coverage (DAUD-03)

| Doctrine | Checked artifacts |
|---|---|
| `tectonurgy` | oracles `module-size`, `change-coupling`, `code-graph`; spec `## MODULARITY` |
| `jigs` | oracle `jig-friction`; `JIG_CATALOG.md`; demand log `aria-probe-candidates.jsonl`; finalize jig-retrospective wire (JIG-05, shipped — project-local `.claude/skills/0-uldf-finalize/` checked first, then the global `~/.claude/skills/` install; absence now means a stale framework install, DEC-154) |
| `bvw` | oracle `block-verifiability-warrant`; `## Verifiability Warrant` README coverage sections |
| `aria_aor` | oracles `aria-status`, `ui-surface-detector`; `.claude/aria-manifest.json`; probe log |
| `in_app_agent` | **`pilot-gated`** — no artifacts prescribed yet. The In-App Agent / Self-Operating App doctrine (TUTOR-01..11, `FOUNDATIONS/IN_APP_AGENT_DOCTRINE.md`) is authored and contract-frozen, but per-project instrumentation waits on the SessionHelm Arc 2 pilot (`registry-anchors-resolve` is in `packs.pilot-gated` — never installed by `/0-uldf-setup-project`). Reported so projects *know it exists* and can design UI surfaces for it; contributes **0** to `missing_total` (DEFER-021) |
| `rspd` | oracle `rspd-elaboration-tree` (skill may judge `not-applicable` on a flat spec) |
| `oraculurgy` | hand-off note — `.claude/oracles/` + `INDEX.md` present; depth audit is `/0-uldf-oracle`'s job |

## Baseline Honesty (DAUD-07)

The **expected**-artifact set is what the framework *ships* as of `framework_baseline` (encoded in the run scripts) — not a doctrine's aspirational prescription. `dependency-drift` is deliberately **excluded** because it ships as a *global helper script* (`~/.claude/scripts/dependency-drift.*`), never installed per-project — checking for it under a project's `.claude/` would false-flag every project. The one global artifact the audit *does* consult is the finalize skill (for the JIG-05 jig-retrospective wire): unlike a helper script, "does the finalize that runs on this project carry the retrospective?" is answerable by grepping the skill that actually executes here (project-local override first, then `~/.claude/skills/`), so the check is meaningful rather than a guaranteed false flag. When the framework adds a per-project artifact, add it to the baseline block in `run.{sh,ps1}` (bump `framework_baseline`) and the audit picks it up.

## Contract

- **Deterministic**: same project state → same output (counts are order-independent).
- **Gracefully absent**: no `.claude/oracles/` → every artifact absent, `exit 0` (a finding, not an error).
- **NO-DATA honest**: a present-but-unreadable scan target → `no_data:true`, never a silent "clean".
- **Advisory**: never blocks anything; an audit engine, not a gate.

## Decision Log

- **On-demand, not `every`-session** — `typical_sessions_using: "on-demand"` keeps it out of the session-start briefing fan-out. It is an audit engine the skill invokes, not a per-session line (that would tax every startup for a question asked rarely).
- **Presence/verdict split** — the deterministic part (does the file exist?) lives here; the judgment part (`not-applicable`, routing) lives in the skill. Mirrors the Oraculurgy discipline: pre-compute the deterministic, leave judgment to the agent.
- **Pattern lineage** — mirrors `probandurgy-footprint` (presence-check shape) but as `kind: project-state` (not a build-config verification oracle).
