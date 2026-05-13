# autonomy-status Oracle

**Kind**: project-state
**Question answered**: *What is the resolved autonomy level for this session, and what is its source?*

## Purpose

The autonomy cascade defined in `commands/-autonomy/set.md` § Status Resolution Order is the single canonical consent gate for model-invocable framework operations (per DEC-39). Without this oracle, the four-step cascade resolution would duplicate across:

- `claude-template/hooks/session-start.sh` and `session-start.ps1`
- `/0-uldf-autonomy-set` no-args status display
- `/0-uldf-proceed` chain-boundary re-resolution
- Ad-hoc consumers across LDIS / LTADS / PODS commands

Centralizing in a single oracle inverts the redundant-investigation tax: each consumer invokes once, reads the JSON, and trusts the result.

## Cascade Order

The oracle resolves these sources in order, returning the first that produces a valid level (with caps and skips applied):

| # | Source | Skip condition | Cap |
|---|--------|----------------|-----|
| 1 | Session override (caller-supplied via `--session-override=<level>`) | Empty argument | none |
| 2 | `ltads/sessions/current-session.md` `**Autonomy Override**` line | Status = `CONCLUDED` or `PAUSED` | none |
| 3 | `.claude/session-state/task-arc-autonomy.json` | Expired (`expires_at` past) OR grantor PID dead | none |
| 4 | `ltads/config.json` `autonomy.default` | none | If value is `autopilot` or `supervised`, **capped to `collaborative`** per DEC-12 Halt Principle |
| 5 | Default | n/a | `collaborative` |

## Output Schema (Frozen at v1.0.0)

```json
{
  "level": "autopilot|supervised|collaborative|controlled|manual",
  "source": "session-override|ltads-session|task-arc-autonomy|config|default",
  "arc_id": "<uuid>|null",
  "expires_at": "<ISO-8601>|null",
  "source_detail": "<human-readable description>",
  "briefing": "<JSON string for [autonomy] briefing line; empty when level==collaborative>"
}
```

`briefing` is intentionally pre-formatted by the oracle so `session-start.{sh,ps1}` can directly emit `[autonomy] <briefing>` without further composition. Empty string signals "no line should be emitted" (gracefully absent at the common-case default).

## Four-Part Qualification (per Oraculurgy Design § 2.3)

| Part | Status | Rationale |
|------|--------|-----------|
| **Deterministic** | ✓ | Inputs are file contents + system clock + PID liveness. Same inputs → same output (modulo TTL boundary crossing). |
| **Recurrent** | ✓ | Every session-start; every chain-boundary re-resolution in `/0-uldf-proceed`; every status display in `/0-uldf-autonomy-set`. ≥3 calls per session typical. |
| **Freshness-contractable** | ✓ | Re-fire on any change to: `ltads/sessions/current-session.md`, `.claude/session-state/task-arc-autonomy.json`, `ltads/config.json`. The session-start hook already invokes once per session-start; that is the freshness contract. |
| **Gracefully absent** | ✓ | If oracle is missing or fails, callers fall back to the documented default `collaborative`. Falling back to default is identical to the current behavior of consumers that don't yet read autonomy at all — zero new failure mode. |

## Consumers

| Consumer | Wiring point | Field used |
|----------|-------------|------------|
| `session-start.{sh,ps1}` | Phase 1d (autonomy briefing) | `briefing` (emitted directly), all fields cached for downstream env-var if needed |
| `/0-uldf-autonomy-set` (no-args) | Status display | `level`, `source`, `source_detail` |
| `/0-uldf-proceed` | Chain-boundary re-resolution after `/0-uldf-ltads-stop` / `/0-uldf-pods-converge` | `level` |
| `/0-uldf-ldis-intake` / `/0-uldf-ldis-plan` | Auto-chain gate evaluation | `level` |

## Smoke Harness

`claude-template/scripts/autonomy-tests/autonomy-status-smoke.{sh,ps1}` exercises ≥8 cascade cases:

1. No override / no LTADS / no config → returns `default:collaborative` with empty briefing
2. `current-session.md` has `**Autonomy Override**: autopilot`, Status: ACTIVE → returns `ltads-session:autopilot`
3. `current-session.md` has Status: CONCLUDED, override line present → SKIPS step 2, falls to default
4. `task-arc-autonomy.json` valid (TTL future, grantor alive) → returns `task-arc-autonomy:autopilot`
5. `task-arc-autonomy.json` expired → SKIPS step 3, falls through
6. `task-arc-autonomy.json` grantor PID dead → SKIPS step 3, falls through
7. `config.json` `autonomy.default: autopilot` → CAPPED to `collaborative`, source=`config`
8. `config.json` `autonomy.default: collaborative` → returns `config:collaborative`

## Cross-References

- **Spec**: `docs/specs/SPECIFICATION.md` § AUTONOMY-02
- **Decisions**: DEC-39 (canonical cascade gate), DEC-40 (task-arc-autonomy.json schema)
- **Cascade definition (canonical)**: `claude-template/skills/0-uldf-autonomy-set/SKILL.md` § Status Resolution Order
- **Briefing wiring**: `claude-template/hooks/session-start.{sh,ps1}` (AUTONOMY-03)
