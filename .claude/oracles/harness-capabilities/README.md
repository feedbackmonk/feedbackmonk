# harness-capabilities Oracle

**Question**: Which native harness features does the current session's harness expose (version-keyed capability map)?

**Kind**: `project-state` · always-fresh · every-session cheap (~0.4s measured: env checks + one `claude --version`). HAL-01 (§ HAL, `docs/specs/SPECIFICATION.md`); DEC-156.

## What it is

The detection engine of the **HAL (Harness Abstraction Layer)**. It emits the frozen capability map — `{schemaVersion, harness, version, detectedAt, capabilities: {12 named booleans}, briefing?}` — that every HAL-gated skill branch consumes via `segments/_hal-gate.md`. The schema is **FROZEN** in `~/.claude/templates/HAL_CAPABILITY_MAP.md`; changing it is a spec-level event. Consumers read `capabilities` only; `briefing` is an advisory display line.

## Semantics (the short version — the contract is authoritative)

- **Undetectable ⇒ `false`**, never guessed. `version: null` forces every version-keyed bit `false`.
- **Non-Claude / unknown harness ⇒ all-false VALID output** (exit 0, never an error).
- **Oracle absent from a project ⇒ consumers take the ULDF path** (which always works — the ULDF equivalent is the spec of record).
- **All-false is always safe**: every consumer's `false` branch is the pre-HAL ULDF behavior. The worst failure this oracle can transmit is a missed fast path.

## The version→capability floor table lives HERE

The table (in `run.sh` / `run.ps1` § 4, byte-parallel) is the ONLY home of version-keying knowledge — consuming skills never version-sniff (the anti-pattern `_hal-gate.md` exists to prevent). Floors come from the changelog where crisp; **rolling features floor conservatively at the confirmed-live version** (2.1.205/2.1.206; evidence base `docs/NATIVE_FEATURES_2026H1_INTEGRATION_ANALYSIS.md`) — a false-`false` costs a missed fast path, never breakage.

**Updating a floor** (e.g. a rolling feature's true first-ship is confirmed earlier, or a new harness version adds a capability): edit the floor in BOTH run scripts, regenerate/adjust the affected `test-fixtures/*.json`, re-run both validators. No schema change, no DEC — unless adding/renaming a *bit*, which is a contract event (see HAL_CAPABILITY_MAP.md § 2).

## Detection model

1. **Harness**: env signals — `CLAUDECODE=1` or `CLAUDE_CODE_SESSION_ID` set ⇒ `claude-code`; neither ⇒ `unknown` (all-false valid map).
2. **Version**: `claude --version`, first `X.Y.Z` token. Failure ⇒ `null`.
3. **Bits**: floor table + runtime kill-switch refinements — `workflowTool` is `false` under `CLAUDE_CODE_DISABLE_WORKFLOWS=1`; `agentTeams` requires the experimental opt-in (`CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`). Bits answer *"can THIS session do this natively, right now?"*.

## Test seams (validators only — never set in normal operation)

| Env | Effect |
|---|---|
| `HARNESS_CAPS_FORCE_VERSION` | Raw version-string candidate replacing the `claude --version` call (unparseable ⇒ `null`) |
| `HARNESS_CAPS_NOW` | Pinned `detectedAt` (golden-fixture determinism) |

## Validation

`validate.sh` / `validate.ps1` run the oracle under six pinned environments and exact-match stdout against `test-fixtures/*.json` (each fixture twice — determinism check): full-capability 2.1.206, Agent-Teams opt-in, workflows-disabled, old 2.1.150 (version-keying proof), version-undetectable, and `non-claude-all-false` (the HAL_CAPABILITY_MAP § 3.2 guarantee). The env-per-fixture mapping is hardcoded in the validators by design (fixtures + validator ship together; goldens stay hermetic).

## Consumers

`segments/_hal-gate.md` (the ONLY sanctioned branch pattern) → HAL-gated skill sites (PODS worktree fast path, finalize/critic review-skill branches, collab-sync Monitor watch mode, proceed SPODS routing, …). Session-start briefing eligible (`typical_sessions_using: every`).
