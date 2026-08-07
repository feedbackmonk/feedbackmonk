# autonomy-status Oracle

**Kind**: project-state
**Question answered**: *What is the resolved autonomy level for this session, what is its source, and what is the resolved per-domain consultation vector?*

## Purpose

The autonomy cascade defined in `~/.claude/skills/0-uldf-autonomy-set/SKILL.md` § Status Resolution Order (Canonical Cascade) is the single canonical consent gate for model-invocable framework operations (per DEC-39). Without this oracle, the four-step cascade resolution would duplicate across:

- `~/.claude/hooks/session-start.sh` and `session-start.ps1`
- `/0-uldf-autonomy-set` no-args status display
- `/0-uldf-proceed` chain-boundary re-resolution
- Ad-hoc consumers across LDIS / LTADS / PODS commands

Centralizing in a single oracle inverts the redundant-investigation tax: each consumer invokes once, reads the JSON, and trusts the result.

## Cascade Order

The oracle resolves these sources in order, returning the first that produces a valid level (with caps and skips applied):

| # | Source | Skip condition | Cap |
|---|--------|----------------|-----|
| 1 | Session override (caller-supplied via `--session-override=<level>`) | Empty argument | none |
| 2 | `ltads/arc-state.json` topmost-arc `autonomyOverride` field (ARC-03/DEC-199) | topmost arc `CONCLUDED` or `PAUSED` | none |
| 3 | `.claude/session-state/task-arc-autonomy.json` | Expired (`expires_at` past) OR grantor PID dead | none |
| 4 | `ltads/config.json` `autonomy.default` | none | Value `autopilot`/`supervised` is **neutralized to `collaborative`** - project config cannot hold durable loosening (AUTODEF-03) |
| 5 | `~/.claude/machine-autonomy.json` (**machine default**, AUTODEF-02) | Absent or malformed -> skipped silently | **Capped downward** by step 4 (see below) |
| 6 | Default | n/a | `collaborative` |

### Steps 4 and 5 resolve TOGETHER (AUTODEF-03)

The machine default is not a plain first-wins step below config: **the resolved level is the MORE consultative of (project config, machine default)** - projects tighten, never loosen.

| Project config | Machine default | Resolves to | `source` |
|---|---|---|---|
| *(none)* | *(none)* | `collaborative` | `default` |
| *(none)* | `autopilot:director` | `autopilot` + `submode: director` | `machine-default` |
| `collaborative` | `autopilot:director` | `collaborative` | `config-cap` |
| `controlled` | `autopilot:director` | `controlled` | `config-cap` |
| `collaborative` | `manual` | `manual` | `machine-default` |
| `collaborative` | *(none)* | `collaborative` | `config` |
| `autopilot` (legacy) | any | neutralized to `collaborative`, advisory retargeted to `--machine-default` | `config` / `config-cap` |

**Machine-default file absent means behavior is byte-identical to pre-AUTODEF.** Synced consumers see zero behavior change until their own user speaks the grant; the file is per-machine, never committed, never synced, and an agent must not write it on its own initiative (AUTODEF-02 - same consent class as sync).

Schema (frozen at first commit):

```json
{ "schemaVersion": 1, "level": "autopilot:director", "setAt": "<ISO-8601Z>", "setBy": "user-word", "note": "<free text>" }
```

**Test seam**: `ULDF_MACHINE_AUTONOMY_FILE` repoints the machine-default path. It exists so smoke harnesses stay hermetic once a real grant is present on the developer's machine - it is not a production configuration knob.

### Submodes and the `director` pin (AUTODEF-01)

Every designation (`--session-override`, the LTADS override line, the arc grant, the machine default) is parsed by ONE routine into base level + autopilot submode, emitted as `submode` (`continuous`/`phase`/`session`/`task`/`director`; `null` for non-autopilot). *A submode the resolver cannot parse is a silent downgrade* - that was DISC-AUTO-01, and a single parse point is the structural answer.

`autopilot:director` is the `autopilot` row with exactly one pin: the `spec` domain's **clamp floor** is `ask-major`. The pin is applied as the level default, so the ordinary tighten-only machinery does the rest - `spec=auto` from any store is a *loosening* request and is clamped + reported in `domain_clamped`, while `spec=ask-all` still tightens further. No store can configure the conversational spec phase away; only a user can make it more conversational.

## Domain Vector (AUTODOM-01..05, DEC-172)

Since **v1.1.0** this oracle also resolves the six autonomy **domain settings** — and it is the *only* thing on disk that does. Before AUTODOM, `/0-uldf-autonomy-set commit=approve-diff` wrote a setting that **zero** scripts, hooks, or oracles ever read; the setting did literally nothing. (That zero-consumer finding is what put domain settings on the 2026-07-12 retire list; the disposition was to *wire* them, because the zero usage was fully explained by the missing consumer. This oracle is that consumer.)

**Two stores**, read in this precedence (later wins, **per domain** — not whole-object):

| # | Store | Scope | Written by |
|---|-------|-------|-----------|
| 1 | `ltads/config.json` → `autonomy.domains` | Persistent | `/0-uldf-autonomy-set <domain>=<option> --persist` |
| 2 | `.claude/session-state/autonomy-domains.json` → `domains` | Session/arc | `/0-uldf-autonomy-set <domain>=<option>` |

Both are optional; **absence is the common case** and the resolved vector is then exactly the level-default row.

### The frozen level→domain default matrix (AUTODOM-03)

| Level | `spec` | `plan` | `delegate` | `decide` | `quality` | `commit` |
|---|---|---|---|---|---|---|
| `autopilot` | auto | auto | auto | auto-all | auto | auto |
| `supervised` | critical-only | summarize | summarize | ask-critical | report | auto |
| `collaborative` | ask-major | approve | approve-roles | ask-major | report | auto |
| `controlled` | ask-all | approve | approve-prompts | ask-all | approve | approve-message |
| `manual` | ask-all | step-by-step | approve-prompts | ask-all | approve | approve-diff |

**`commit` is `auto` at collaborative, supervised, and autopilot on purpose.** `~/.claude/CLAUDE.md` § Propagation Operations (DEC-116 form 5) makes **finalize** the commit-consent surface: after verified, directed work, committing is already in-authority at collaborative+. A default of `approve-message` there would manufacture exactly the consent gate DEC-116 / COMMS-GATE-02 bans. Do not "fix" this row.

### Tighten-only (AUTODOM-04) — the safety property

Each domain has an ordering from **least** to **most** consultative:

| Domain | Ordering (least → most consultative) |
|---|---|
| `spec` | `auto` → `critical-only` → `ask-major` → `ask-all` |
| `plan` | `auto` → `summarize` → `approve` → `step-by-step` |
| `delegate` | `auto` → `summarize` → `approve-roles` → `approve-prompts` |
| `decide` | `auto-all` → `ask-critical` → `ask-major` → `ask-all` |
| `quality` | `auto` → `report` → `approve` |
| `commit` | `auto` → `approve-message` → `approve-diff` |

An override may only move a domain **toward more consultation**. A **loosening** override is **CLAMPED** back to the level default and reported in `domain_clamped` — never silently honored.

*Why this rule exists*: without it, a domain override is an **autonomy-escalation backdoor around the level ladder**. `manual` + `commit=auto` would grant unattended commits that `/0-uldf-autonomy-set autopilot --persist` is explicitly forbidden from granting (the cascade caps `ltads/config.json` at `collaborative` for exactly this reason). Domain settings would become the cheap way to buy the autonomy the persistence restrictions deny. Tighten-only means a domain override can add friction but never remove it — so it is always safe to honor, from any store, at any level.

An **unrecognized** option value is likewise ignored (level default stands) and reported in `domain_invalid` — a typo'd *tightening* request must never be silently dropped into "less consultation than you asked for".

## Output Schema (v1.2.0 — AUTODEF fields additive over v1.1.0, itself additive over the frozen v1.0.0 core)

```json
{
  "level": "autopilot|supervised|collaborative|controlled|manual",
  "submode": "continuous|phase|session|task|director|null",
  "source": "session-override|ltads-session|task-arc-autonomy|config|config-cap|machine-default|default",
  "arc_id": "<uuid>|null",
  "expires_at": "<ISO-8601>|null",
  "source_detail": "<human-readable description>",
  "briefing": "<JSON string for [autonomy] briefing line; empty when level==collaborative AND no domain override is active>",

  "domains":          { "spec": "…", "plan": "…", "delegate": "…", "decide": "…", "quality": "…", "commit": "…" },
  "domain_overrides": ["commit"],
  "domain_clamped":   [{ "domain": "decide", "requested": "auto-all", "clamped_to": "ask-major" }],
  "domain_invalid":   [{ "domain": "quality", "requested": "bogus-option" }]
}
```

- **`domains` is the field consumers read.** All six keys are ALWAYS present, already resolved (level default, tightened by any honored override). A consumer never needs to know whether an override existed — it reads its one domain and obeys.
- `domain_overrides` / `domain_clamped` / `domain_invalid` are for *display and audit* (the `/0-uldf-autonomy-set` status block, the session-start briefing). A consumer enforcing a domain does not need them.
- `briefing` is pre-formatted so `session-start.{sh,ps1}` can emit `[autonomy] <briefing>` directly. Empty string signals "no line" (the quiet default). **AUTODOM-05**: it also fires at `collaborative` when any override/clamp/invalid is active — an active domain override must be unavoidably in context at session start, not invisible. **AUTODEF-04**: it is emitted UNCONDITIONALLY when `source` is `machine-default` or `config-cap` (never gracefully absent), carrying `submode` and — for a cap — `capped_from`. An ambient standing grant that governs every session on the machine must be unmissable in every session it governs, and a *capped* project has to be able to show the user why it is not ambient.
- The `submode` field and the two new `source` values are ADDITIVE: consumers that switch on `source` treat unknown values as "some resolved source", and the session-start hooks print `briefing` verbatim, so they needed no change.

The v1.0.0 core fields are unchanged in name, type, and value; the four domain fields are purely additive. A consumer written against v1.0.0 keeps working.

## Four-Part Qualification (per Oraculurgy Design § 2.3)

| Part | Status | Rationale |
|------|--------|-----------|
| **Deterministic** | ✓ | Inputs are file contents + system clock + PID liveness. Same inputs → same output (modulo TTL boundary crossing). |
| **Recurrent** | ✓ | Every session-start; every chain-boundary re-resolution in `/0-uldf-proceed`; every status display in `/0-uldf-autonomy-set`. ≥3 calls per session typical. |
| **Freshness-contractable** | ✓ | Re-fire on any change to: `ltads/arc-state.json`, `.claude/session-state/task-arc-autonomy.json`, `ltads/config.json`. The session-start hook already invokes once per session-start; that is the freshness contract. |
| **Gracefully absent** | ✓ | If oracle is missing or fails, callers fall back to the documented default `collaborative`. Falling back to default is identical to the current behavior of consumers that don't yet read autonomy at all — zero new failure mode. |

## Consumers

| Consumer | Wiring point | Field used |
|----------|-------------|------------|
| `session-start.{sh,ps1}` | Phase 1d (autonomy briefing) | `briefing` (emitted directly), all fields cached for downstream env-var if needed |
| `/0-uldf-autonomy-set` (no-args) | Status display | `level`, `source`, `source_detail`, `domains`, `domain_overrides`, `domain_clamped`, `domain_invalid` |
| `/0-uldf-proceed` | Chain-boundary re-resolution; delegation/topology consent | `level`, `domains.delegate` |
| `/0-uldf-ldis-intake` / `/0-uldf-ldis-plan` | Auto-chain gate evaluation; plan-presentation consent | `level`, `domains.plan` |
| `/0-uldf-ldis-spec` | Spec-gap consultation | `level`, `domains.spec` |
| `/0-uldf-finalize` | Commit/push consent point | `level`, `domains.commit` |
| `/0-uldf-quality` | Quality-finding disposition | `level`, `domains.quality` |
| `/0-uldf-ltads-admin` | Decision-gate authoring/resolution | `level`, `domains.decide` |

**Consumer contract (AUTODOM-02)**: read your one key out of `domains` and obey it. The value is already resolved and already clamped — a consumer never needs to re-derive the level default, and **must not** re-widen a value it finds too strict. A domain value can only be at-or-tighter than the level's own behavior; obeying it can therefore never violate the level.

## Smoke Harness

Two harnesses, both must pass:

**`~/.claude/scripts/autonomy-tests/autonomy-status-smoke.{sh,ps1}`** — the cascade (23 legs):

1. No override / no LTADS / no config → returns `default:collaborative` with empty briefing
2. `arc-state.json` topmost arc has `autonomyOverride: "autopilot"`, status ACTIVE → returns `ltads-session:autopilot`
3. `arc-state.json` topmost arc CONCLUDED, override field present → SKIPS step 2, falls to default
4. `task-arc-autonomy.json` valid (TTL future, grantor alive) → returns `task-arc-autonomy:autopilot`
5. `task-arc-autonomy.json` expired → SKIPS step 3, falls through
6. `task-arc-autonomy.json` grantor PID dead → SKIPS step 3, falls through
7. `config.json` `autonomy.default: autopilot` → CAPPED to `collaborative`, source=`config`
8. `config.json` `autonomy.default: collaborative` → returns `config:collaborative`

**`~/.claude/scripts/smoke-tests/autonomy-domains-smoke.sh`** — the domain vector + AUTODEF cascade (89 legs): level-default resolution for all five levels (incl. an explicit assertion that `commit=auto` at collaborative/autopilot, so a "helpful" tightening of the DEC-116 row fails loudly), honored tightening, **clamped loosening**, invalid value, unchanged no-override path, AUTODOM-05 briefing-fires-at-collaborative, two-store per-domain precedence, the **AUTODEF** legs (director submode + spec clamp floor, machine-default source, config-cap in both directions, legacy-config neutralization, malformed-file graceful absence, absent-file byte-identity, and a static assertion that the sync scripts never name the machine-default file), and **sh/ps1 byte parity over every fixture** — the leg that caught a PowerShell single-quote (`\u0027`) escaping divergence in the AUTODEF detail strings during this very build.

## Cross-References

- **Spec**: `docs/specs/SPECIFICATION.md` § AUTONOMY-02, § AUTODOM (AUTODOM-01..05), § AUTODEF (AUTODEF-01..04)
- **Decisions**: DEC-39 (canonical cascade gate), DEC-40 (task-arc-autonomy.json schema), DEC-172 (domain settings wired, not retired; tighten-only clamp), DEC-189 (machine-default cascade source, `autopilot:director`, downward cap)
- **Cascade definition (canonical)**: `~/.claude/skills/0-uldf-autonomy-set/SKILL.md` § Status Resolution Order
- **Domain settings (canonical)**: `~/.claude/skills/0-uldf-autonomy-set/segments/domain-settings.md`
- **Briefing wiring**: `~/.claude/hooks/session-start.{sh,ps1}` (AUTONOMY-03)
