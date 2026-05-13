# project-runtime-state Oracle

**Kind**: project-state
**Spec**: WT-05 in `docs/specs/SPECIFICATION.md`
**Decision**: DEC-61 in `docs/specs/DECISIONS.md`
**Created**: 2026-05-10 (PODS opt-in worktree mode Arc 1)

## Question

> Does this project have live dev servers, shared build artifacts, file watchers, or stateful runtimes that would conflict under PODS worktree isolation?

## Consumer

`claude-template/segments/-pods/parallelize_analysis.md` Step 6 (the WT-06 proactive heuristic) calls this oracle to compute `antiFitScore` and surface an opt-in/anti-fit recommendation when a user runs `/0-uldf-pods-parallelize`. Suggestion-only — the heuristic never auto-flips `--worktrees` based on this oracle's output.

Not surfaced at session-start. Compute cost is ~150ms (port probes + filesystem globs); paying that on every session-start would be wasteful when most sessions never invoke `/0-uldf-pods-parallelize`.

## Output Schema (frozen at v1)

```json
{
  "schemaVersion": 1,
  "hasLiveDevServer": false,
  "devPortRegistryEntries": [
    { "project": "<name>", "port": 5173, "source": "MACHINE_CONFIG.md" }
  ],
  "sharedBuildArtifacts": ["node_modules", "target", ".gradle"],
  "fileWatchers": ["vite.config.ts", "nodemon.json"],
  "statefulRuntime": "tauri",
  "antiFitScore": 3,
  "antiFitReasons": [
    "stateful runtime detected: tauri",
    "file watcher config(s) present: vite.config.ts, nodemon.json",
    "Dev Port Registry assignment(s) for this project: 1"
  ]
}
```

### Field semantics

- `schemaVersion`: integer, frozen at 1. Future additive fields land at v2 but field-level addition keeps v1 consumers working (graceful absence).
- `hasLiveDevServer`: boolean. True iff at least one Dev Port Registry-assigned port for this project is currently bound (LISTEN state).
- `devPortRegistryEntries`: array of `{project, port, source}`. Parsed from `~/.claude/MACHINE_CONFIG.md` `## Dev Port Registry` section. Scoped to the current project (basename of `pwd`) by case-insensitive substring match. `source` is currently always `"MACHINE_CONFIG.md"`; reserved for future multi-source merging.
- `sharedBuildArtifacts`: array of strings. Detected dirs at workdir root: `node_modules`, `target`, `.cargo`, `.gradle`, `vendor`, `.venv`, `.next`, `.nuxt`, `build`, `dist`. Order is detection order (stable across runs).
- `fileWatchers`: array of strings. Detected configs at workdir root: `vite.config.{js,ts,mjs,cjs}`, `nodemon.json`, `webpack.config.{js,ts}`, `tsup.config.{js,ts}`, `rollup.config.{js,ts}`.
- `statefulRuntime`: string or null. One of: `"tauri"`, `"electron"`, `"expo"`, `"next.js-dev"`, `"django-runserver"`, or `null`. First match wins; checked in detection order (Tauri → Electron → Expo → Next → Django).
- `antiFitScore`: integer in [0, 5]. One point per active indicator (see below). >= 3 is treated as a strong anti-fit signal by the WT-06 heuristic.
- `antiFitReasons`: array of strings. Human-readable, one entry per active indicator. Suitable for direct display to the user.

### Indicator scoring

Each contributes 1 point to `antiFitScore`, capped at 5:

1. `hasLiveDevServer == true` (at least one assigned port bound)
2. `statefulRuntime != null`
3. >= 1 file-watcher config present
4. >= 2 shared-build-artifact dirs present (single ones are normal)
5. >= 1 Dev Port Registry assignment for this project (even if not bound)

## Freshness

`always-fresh`. Each invocation re-probes port liveness and re-globs the workdir; no caching. Compute cost is ~150ms in the typical case (cheap globs + 1-3 port probes). On Windows, `Get-NetTCPConnection` dominates; on POSIX, `lsof -i :PORT` is the typical path with `ss`/`netstat` fallbacks.

## Validation

Self-tests at `validate.sh` / `validate.ps1`:

- Schema fields present
- `schemaVersion == 1`
- `antiFitScore` in `[0, 5]`
- Determinism: two consecutive runs produce identical output

Test fixtures at `test-fixtures/`:

| Fixture | Description | Expected |
|---|---|---|
| `clean-static-repo.json` | Empty project: no `node_modules`, no `package.json`, no port assignment | `antiFitScore == 0` |
| `live-dev-server.json` | Project with active Dev Port Registry entry + bound port | `antiFitScore >= 3`, `hasLiveDevServer == true` |
| `tauri-stateful.json` | Tauri project with `node_modules` + `target` + `vite.config.ts` | `antiFitScore >= 3`, `statefulRuntime == "tauri"` |

Fixtures are reference snapshots — the oracle is environment-sensitive (port-bind state, MACHINE_CONFIG.md contents), so fixtures document what the heuristic **expects to see** when it consults the oracle in each environment, rather than serving as bit-exact input/output pairs.

## Lineage

- **Trigger**: 2026-05-10 research evaluation of `obra/superpowers` Claude Code plugin identified `using-git-worktrees` as the single additive idea worth lifting. Worktree isolation breaks under shared runtime state — this oracle pre-answers that fit-or-anti-fit question.
- **Decision**: DEC-61 (PODS Worktree Mode is Opt-In; Same-Branch Remains Default).
- **Spec**: WT-05 (this oracle), WT-06 (heuristic that consumes it).
- **Pattern**: Sibling to `project-type` (similar manifest-file inspection) and `dispatchable-sessions` (similar deterministic JSON-emitting probe). No `--gc` mode — this oracle is read-only with no registry to sweep.

## Known Limitations

- Dev Port Registry parsing is tolerant but not exhaustive: list-item form (`- name: port`) and table-row form (`| name | port |`) are recognized; freer prose may not be.
- `hasLiveDevServer` reflects port-bind state at the moment of invocation. A dev server that binds-and-releases would not be caught between probes.
- Project-name match is substring-based (case-insensitive). False positives are possible for projects whose names overlap (e.g., `foo` and `foo-bar`); future work could disambiguate via workDir absolute-path matching.
- The shared-build-artifact detection is presence-only; it does not distinguish a `node_modules/` with active hot-reload vs. a stale committed one. Indicator is conservative (favors anti-fit signal) by design.
