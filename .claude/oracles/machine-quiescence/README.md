# machine-quiescence Oracle

## Synopsis

Verification Oracle that answers **"is this machine clean enough that a number taken now means anything?"** — with the residue *named* (which port, which PID, which process) and a verdict a measurement lane can **refuse** on. Come here for the verdict/exit-code contract, the ground-truth inversion, and the fixture seam. Don't come here for *project*-scoped worktree-fit signals — that's the sibling `project-runtime-state` oracle, which shares this oracle's Dev Port Registry parser but answers a different question.

## Identity

**Question answered**: Is this machine quiescent enough to measure on, and if not, exactly what would contaminate the measurement?

**Category**: environment
**Kind**: `verification` (`FOUNDATIONS/ORACULURGY_DESIGN.md` Part 11 — the deterministic-inner-loop mechanism of Probandurgy)
**Spec**: `docs/specs/SPECIFICATION.md` § QUIESCE (QUIESCE-01..09)
**Decision**: DEC-208; DEC-297 (the validity declaration, the presence-vs-utilisation bound, min-of-N)
**Trigger**: DEFER-043 (inject ← Table, 2026-07-29); DEFER-133 (the proxy-referent finding, 2026-08-06)

## Why it exists

Three silent wrong answers in one Table session, all the same family — *something left running quietly changed what a measurement meant, and nothing in the framework noticed*:

1. An **orphaned preview server** made a verification run grade new code against an old bundle. Playwright's `reuseExistingServer` over a `build && preview` command means an already-listening server makes the suite **skip the build entirely**. The run passed green against the previous commit. It surfaced only because the results looked too good.
2. **Four idle-but-alive PODS worker sessions** moved a performance comparison by ~20–25% on the *incumbent* renderer — the control that no code change could touch.
3. **Stale registry PIDs reported false liveness** — `dispatchable-sessions` returned zero live siblings while worker shells were plainly running.

A fourth episode is why this is a **gate** and not a rule: an LD had to hand-write a measurement-window guard for a worker (*verify nothing is listening on that port; verify from the runner's own output that the build actually ran; stop rather than publish numbers you cannot vouch for*). Without it, a re-measurement after a performance fix would have timed the **pre-fix** bundle and reported the fix as inert — into a published comparator file. It worked because one agent thought of it. That is not a mechanism.

**The design principle: convert a silent wrong answer into a loud refusal.**

## Three invariants — read these before changing anything

### 1. Ground truth is the OS; the registries only *name* what is found

Detection is **listening sockets and live processes**. `~/.claude/MACHINE_CONFIG.md` and `active-sessions.json` are consulted only to *attribute* residue to a project. That inversion is deliberate: incident #3 above was a registry confidently reporting liveness that did not exist. A registry may name residue; it must never be the thing that detects it. **Residue that cannot be attributed is reported anonymously — never dropped.**

### 2. The detection set is the reserved *bands*, not the claimed ports

The port in the motivating incident (a Playwright preview) was never registered. A gate scoped to claimed ports would have been blind to exactly the residue it exists to catch. The set is: `--port` requests ∪ registry assignments ∪ reserved ranges ∪ common dev defaults.

### 3. It never actuates

This oracle reports. It does not close, kill, or unbind anything — asserted by a cell in both self-tests and in the smoke. **Warm worker sessions stay warm by design**: in the originating incident two real defects were fixed by dispatching them back to their original authors, who still held full context. The defect is the missing *signal*, not the liveness.

## Verdict contract

| Verdict | Exit | Meaning |
|---|---|---|
| `quiet` | 0 | No residue in the detection set. |
| `noisy` | 1 | Residue present, none of it definitively invalidates *this* run. Survivable for a pass/fail run; not for a timing measurement. |
| `unknown` | 2 | The probe was degraded. **Fails closed** — treat as a refusal. A probe that cannot see residue must never report quiet. |
| `blocking` | 3 | Residue that definitively invalidates *this* run. |

Two verdicts rather than one is what keeps the gate credible. A developer machine is almost never `quiet`, so a single boolean would be permanently red and quickly ignored. `blocking` is reserved for the deterministic cases — **a port this run declared it needs is held** (episodes 1 and 4: unambiguous, no judgment required), or **a `--timing` run on a machine with any residue at all** (episode 2: only a timing measurement cares). Everything else is `noisy` with the residue named, and the caller judges.

## What the verdict does NOT mean — presence is not utilisation (DEFER-133, DEC-297)

**This oracle counts processes that exist. It does not measure how hard any of them is working**, and the gap between those two propositions is several-fold. Measured 2026-08-06: the same script, on the same repo, returned `noisy` twice with comparable headline counts (22–28 vs 18 agent sessions; 6–9 vs 5 build processes) and wall clocks **~5× apart** (4.62–7.90s vs 1.26–1.43s) — with the *larger* corpus on the *faster* side. The first machine's agent sessions were running smoke suites; the second's were idle at their prompts. **Nothing in this output distinguishes those two states.**

So: **a `noisy` verdict is a floor on suspicion, never a magnitude of contention.** The concrete cost of reading it as a magnitude is on record — DEFER-125 was filed, carried across a wave, and consumed a lane's triage on the strength of a runtime figure whose real cause was transient load the verdict could not express. That brief did everything the doctrine asks: it refused to publish, named the residue, said so. The doctrine still let it record a wrong figure as a durable fact.

**The remedy is not in this oracle — it is min-of-N.** Load can only *inflate* wall clock, so the minimum of N observed runs upper-bounds the clean-machine time, which makes "fast enough" provable on any machine without a `quiet` verdict and without clearing anyone's residue. The recipe, its one-directional guardrail, and its warm-cache bound live in `segments/_measurement-quiescence-guard.md` § "Taking a number on a machine that is never quiet" — a single owner; do not re-derive it here.

### Why no utilisation field was added — declined with a measurement (DEC-297)

Weighed and **declined**, and the reason is stability rather than cost. Five candidate signals were measured on this machine (Windows PowerShell 5.1, min-of-5, so the figures upper-bound the clean cost):

| Candidate | Cost (min) | Verdict |
|---|---|---|
| `Win32_PerfFormattedData_PerfOS_Processor` (machine CPU%) | 270 ms | affordable, **unstable** |
| `Win32_PerfFormattedData_PerfProc_Process` (count ≥5%) | 557 ms | affordable, **unstable** (47% rel. stddev) |
| `Get-Counter \Processor(_Total)\% Processor Time` | 1006 ms | affordable, **unstable** |
| two-sample `Win32_Process` CPU-time delta, 1 s window | 1461 ms | affordable, **unstable** (28% rel. stddev) |
| `Get-Counter \System\Processor Queue Length` | 1018 ms | affordable, **unstable** |

Every one fits the 12000 ms budget beside the ~2.9–4.4 s the oracle already pays, so **cost was never the blocker**. Stability was: the *expensive reference* signal varies against **itself** by **46.3 pp range / 14.9 pp stddev across ten consecutive reads**, which is larger than the 13.0 pp mean gap between the cheap proxy and that reference. The cheap probe is not the problem — **the quantity is not stable at the timescale an oracle call can afford.**

Both readings of that instability point the same way, which is what makes the decline robust: if the variance is sampling noise, the emitted number is wrong; if it is genuine second-to-second volatility, the number is stale before the measurement it qualifies has begun. Either way a single pre-measurement sample cannot carry the claim, and emitting it would install **a second undeclared proxy inside the fix for the first** — the precise defect DEFER-133 exists to close, one level down.

**Named re-arm condition**: a load figure must be integrated **over** the measured window by whatever takes the number, not sampled **before** it by this oracle. That is a different artifact — a measurement wrapper — and it belongs to whoever builds one, not here.

## Usage

```bash
# Bare reading
.claude/oracles/machine-quiescence/run.sh

# "I am about to run a suite that binds 14239" — a listener there is BLOCKING
.claude/oracles/machine-quiescence/run.sh --port 14239

# "I am about to take a timing number" — any residue is BLOCKING
.claude/oracles/machine-quiescence/run.sh --timing --port 14239
```

```powershell
.claude\oracles\machine-quiescence\run.ps1 -Port 14239 -Timing
```

Consumed via `segments/_measurement-quiescence-guard.md` — see that segment for the refusal procedure, not this README.

## Files

| File | Purpose |
|---|---|
| `oracle.json` | Manifest: frozen output schema, exit-code contract, cost provenance |
| `run.sh` | Bash entry point — POSIX probe inline; on Git Bash delegates the probe to `probe.ps1` (one shell-out, never a per-port loop) |
| `run.ps1` | PowerShell entry point |
| `probe.ps1` | **Raw-fact probe only** — emits TSV facts, performs no classification and reaches no verdict. Shared by both twins so there is one OS probe and two classifiers |
| `validate.sh` / `validate.ps1` | Sandbox self-tests (runs, conforms, fails closed, never actuates) |

Behavioural coverage lives in `scripts/smoke-tests/machine-quiescence-smoke.sh` (30 cells across four PowerShell engines), not in the self-tests.

## Test seam

`ULDF_QUIESCE_FIXTURE=<file>` supplies raw facts instead of probing the OS, so the classification and verdict logic is exercisable — and *falsifiable* — without needing a real noisy machine. Both twins honour it, and TWIN-01 parity is asserted over it. `ULDF_MACHINE_CONFIG_FILE=<file>` does the same for the registry.

## Cost

~4.9s on Windows (one `Get-NetTCPConnection` sweep + one `Win32_Process` CIM enumeration over ~690 processes). `lane: slow`, **never** a session-start oracle. That is a deliberate Oraculurgy call: a briefing line every session is a warning nobody reads, and the question only has an answer worth paying for at the moment a number is about to be taken.

Re-measured 2026-08-06 under `noisy` (22 agent sessions, 5 build processes): **2.89–4.38s** over 3 runs, against the declared 12000 ms. Published under min-of-N — the observed minimum upper-bounds the clean time, so the declaration is confirmed conservative without needing a quiet machine. Note the recursion and take it seriously: **this figure is itself a wall-clock number taken on a non-quiet machine**, which is exactly why it is stated as a bound rather than as the cost.

## Decision Log

- **Why a new oracle rather than composition over `project-runtime-state`** — quiescence is a *machine*-level question and every existing ingredient is project-scoped (`dispatchable-sessions` reads this project's registry; `pid-orphan-detector` sweeps this project's PID files; `project-runtime-state` scopes registry rows to the current directory). The scope genuinely did not exist. What *was* composable — the Dev Port Registry parse — was extracted to `scripts/lib/dev-port-registry.{sh,ps1}` and fixed there rather than reimplemented here.
- **Why the parser moved out and got fixed first** — the pre-DEC-208 parse was inlined in both `project-runtime-state` twins with a regex that matched **zero** real registry rows (markdown emphasis + code spans), so `hasLiveDevServer` was structurally incapable of returning true from 2026-05-10 to 2026-07-29. Composing a quiescence gate over that would have inherited a false negative on exactly the residue it must catch.
- **Why build-process matching is anchored to command *words*** — a bare substring scan reported 75 "build processes" on the authoring machine, of which ~70 were the Playwright MCP server, its Chrome helpers, and (self-referentially) the `grep` inspecting them. Residue by path-spelling is not residue. Anchoring cut it to 21, all genuine.
- **Why no session-start briefing line** — considered and rejected at the design gate: it costs a socket+process sweep every session to surface a fact that only matters at a measurement seam, and it is the shape of warning that gets skimmed. The guard segment fires where the decision is.
