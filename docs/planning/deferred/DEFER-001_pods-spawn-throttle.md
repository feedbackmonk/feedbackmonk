# DEFER-001: Throttle PODS worker spawning to prevent burst-spawn machine crash

**Status**: DECLINED — SUPERSEDED (2026-08-06, `defer-drain-FeedbackMonk-20260806`)
**Deferred**: 2026-06-18

> ## Disposition — the brief's central claim was already false when it was written
>
> **Historical below this banner; do not cite the body as current.** The incident narrative
> (§ Extracted Context) is accurate and is preserved as the only account of the 2026-06-18 crash.
> The *diagnosis of the framework's state* is not.
>
> **What was measured (2026-08-06, ULDF framework repo `S:/ULDF` + synced `~/.claude/`):**
>
> - The fix this brief proposes **already shipped on 2026-06-09** — commit `432cc833`,
>   *"fix(pods-spawn): serialize Windows `--all` ConPTY tab-adds to stop wt.exe burst-crash
>   (DISC-SPAWN-02)"* — **nine days before** the 2026-06-18 incident that generated this brief.
>   The premise *"no throttle, no readiness gate, and no concurrency cap"* (§ Summary) was
>   therefore untrue at the time of writing.
> - `~/.claude/scripts/spawn-pods-all.ps1` implements proposals **1–3 verbatim**: serialize
>   (one worker at a time), a **readiness gate** polling the worker's `shell.pid` until it names
>   a live process with a bounded timeout — the brief's own preferred design over a fixed sleep —
>   a settle delay, fail-loud abort (remaining workers are NOT spawned on timeout), and pacing
>   configurable via `.claude/config.json` `podsSpawn`
>   (`verifyTimeoutSeconds` / `windowsSettleSeconds` / `sequential`). 7/7 smoke harness at
>   `scripts/hygiene-tests/spawn-pods-all-smoke.ps1`.
> - It is the **mandated** Windows `--all` path (`segments/-pods/spawn-collaborator_prompt-template.md`
>   Step 5b BURST GUARD; `skills/0-uldf-pods-spawn-collaborator/SKILL.md` Notes).
> - **All four Open Questions are answered** by what shipped: readiness signal is the worker's
>   `shell.pid` (not `claudeShellPid`), with a liveness re-check on timeout; defaults are 60s
>   verify / 4s settle; the `.sh`/tmux path was **deliberately not mirrored** (the crash is
>   `wt.exe`/ConPTY-specific — DISC-SPAWN-02 note (c)); and the worker-count warning is subsumed
>   by fail-loud abort.
> - The guard's own follow-on defects were separately found and drained in ULDF
>   (cold-start false-negative → duplicate spawn; registry clobber; idempotent resume adoption,
>   ULDF `SPECIFICATION.md` RESUME-01/02) — i.e. this subsystem has had three rounds of
>   hardening since this brief was filed.
> - **Only proposal 4 never shipped** — the optional pre-spawn free-memory check. No observed
>   incident has required it; it is not carried forward as work.
>
> **Why the 2026-06-18 crash happened anyway — attribution NOT established.** The registry
> timings in § Extracted Context (spawns at 18:13:40 / :46 / :52, ~6s apart, while CLAUDE-B's pid
> took 19s to appear) are **incompatible with the orchestrator having been used** — its
> verify-between gate cannot emit a second spawn 6s after the first when the first has not
> verified. So the LD fired a per-worker loop. Whether it did so because the guard had not yet
> been synced into `~/.claude/` (a user-consent-gated propagation op, so lag is by design) or
> because it bypassed a guard that was present is **undeterminable** — `~/.claude/` carries no
> sync-provenance record. That residual — enforcement against a *direct-spawn* burst is prose,
> not a hook — is recorded as one line in
> [`docs/planning/observations-ledger.md`](../observations-ledger.md) rather than filed as a
> brief into ULDF, per the 2026-08-06 filing gate (real harm, but unattributable).
>
> **No feedbackmonk action. No ULDF brief.** This item is closed.
**Deferred from**: feedbackmonk P5a Stage 1 — a PODS session (`collab-20260618-180600`, 3 workers) crashed all terminals during the worker-spawn burst; this brief was captured during recovery.
**Scope**: ⚠️ **CROSS-REPO — the fix belongs in the ULDF framework repo (`claude-template/` → synced `~/.claude/`), NOT in feedbackmonk.** Captured here only because that is where the incident occurred. Port to the framework repo and dismiss this item once filed there.

## Summary

`/0-uldf-pods-parallelize` + the PODS worker spawn launch N heavy Claude Code sessions through `wt.exe` in a rapid burst with **no throttle, no readiness gate, and no concurrency cap**. On 2026-06-18, spawning 3 workers within ~12 seconds wedged the machine and crashed *every* terminal ~14 seconds later — before any worker did a single unit of work. The fix is to **sequence spawns** (one at a time, with a readiness signal or delay between each) instead of bursting them. Any PODS session with 3+ workers on this machine currently risks a full mid-spawn crash; the recovery cost on 2026-06-18 was an entire lost LD session.

## Extracted Context

### What Was Discussed

The PODS session `collab-20260618-180600` was set up correctly: GUIDE.md, three task files, spawn-prompts, and channels all written; Stage 0 (frozen contracts C22/C23/C24 + data-model foundation) committed cleanly to `main` (`0872e8a`). The LD then spawned all three workers. From the session registry (`active-sessions.json`):

- CLAUDE-A spawned 18:13:40Z, claudeShellPid written 18:13:47Z
- CLAUDE-B spawned 18:13:46Z, claudeShellPid written 18:14:05Z
- CLAUDE-C spawned 18:13:52Z, **claudeShellPid never written (null)** — it never finished initializing
- Mass death at 18:14:06Z — the SessionEnd hook flipped CLAUDE-A and CLAUDE-B to `closed`; CLAUDE-C was left a stale `active` entry with a dead PID.

The three spawns landed ~6 seconds apart, and the whole machine's terminals went down ~14 seconds after the third. The workers had done **zero** work (all at `SPAWNING / 0% / Awaiting spawn`), so the crash is attributable to the spawn burst itself, not to any work.

A process probe during recovery confirmed the only surviving `claude.exe` processes belonged to a *different* project (SessionHelm, parented by `session-helm.exe`) — i.e. the feedbackmonk worker processes were genuinely killed, and there were no feedbackmonk orphans to clean up.

### Key Decisions Already Made

- **Diagnosis (high confidence)**: burst-spawn of N heavy Claude Code sessions via `wt.exe` overwhelmed Windows Terminal / ConPTY and/or memory. Corroborated by CLAUDE-C never initializing (null `claudeShellPid`) and by this machine's documented prior `wt.exe` multi-spawn fragility (DISC-SPAWN-01, where a single handoff exploded into ~7 `wt.exe` tabs).
- **Restoration approach adopted**: respawn workers **sequentially with a stability check between each** — chosen specifically to avoid re-triggering the burst crash. This validates the proposed fix in practice.

### Constraints & Requirements

- Fix must live in the framework, not in any consuming project.
- Must preserve PODS' parallel-execution model — the throttle is only on the *spawn*, not on the workers running concurrently afterward.
- Should be Windows-aware (`wt.exe`/ConPTY) but ideally cross-platform (the `.sh` spawn path on mac/Linux tmux may not need it, or may want a smaller delay).
- Backward compatible: existing `/0-uldf-pods-spawn-collaborator --all` callers should keep working, just throttled.

### References

- `~/.claude/scripts/spawn-claude-session.ps1` — the Windows spawn mechanism (and `.sh` peer for mac/Linux).
- `/0-uldf-pods-spawn-collaborator` skill (the `--all` fan-out path) — `claude-template/skills/0-uldf-pods-spawn-collaborator/`.
- `/0-uldf-pods-parallelize` skill — sets up the session and may trigger the spawn.
- DISC-SPAWN-01 / DEC-68 — prior `wt.exe` multi-spawn fragility on this machine (file-first handoff delivery was the earlier fix for a related symptom).
- PW-009 (`docs/PLATFORM_WORKAROUNDS.md`) — related class of "background process trees wedge the machine" incident.
- Evidence snapshot: `.claude/collaboration/collab-20260618-180600/` registry + `active-sessions.json` (closed CLAUDE-A/B, stale CLAUDE-C).

## Scope Assessment

- **Estimated size**: Single session (framework-repo change to one script + one skill, plus a test/manual verification).
- **Suggested entry point**: Direct implementation in the framework repo (well-understood fix), or `/0-uldf-ldis-plan` there if the readiness-gate design needs scoping.
- **Dependencies**: None blocking. Should be fixed before the next large (3+ worker) PODS session on this machine.
- **Related items**: DISC-SPAWN-01 / DEC-68 (same spawn subsystem); PW-009 (machine-wedge class).

## Proposed Fix (carry into the framework session)

1. **Sequence, don't burst**: in the `--all` fan-out, spawn one worker, then wait before the next.
2. **Readiness gate over fixed sleep**: prefer waiting until the just-spawned worker's `claudeShellPid` appears in `active-sessions.json` (bounded timeout), falling back to a fixed ~15–20s inter-spawn delay.
3. **Concurrency cap + configurable delay**: expose an inter-spawn delay and a max-concurrent-spawn knob (defaults conservative on Windows).
4. **Optional pre-spawn resource check**: skip/blocking-warn if free memory is below a threshold before launching another heavy session.

## Open Questions

- Readiness signal: is `claudeShellPid`-in-registry reliably written early enough to gate on, or is a fixed delay more robust? (CLAUDE-B took ~19s to write its pid; CLAUDE-C never did.)
- Right default inter-spawn delay for this machine (15s? 20s? adaptive?).
- Does the `.sh`/tmux path on mac/Linux need the same throttle, or is `wt.exe` the specific weak point?
- Should `/0-uldf-pods-parallelize` warn when a planned worker count (e.g. ≥4) exceeds a safe-on-this-machine threshold?
