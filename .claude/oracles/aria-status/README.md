# `aria-status` Oracle

## Synopsis

Project-state oracle reporting ARIA instrumentation health (surface present, endpoint reachable, foundation-layer status) and emitting the `[aria-status]` ORACLE BRIEFING line — **Leg A** of ARIA's four-leg detection machinery (DEC-26, ARIA-03), the predictive session-start surface. Come here for endpoint-probe semantics (300ms hard ceiling), the five briefing-line forms, the surface-present rule, and the optional `.claude/aria.json` config schema. Don't come here for the surface-classification logic itself — that delegates to the sibling `ui-surface-detector` (ARIA-02, Leg B).

## Purpose

Answers *"What is the ARIA instrumentation status of this project?"* — surface present? endpoints reachable? foundation-layer healthy? — and emits a single-line briefing for the session-start ORACLE BRIEFING block.

This oracle is **Leg A** of ARIA's four-leg detection machinery (Phase 1 per DEC-26, ARIA-03). Sibling: `ui-surface-detector` (ARIA-02), which this oracle uses for surface detection.

Spec: ARIA-01, ARIA-03 in `docs/specs/SPECIFICATION.md`. Contract: ARIA-07.

## Output

```json
{
  "surface_present": true,
  "exposure_mechanism": "http",
  "endpoint_reachable": true,
  "endpoint_url": "http://127.0.0.1:14550/aria/health",
  "foundation_layer": { "errors": true, "async": true, "navigation": true },
  "recent_success_at": "2026-04-29T15:00:00Z",
  "query_count_24h": 12,
  "briefing": "ARIA: errors+async+navigation healthy (qph=0)"
}
```

## Briefing-Line Forms (ARIA-01 acceptance #5)

| Case | Briefing |
|------|----------|
| present-and-healthy | `ARIA: errors+async+navigation healthy (qph=N)` |
| present-but-degraded | `ARIA: <healthy cats> healthy; <degraded cats> UNREACHABLE — see /0-uldf-oracle aria-status` |
| instrumented-but-unreachable (`.claude/aria.json` exists, server down) | `ARIA: configured but endpoint unreachable at <url>` |
| surface-but-no-instrumentation (no `.claude/aria.json`, server down) | `ARIA: UI/runtime surface detected; no ARIA instrumentation. /0-uldf-ldis-plan can scaffold.` |
| no-surface | empty string (line suppressed by hook) |

`qph` (queries per hour) = `query_count_24h / 24`, computed from `.claude/aria-telemetry.jsonl`.

Briefing is always ≤200 chars (defensively truncated).

## Surface-Present Rule

The oracle delegates to `ui-surface-detector`. `surface_present: true` when `surface_kind ∉ {none, cli-tool}`. CLI tools may eventually expose ARIA-style instrumentation, but the foundation-layer endpoints (errors, async, navigation) target UI/service runtime perception per DEC-26.

## Endpoint Probe

- Default endpoint: `http://127.0.0.1:14550/aria/health` (per ARIA-07 contract). Override via `.claude/aria.json` `endpoint_url` field.
- Probe timeout: **300ms hard ceiling**. On timeout → `endpoint_reachable: false`.
- Response validation: must include `_meta` envelope + `oracleStatus`. Schema-violating responses are treated as unreachable.
- Foundation-layer derivation:
  - `oracleStatus: "healthy"` → all three categories `true`
  - `oracleStatus: "degraded"` → `_meta.degradedCategories[]` flips matching booleans to `false`; rest remain `true`
  - Server unreachable → all three `false`

## Telemetry

`query_count_24h` (optional) is computed by counting JSONL entries in `.claude/aria-telemetry.jsonl` with `timestamp` ≥ now − 24h. The MCP server (ARIA-06) writes this file. Empty/missing log → field omitted.

## Invocation

```bash
# Unix
bash .claude/oracles/aria-status/run.sh

# Windows
powershell -NoProfile -File .claude/oracles/aria-status/run.ps1
```

Invoked automatically by the session-start hook in parallel with other every-session oracles. The 200ms `compute_cost_ms` reflects typical (cache-warm, loopback-fast) cost; the 300ms probe ceiling is the defensive worst case.

## Configuration

Optional `.claude/aria.json`:

```json
{
  "endpoint_url": "http://127.0.0.1:14550/aria/health",
  "exposure_mechanism": "tauri-ipc"
}
```

Both fields optional; the oracle uses defaults (`http`, default endpoint URL) if absent.

## Behavior on Edge Cases

- **No surface**: emits `surface_present: false`, all foundation flags `false`, briefing empty (suppressed). Other fields are degraded defaults.
- **Surface present, server unreachable, no aria.json**: surface-but-no-instrumentation briefing.
- **Surface present, server unreachable, aria.json exists**: instrumented-but-unreachable briefing.
- **Server response missing `_meta`**: treated as unreachable (contract violation).
- **Telemetry log corrupt**: `query_count_24h` field omitted; `qph` defaults to `0`.

## Validation

```bash
bash .claude/oracles/aria-status/validate.sh
powershell -NoProfile -File .claude/oracles/aria-status/validate.ps1
```

Validators check JSON well-formedness, required fields, enum values, briefing length, and the surface_present/briefing invariant.

## Cross-References

- Spec: ARIA-01, ARIA-03 (ULDF `docs/specs/SPECIFICATION.md`)
- Contract: ARIA-07 (`docs/specs/SPECIFICATION.md`); binding contract document during PODS Phase 1 at `.claude/collaboration/collab-20260429-145000/channels/aria-07-contract.md`
- Decisions: DEC-25..DEC-29 (`docs/specs/DECISIONS.md`)
- Design: `FOUNDATIONS/ARIA_INTEGRATION_DESIGN.md` § 3 (architecture), § 4 (endpoint contract)
- Sibling: `ui-surface-detector` (ARIA-02) — surface-detection helper
- Cross-track validator: `claude-template/scripts/validate-aria-contract.{sh,ps1}` Track-A section
