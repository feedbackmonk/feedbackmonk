---
status: open
commissioned_by: ULDF, session 978b7656-47d2-46b0-992a-e424f930d42d (the testing-runtime rollout, TLV-17 / DEC-745, released by SessionHelm's closed adoption brief)
origin: the owner's framework-wide testing standard is deployed and proved in SessionHelm; every Playwright project adopts it
harm: witnessed
beneficiary: product
deferred_because: another-project
---

**This project's lanes**: Three lanes: `admin-ui/`, `marketing/` and `widget/`. Each has its own config, so each vendors its own `_liveness/` copy beside that config.

Adopt the ULDF testing-runtime standard (DEC-745, TLV-1..17) in this project's Playwright lanes.
Tests settle and then check once. A stall detector that counts samples, not seconds, ends a wait
nothing can satisfy. Browser runs take a slot in one machine-wide queue, and no test run owns the
project's dev server.

**The procedure**: `~/.claude/templates/testing/README.md` § Adopting it in a Playwright project
(copy `playwright/*.ts` to `tests/_liveness/` unchanged, then rework config, imports and wiring).
**Done** means `python ~/.claude/oracles/_runner/run.py --only test-liveness` reads clean, plus
two proofs observed on real runs:
(1) a deliberate wait on a never-rendered element ends with `STALL (testing-runtime TLV-4)`
naming the locator, then delete that probe;
(2) one spec whose app work is slow but real (an IPC/fetch/backend reply) stays green, because
it is wired into the counter.
Run only the specs your change reaches, never the whole suite (the backstop runs it).

**What SessionHelm learned as the first adopter** (its commits 9786ce3 and e4fd7d9):
- Wire the IPC/backend counter **once**, as one init script in the project's shared fixture,
  not in each test's stub. For Tauri, wrap `window.__TAURI_INTERNALS__` through accessors so
  `=`, `??=`, a spread replacing `invoke` and a later `.invoke =` are all caught, and report
  each call through `window.__uldfQuiet.track`.
- Changing the spec imports is one regex over `import {...} from "@playwright/test"` when it
  names `test` (this covers multi-line clauses). Re-export the type names your specs use
  (`Page`, `Locator`, ...) from the project helper, so only the specifier changes.
- An existing `globalSetup` composes as an array: `[queue-setup, <yours>]` (Playwright >= 1.59).
  On an older Playwright, call `enterQueue(config)` first thing in yours.
- `webServer` goes and `metadata.testServer` replaces it, on the project's reserved port from
  `~/.claude/MACHINE_CONFIG.md` (Dev Port Registry). The server it starts is detached and
  outlives the run and the worktree. If you remove a worktree that started one, kill that server
  first, because nothing reaps it.
- A first cold transform can take about a minute. The queue does not hide cold-start cost, and
  the detector skips starved samples instead of calling them a stall. Don't add a timeout back.
- Keep every Playwright timeout at `0` (DEC-743). A timeout that only stops a test is removed.

Report a defect in the template itself (`~/.claude/templates/testing/`) to ULDF as a brief at
`S:/ULDF/docs/planning/deferred/`. Never edit the vendored copy to work around it. Close this
brief by deleting it in the commit that lands the adoption, and put the proofs in that commit's
message.
