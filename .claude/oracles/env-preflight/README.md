# env-preflight Oracle

**Kind**: `verification` (ORACULURGY_DESIGN.md Part 11) · **Spec**: LTADS-ENV-01 · **Decision**: DEC-124

Deterministic environment-capability preflight for `/0-uldf-ltads-start`. Answers *"does this environment have the capabilities the project's development requires?"* at ~0 tokens, replacing the 239-line hand-rolled agentic probe that previously lived in `segments/-ltads/start_preflight.md` (scrutiny 04 F17/ADD-3 — Enforcement Placement § 1: code can enforce it). That segment is now a thin invoker over this oracle.

## Consumers

- `/0-uldf-ltads-start --preflight` (standalone validation, via `start_preflight.md`)
- `/0-uldf-ltads-start` Phase -2 (when environment validation is needed during the start flow)

Not wired into the every-session briefing fan-out — preflight is on-demand (`consultation.typical_sessions_using: "on-demand"`).

## Checks

| Check | Gate | Critical | Notes |
|---|---|---|---|
| `git` | always | yes | binary + inside a work tree |
| `json-parser` | always | no | jq, else a **working** python |
| `python` | always | no | PW-005 probe: a Windows Store stub binary counts as `degraded`, not present |
| `node` | `package.json` exists | yes (when gated in) | build/test cannot run without it |
| `playwright` | `playwright.config.*` exists | no | checks `node_modules/{playwright,@playwright}` presence — no slow `npx` call |
| `adb` | Android markers (widened: `android/`, `AndroidManifest.xml`, `*.gradle`, `apps/*/android`, `packages/*/android`, Expo `app.json`) | no | live `adb devices` probe bounded to 2s (`timeout` on Unix, `Start-Job` on Windows); degrades to binary-presence when unboundable |
| `node-deps` | `package.json` exists | no | node_modules presence (wall 1); fix points at `provision deps` (PROV-01) |
| `workspace-built` | workspace markers (`pnpm-workspace.yaml` / package.json `workspaces`) | no | workspace packages with `build` scripts but no `dist`/`lib`/`build` output (wall 4); fix points at `provision workspace` |
| `jdk` | Android markers (widened set above) | no | best JDK 17+ selection: Android Studio JBR > `JAVA_HOME` > PATH (wall 6); reports what stale PATH java would have been used. `PROVISION_JBR_CANDIDATES` (semicolon-separated) overrides discovery for tests |
| `version-drift` | `package.json` + `.claude/config.json` `provision.latestKnownMajors` | no | advisory (wall 9): installed majors vs the **config-driven** latest-known table. No table = `not-applicable` with explicit NO-DATA wording — a hardcoded table would itself go stale |
| `mobile-mcp` | mobile markers (Android set OR `app.json` + react-native/expo dep) | no | PROV-03 detection (spec: PODS MSG-001, CLAUDE-E): mobile MCP registration in project-scoped `.mcp.json` + command resolvability + **project trust state** (DEFER-PROV-MCP-REACH). NO-DATA honesty: unparsable is never reported as unregistered |

### `mobile-mcp` trust reporting (DEFER-PROV-MCP-REACH)

Registration in `.mcp.json` is **necessary but not sufficient**: Claude Code will not start a
project-scoped MCP server until the user has trusted it, so a registered-and-resolvable server whose
project lacks trust never loads its tools. The probe reads `~/.claude.json` → `projects[<root>]` and
reports:

| Trust state | `status` |
|---|---|
| `enableAllProjectMcpServers: true`, **or** the server named in `enabledMcpjsonServers` | `available` |
| server named in `disabledMcpjsonServers` | `unavailable` (explicitly disabled) |
| neither — trust never granted | `degraded` (`registered-but-untrusted`) |
| `~/.claude.json` absent/unreadable/unparsable, or no matching project key | `degraded` (**NO-DATA**, never a synthesized pass) |

The `fix` string names the three routes: accept the interactive trust prompt, set
`enableAllProjectMcpServers` for the project, or register the server at user scope.

**Read-only** (Part 11 § 11.3.4): the oracle reports trust, it never actuates it — `~/.claude.json`
is user-level consent state. Project-key matching normalizes separator style, drive-letter form
(`/c/…`, `/mnt/c/…` → `c:/…`), case, and Windows 8.3 short names; a genuine no-match is NO-DATA
rather than a guess.

Conditional checks report `not-applicable` when their gate marker is absent — non-web/non-mobile projects pay only the universal checks.

**PROV-01/03/08 extension (2026-07-02, ATS Arc 2)**: the five checks below `adb` are the provisioner's *check leg* — read-only state probes whose `fix` strings route to the `provision.{sh,ps1}` actuator (`~/.claude/scripts/provision/`). The oracle stays read-only (Part 11 § 11.3.4); the actuator carries the sanctioned mutation (Sweeper discipline, Part 12). Read/write split per the CSI-10 `run`/`update-baseline` precedent.

## Frozen Output Schema

Single-line JSON. **This schema is frozen** — programmatic consumers (the `start_preflight.md` invoker) parse these fields; extend additively only.

```json
{
  "ok": true,
  "checks": [
    {"name": "git", "status": "available", "critical": true, "detail": "...", "fix": ""}
  ],
  "critical_failures": 0,
  "warnings": 1,
  "summary": "all critical capabilities available (1 warning(s))",
  "briefing": ""
}
```

- `status` enum: `available | degraded | unavailable | not-applicable`
- `ok` is `false` iff `critical_failures > 0` (a critical check reports `unavailable`)
- `briefing` is empty when `ok=true` (graceful-absence convention)
- Exit code is always `0` — the oracle is advisory; the consuming skill decides STOP vs proceed-with-warnings

## Invocation

```bash
bash .claude/oracles/env-preflight/run.sh                       # Unix
powershell -NoProfile -File .claude/oracles/env-preflight/run.ps1   # Windows
```

Self-test: `validate.sh` / `validate.ps1` (schema fields + universal check entries + git-available in a work tree).

## Read-only contract

`run.{sh,ps1}` never write (Part 11 § 11.3.4). There is no baseline and no state — every run is a fresh probe (`freshness.strategy: always-fresh`).
