# solicitation-invariant-check

> Verification Oracle (`kind: "verification"`). Defends the load-bearing privacy
> invariant of the per-user feedback-solicitation feature (FR-FBR-29):
> **`opted_out` is a TERMINAL state.**

## Purpose

A consumer (e.g. GitCellar Desktop) shows an ambient "got a minute for feedback?"
nudge to engaged users. feedbackmonk owns the **durable** record of whether a
given end-user may be asked, keyed by the stable JWT `sub`, so the consumer can
honor "ask at most ~twice/year, honor dismissal, honor opt-out" **even across
client reinstalls**. The single most load-bearing promise in that record is:

> Once a user opts out, that decision is FOREVER. No event other than
> `opted_out` is ever honored again, and the state machine cannot be bypassed.

This oracle proves that invariant **from CODE**, not from a self-reported
`opted_out` flag. It is the **anti-reward-hacking leg** of the feature, mirroring
the sibling trust-boundary oracles `approval-gate-enforcement` (work-order
approval) and `public-board-moderation-gate` (board moderation).

Why an oracle and not just a unit test: a happy-path test (`opt out → status ==
opted_out`) confirms the success case but cannot prove the **absence** of a code
path that honors a `prompted` / `dismissed` / `gave_feedback` event after
opt-out, nor that the HTTP handler can be rewired to `upsert` a new status
**without** routing through the terminal guard. An opted-out sub silently
becoming eligible again is the exact privacy regression this feature exists to
prevent.

## The invariant defended

`opted_out` is terminal:

1. The `SolicitationStatus::OptedOut` variant exists.
2. `is_opted_out()` classifies **exactly** `OptedOut` (never widened).
3. `apply_event` rejects **every** non-opt-out event once opted out, via the
   guard `_ if current.is_opted_out() => Err(SolicitationError::OptedOut)`.
   (The `OptedOut` *event* itself returns `Ok(OptedOut)` from any state —
   idempotent re-opt-out.)
4. The HTTP handler never writes a status that did not pass through the state
   machine, and authenticates only **JWT identity-class** callers (DEC-FBR-04).

## The three probes (detection-from-code)

| Probe | What it asserts | Source | When |
|---|---|---|---|
| **A — state machine** | In `crates/feedbackmonk-core/src/solicitation.rs`: (1) `OptedOut` variant exists; (2) `is_opted_out` matches EXACTLY `Self::OptedOut`; (3) `apply_event` carries the terminal guard returning `Err(SolicitationError::OptedOut)` when `current.is_opted_out()` for non-opt-out events. | static source parse | **LIVE** |
| **B — handler binding** | In `crates/feedbackmonk-api/src/handlers/solicitation.rs`: (1) `post_solicitation_event` routes every `upsert(` write through a prior `apply_solicitation_event(...)` call (no write bypasses the machine); (2) auth requires `list_active_for_class(&scope, KeyClass::Identity)` (DEC-FBR-04, identity-class JWT only). | static source scan | **LIVE** |
| **C — behavior** (`--full`) | Runs `cargo test -p feedbackmonk-api --test solicitation_integration` against the real DB — the drift-detection leg so the static probes and live behavior cannot silently diverge. | integration test | **PENDING** until `crates/feedbackmonk-api/tests/solicitation_integration.rs` lands (authored concurrently by the main session) |

Probe A mirrors `public-board-moderation-gate` Probe A (which asserts
`is_publicly_visible` matches EXACTLY `Approved`). Probe B mirrors
`approval-gate-enforcement` Probe B (handler must consult the security
predicate). Probe C never runs in the default A+B invocation.

## Invocation

```bash
# Default (Probes A + B only — fast, no DB, no cargo):
python .claude/oracles/solicitation-invariant-check/oracle.py
bash   .claude/oracles/solicitation-invariant-check/oracle.sh      # Unix shim
pwsh   .claude/oracles/solicitation-invariant-check/oracle.ps1     # Windows shim

# With Probe C (behavioral drift-detection — requires DB + cargo):
python .claude/oracles/solicitation-invariant-check/oracle.py --full
```

Exit `0` on PASS, `1` on FAIL, `2` on environment failure. Probe C reports
PENDING (not failure) until the integration test exists.

## Lineage

- **FR-FBR-29** — per-user feedback-solicitation state (GitCellar in-app nudge, Capability 2)
- **DEC-FBR-IMPL-24** — `opted_out` is a TERMINAL state (the load-bearing privacy promise)
- **DEC-FBR-04** — JWT identity is the ONLY identity feedbackmonk holds; identity-class keys only
- **DEC-FBR-IMPL-03** — canonical-Python oracle + thin shims pattern
- Mirrors **`approval-gate-enforcement`** (work-order approval trust boundary) and
  **`public-board-moderation-gate`** (board moderation trust boundary) — the
  sibling anti-reward-hacking trust-boundary oracles.
- Probandurgy Verification Oracle pattern — `FOUNDATIONS/ORACULURGY_DESIGN.md` Part 11.
