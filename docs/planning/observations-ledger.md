# Observations Ledger

One line per finding that did **not** clear the `/0-uldf-inject` Phase 2.5 filing gate (user
directive 2026-08-06): internally-discovered with no observed harm, unmeasured inference, or a
verification-machinery coverage gap with no witnessed wrong verdict. Append-only; newest last.
A line is promoted to a DEFER brief by whoever actually **observes** the harm it predicts —
cite the line when promoting. This file is deliberately not read at session start and has no
oracle; it is a grep surface, not a work queue.

Format: `YYYY-MM-DD | one-sentence finding | where seen | filed-by`

---
2026-08-06 | ULDF's DISC-SPAWN-02 burst guard is enforced in code only along the path an LD *chooses* (`spawn-pods-all.ps1`); nothing stops an LD from firing N direct `spawn-claude-session.ps1` calls instead, and the only defense on that path is the segment's prose "`--all` MUST go through the orchestrator" — no PreToolUse hook covers spawn *rate* (`pre-tool-use-ctd-spawn-gate` covers designation only). The 2026-06-18 feedbackmonk crash IS consistent with such a bypass (spawns 6s apart while a worker's pid took 19s — timings the orchestrator's verify-between gate could not produce), but attribution is NOT established: `~/.claude/` carries no sync-provenance record, so whether the guard had even reached that machine 9 days after it landed in `claude-template/` is undeterminable | measured while draining feedbackmonk DEFER-001; framework commit `432cc833` (2026-06-09) vs incident 2026-06-18, registry `.claude/collaboration/archived/collab-20260618-180600/` | defer-drain-FeedbackMonk-20260806 (ledger not brief under the 2026-08-06 filing gate: the harm is real but is 7 weeks old and **cannot be attributed** between bypass and sync-lag — filing a brief into ULDF on an unverified attribution would be exactly the unmeasured inference the gate exists to stop. Promote when an LD is witnessed bursting direct spawns with the orchestrator demonstrably present)
