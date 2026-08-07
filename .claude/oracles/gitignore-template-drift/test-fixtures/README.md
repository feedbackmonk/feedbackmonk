# gitignore-template-drift fixtures

> Synopsis — Six representative input pairs (`baseline.gitignore` + `project.gitignore`) covering the drift cases the oracle must classify correctly. Driven by `validate.{sh,ps1}` via `CLAUDE_GITIGNORE_BASELINE` / `CLAUDE_GITIGNORE_PROJECT` env overrides.

## Purpose

Anchor the FROZEN output schema (`{ drifted, missing_patterns, baseline_patterns, project_patterns, briefing }`) and detection semantics (line-trimmed exact match, only patterns under the `# Claude Code (session artifacts — never commit)` header) against the failure modes a real adopting project can land in.

## File index

| Fixture | Setup | Expected | Why this case matters |
|---|---|---|---|
| `no-drift/` | project has all 13 baseline framework patterns | `drifted=false`, `missing_patterns=[]`, `briefing=""` | Empty-briefing case — line MUST suppress in hook briefing |
| `1-pattern-missing/` | project missing the trailing `.tauri-dev.pid` line | `drifted=true`, `missing_patterns=[".tauri-dev.pid"]`, `briefing` references count `1` | Single-pattern drift (typical regression after a baseline update) |
| `5-patterns-missing/` | project has only the 8 Phase 0 patterns; missing the 5 CSI / handoff / app-lifecycle additions | `drifted=true`, `missing_patterns` length 5 | Legacy-project shape (pre-CSI-1.6 setup) — the trigger case for HYGIENE-02/03 |
| `project-has-extra-patterns/` | project has all 13 framework patterns PLUS user customizations (`.idea/`, `target/`, etc.) | `drifted=false`, `missing_patterns=[]` | Extras are user customization, not drift; oracle must not flag them |
| `no-baseline-found/` | only `project.gitignore` exists; baseline path resolves to nothing | `drifted=false`, `baseline_patterns=0`, `briefing=""` | Graceful absent — oracle never spams "baseline missing" briefing line |
| `project-no-gitignore/` | only `baseline.gitignore` exists; project path resolves to nothing | `drifted=true`, `missing_patterns` length 13, `project_patterns=0` | Brand-new project that hasn't committed a `.gitignore` yet — full drift, full prompt |

## Public API

Driven by `validate.sh` / `validate.ps1`. Each fixture is consumed via:

```bash
CLAUDE_GITIGNORE_BASELINE="<fixture>/baseline.gitignore" \
CLAUDE_GITIGNORE_PROJECT="<fixture>/project.gitignore" \
bash run.sh
```

(For the two "missing" fixtures, the corresponding env var points at a path that does not exist — the oracle's graceful-absent branches handle them.)

## Constraints

- Baseline files include a non-framework prelude (e.g. `node_modules/`) before the framework header to verify the section parser correctly skips pre-header content.
- Project files vary in non-framework content to verify the pattern set is independent of project-specific lines.
- Patterns preserve UTF-8 bytes verbatim (the em-dash in the section header is U+2014); fixtures must not be re-saved under a non-UTF-8 encoding.

## Decision log

- **Why fixture-file pairs rather than inline heredocs in validate.sh?** Same precedent as `dispatchable-sessions/test-fixtures/` (CSI-01 fixture as a JSON file alongside the validate script). Side benefit: fixtures are inspectable without reading the validate script, and their content is reviewable in code review as data.
- **Why 6 cases, not more?** The frozen output schema has 5 fields and the detection logic has 4 decision points (drifted vs no-drift × baseline-present vs absent × project-present vs absent). 6 cases exhaust the meaningful Cartesian product without redundant coverage.
