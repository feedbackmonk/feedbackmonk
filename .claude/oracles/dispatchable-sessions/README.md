# dispatchable-sessions — implementation notes and documented bounds

The manifest (`oracle.json`) is the schema authority; this README exists because two
non-obvious properties of this oracle earned an incident each (DEFER-064, diagnosed
2026-08-03) and the next editor should inherit the bounds, not rediscover them.

## The `--gc-cheap` budget: what it bounds, and the starvation class

The ~1000ms cheap budget bounds **the per-entry liveness probe loop only** — not setup.
Three invariants, all load-bearing (validate cells T8/T9/T10 pin them):

1. **The clock starts after the candidate parse.** On MSYS, forks are 300–500ms+ under
   load; with the clock started before the parser fork, setup alone exceeded the whole
   budget and the sweep deferred before the first probe — silently, every session start,
   while `--gc` (unbudgeted) stayed green. This is the DISC-ORA-05 starvation class; that
   entry's "latent sibling risk" paragraph named this very file and the prediction fired.
2. **The clock is fork-free where the shell allows** (`$EPOCHREALTIME` in bash ≥ 4.4,
   `Stopwatch` in PowerShell). The old bash clock was a `date` fork — ~372ms per check on
   the incident machine, i.e. the measurement was consuming the budget it measured.
3. **Minimum-progress guarantee**: the first candidate is always probed; the budget is
   consulted only after at least one probe has run. A slow environment therefore defers
   the *tail*, never the whole sweep, and repeated session starts drain any backlog
   (convergence — T10). Candidates collected before a mid-loop budget break are still
   swept.

Test seam: `ULDF_DS_GC_BUDGET_MS` (numeric, ms; non-numeric falls back to 1000). Used by
T8–T10 to make budget behavior deterministic; not a tuning knob for production.

## What the self-test's green can and cannot vouch for

The liveness cells (D2/D5, S1–S4) embed a **spawned sleeper PID** in their fixtures. That
fixture is an assumption with a lifetime: in the DEFER-064 incident the bash validator's
run (~4.5 min on a fork-slow machine — every probe/ancestor walk forks `powershell.exe`)
outlived its 120s sleeper, and D5/S1–S3 asserted "alive" about a genuinely dead PID. The
oracle answered correctly; **the harness lied about its own fixture**, pseudo-
deterministically (fixed fork count → stable elapsed time → byte-identical red output,
which mimicked a code defect).

Defenses now in place: 1800s sleeper lifetime, and `ensure_sleeper` / `Ensure-Sleeper`
re-verifies liveness at every fixture write that embeds the PID, re-spawning on expiry
and saying `[harness] ... not an oracle verdict` when it does. The bound to keep in mind:
a green liveness cell vouches for the oracle's classification *of a PID the harness
re-verified at write time* — if you add a cell that embeds the sleeper PID, call the
guard before the write, or the class returns.

Before the guard existed, this red class was **invisible to the oracle's own green
self-test on faster machines** (the ps1 twin finished in 67s and stayed 22/22 while the
bash twin was red at HEAD) — the OVALID shape: a self-test written in the fixture's
vocabulary cannot see the fixture's assumption breaking. The gap is now *declared*
(`assertion.known_gaps` in the manifest) and *guarded*, not assumed away.

## Falsifiability

Seeded-defect evidence for the three claims above (each demonstrated RED under the
seeded defect, green at the fix): `docs/falsifiability/2026-08-03-defer064-dispatchable-sessions-selftest.md`
in the ULDF repo. Diagnosis of record: DISC-ORA-07 in `docs/specs/DISCOVERIES.md`.
