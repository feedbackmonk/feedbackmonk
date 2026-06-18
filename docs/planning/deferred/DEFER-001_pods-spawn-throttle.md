# DEFER-001: Throttle PODS worker spawning to prevent burst-spawn machine crash

**Status**: PROPOSED
**Deferred**: 2026-06-18
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
