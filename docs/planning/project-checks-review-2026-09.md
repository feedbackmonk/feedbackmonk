# Project checks review — 2026-09-30

This review applies ULDF DEC-710: every project reviews its own quality checks against
the rebuilt ULDF. It covers all 20 oracles that were under `.claude/project-oracles/` on
2026-09-30. Each oracle was run, read against the product code it guards, and
mutation-tested: the regression it names was planted in a scratch copy of the tree to see
whether it fails.

## What the review found

**All 20 passed, and most of those passes proved little.** Mutation testing broke most of
them:

| Mutations | The old oracles caught | The kept oracles catch |
|---|---|---|
| 112 planted across seven oracles | 5 | all 112 |
| Separate runs on `pii-scrub-audit`, `feedback-as-data-audit` and `i18n-literal-ratchet` | — | every one |

The failures came from four causes:

- **Stale hand-kept lists.** The rate-limit oracle checked four named routers, and eight
  public routers exist.
- **Word-presence checks that a comment satisfies.** `approval-gate-enforcement` passed on
  the function's name appearing in a doc comment.
- **A parser that read the wrong block.** `tier-enforcement-status` could never fail.
- **Vacuous "cold-start" passes.** They waited for code that shipped in v1.

**Product defects found**:

| Defect | Found by | State |
|---|---|---|
| `widget_config` (unauthenticated, three DB queries per hit), `me_feedback`, `me_feedback_data` (including the export) and `solicitation` had **no per-IP rate limit**, although `main.rs` and commit `2af28ec` said every public router did | review of `public-route-ceiling`'s hand-kept list | **Fixed** on the owner's word: all four are wrapped in `apply_public_rate_limit`, which shares the existing per-IP budget. `host-tenant-binding` now fails any public router without it |
| **Opt-out race** (privacy, DEC-FBR-IMPL-24): the handler reads the state, checks it in memory, then writes without a condition, so a concurrent `prompted` could overwrite `opted_out` | review of `solicitation-invariant-check` | **Fixed** on the owner's word: the upsert refuses to overwrite `opted_out` in SQL, and test `stale_write_cannot_overwrite_opted_out` covers it |
| A runner **failure reason reached the wire with only PII scrubbing**, not the egress sanitizer. Agent-controlled text got there through serde's error message | review of `feedback-as-data-audit` | **Fixed** on the owner's word: `failure_reason_for_egress` in `report.rs`, with a test. The redone oracle fails on the old code |
| Recommendation title and body (model-derived from feedback) enter the runner prompt's **trusted** layer | same | **Owner's decision**: `docs/planning/deferred/runner-recommendation-text-in-trusted-layer-20260930.md` |
| Five settings the api reads were missing from `SELFHOST_ENV.md`: the solicitation snooze and four GlitchTip variables | review of `selfhost-compose-smoke` | **Fixed**: catalog rows added, and new Probe D fails on any gap |
| **Crash correlation is built but never wired.** Nothing constructs `GlitchtipCorrelator`, so setting the four GlitchTip variables does nothing, contrary to the adoption contract §5.6 | same | **Owner's decision**: `docs/planning/deferred/crash-correlation-not-wired-20260930.md`. The contract carries a correction note |
| The committed `widget/dist` was never checked against `widget/src`, and **CI never built or tested the widget or admin-ui** | review of `widget-bundle-size` | **Fixed**: CI job `frontends` runs the widget's tests, rebuilds it and fails when the committed `dist/` differs; the build is deterministic. It also runs admin-ui's tests and build |
| admin-ui tests failed under load on vitest's 5 s default timeout (the userEvent form tests) | running admin-ui's suite, which nothing did | **Fixed**: `testTimeout: 20_000`. No assertion changed |

## Decisions, one row per oracle

"Last run" is the default (static) invocation on 2026-09-30, after the changes. Times are
the minimum of three runs on the dev workstation while other cargo builds were running.
Python start-up alone was 0.67 s.

| Oracle | Decision | Reason | Last run |
|---|---|---|---|
| `multi-tenant-isolation-check` | **update** | The right keystone check (DEC-FBR-03). Probe A missed `raw_sql`, bare `query(` and string-SQL `execute`/`fetch`. Probe B skipped generic functions. Widened, and the allowlist is now parsed as TOML with its names checked against real functions (16 mutations; the old version caught 1) | PASS, 1.4 s, fast |
| `host-tenant-binding` | **redo + consolidate** | Its hand-kept router lists missed `tenant_settings_router` and passed twice-merged routers. It now classifies every `.merge()` in `build_app`: public routers must be host-bound, rate-limited, and carry CORS exactly where required; admin routers admin-bound; unbound only health and capabilities. Absorbs `public-route-ceiling` and cors Probe A (28 mutations) | PASS, 1.9 s, fast |
| `public-route-ceiling` | **retire (consolidated)** | Superseded by `host-tenant-binding`. Its list of four hid four unlimited public routers | — (retired) |
| `cors-allowlist-enforcement` | **retire (consolidated + test)** | Probe A passed on any `.layer(`, not just CORS; the wiring check now lives in `host-tenant-binding`. Probe B duplicated `tests/cors_preflight.rs` | — (retired) |
| `pii-scrub-audit` | **update** | Probe A missed `fmt::init()`, `FmtSubscriber` and `set_global_default`, so it was widened. Probe B (the pattern hash) moved to the tracing crate's test, which used to pass silently when the hash file was missing and now fails | PASS, 1.3 s, fast |
| `public-id-as-capability` | **retire (test covers)** | It passed when the authorization result was ignored. `attachment_list_download.rs` (`read_without_submitter_credential_404`, `cross_scope_attachment_ids_404`) catches that | — (retired) |
| `submission-idempotency` | **retire (test covers)** | It only checked that words were present. `submit_idempotency.rs` asserts identity scoping and the 409 on reuse | — (retired) |
| `approval-gate-enforcement` | **update** | Probe B was satisfied by a doc comment. It now checks inside `transition_work_order` and enforces a single writer of `work_orders.state`. Probe A dropped, since it duplicated core unit tests (14 mutations) | PASS, 1.6 s, fast |
| `public-board-moderation-gate` | **update** | Only one SQL string per function had to carry the `approved` filter, so the unfiltered items query in `list_public_board` passed. Now every string is checked, and the board handler may call only allowlisted reads (9 mutations; the old version caught 1) | PASS, 0.5 s, fast |
| `feedback-erasure-completeness` | **update** | Probe C accepted the scoping tokens anywhere in the function. It is now scoped to the DELETE itself, and it adds the bulk-erase order and ownership check plus "every end-user-keyed table is erased" (22 mutations) | PASS, 1.1 s, fast |
| `solicitation-invariant-check` | **move** | Its static probes duplicated core unit tests and could not see the read-then-write race. The guard is now SQL plus a DB test | — (moved into `solicitations.rs`) |
| `feedback-as-data-audit` | **redo** | Egress was checked per file, so the failure-reason path passed because another function in the same file sanitized. Now checked per function and per argument; the envelope is checked against the trusted layer and HTTP clients are confined to `client.rs` (3 mutations, including the pre-fix `report.rs`) | PASS, 1.6 s, fast |
| `translation-egress-q24-isolation` | **update** | The `"off" => Ok(None)` check could never fail. Fixed, and added Probe E: no public router file names `body_translated` (10 mutations; the old version caught 3) | PASS, 1.1 s, fast |
| `tier-enforcement-status` | **redo** | Probe A's parser read the wrong block, so it never inspected a handler, and its allowlist named 5 functions that no longer exist. It now finds the repository inserts into `projects`/`feedback` and requires each caller to run `check_tier_quota` first. Probe B dropped as a duplicate of core tests (13 mutations). Guards revenue | PASS, 1.5 s, fast |
| `widget-bundle-size` | **update** | Right check. A missing `dist/` or missing locale chunks printed a vacuous PASS and now fail. Freshness against `src/` is CI job `frontends`, and the build writes `dist/README.md` | PASS, 0.5 s, fast |
| `selfhost-compose-smoke` | **update** | Added Probe D (code-read settings vs the catalog), which exposed the five-variable gap. No validator now yields exit 3 `unknown` instead of a silent PASS. Docker makes it the slowest, so it is `slow` | PASS, 5.7 s, slow (Docker start-up varied 3–90 s during the day; a Docker timeout is now `unknown`, exit 3, not a FAIL) |
| `i18n-catalog-integrity` | **keep** | Deterministic, shares its implementation with the validator, and is the only CI enforcement of catalog shape | PASS, 1.7 s, fast |
| `i18n-literal-ratchet` | **keep (widened)** | Sound. Its scan now also covers admin-ui `.ts` modules, which were unscanned. A planted `textContent = "Save changes"` in `shared/format.ts` fails it | PASS, 1.1 s, fast |
| `translation-gap-status` | **keep** | An advisory release-gate report (`kind: project-state`), correctly outside the gate | reports DUE (by design, DEC-FBR-17), 0.7 s, operator |
| `feedback-parity-status` | **keep until cutover, then retire** | GitCellar's cutover status report. Its detectors check that files exist, and gap #2 reads CLOSED although the correlator is unwired (see the brief). Retire once the GitCellar cutover is recorded done | CUTOVER GATE: OPEN, 0.9 s, operator |

**Counts:** keep 4 (`i18n-catalog-integrity`, `i18n-literal-ratchet`, `translation-gap-status`,
`feedback-parity-status`) · update 8 · redo 3 (`host-tenant-binding`, `feedback-as-data-audit`,
`tier-enforcement-status`) · consolidate 2 (`public-route-ceiling` and `cors-allowlist-enforcement`
into `host-tenant-binding`) · move 1 (`solicitation-invariant-check`) · retire 2
(`public-id-as-capability`, `submission-idempotency`, both covered by tests). Seven are
retired or moved in all counts, and five directories were removed. Every retirement of a
security, PII or data-loss guard was put to the owner first, and in each case the guard now
lives in a test or in another oracle.

## Housekeeping

- `manifest.json` became `oracle.json`, since ULDF's finalize proof reads `oracle.json`. Each
  now declares `lane` and a measured `expected_runtime_ms`, and `invocation` names `oracle.py`
  directly.
- The 17 `oracle.sh`/`oracle.ps1` shims were deleted. They only delegated to `oracle.py`,
  and CI never used them.
- The 5 `manifest.toml` files were deleted. They were self-declared mirrors of the JSON.
- Retired names and their reasons are in `.claude/config.json`
  `verification.retired_oracles`.

## The mode: `gate`

`.claude/config.json` sets `verification.projectOracles: gate`.

- **All green.** Every kept `fast` and `slow` oracle passes on the tree, through the ULDF
  finalize function itself:
  `note project-oracles (gate): 13 ran in 27.4 s, 1 not run; all green`, with the whole
  suite (fast and slow).
- **Fast enough.** The `fast` lane is 12 oracles, and the finalize proof runs them one after
  another in about 18–20 s. That was measured on this workstation while other cargo builds
  were running, and each oracle is 0.5–1.9 s, of which Python start-up is about 0.4–0.7 s.
  Each is offline and deterministic, with no database, no network and no Docker. Twenty
  seconds is small beside the cargo test suite the same proof runs.
- **Honest.** Every kept oracle has a recorded mutation it fails on, so a green result means
  something.
- The one oracle that needs Docker is `slow`, and it runs only on whole-suite checks.

**What would move this back to `report`:** an oracle that goes red for a reason unrelated
to the change being verified, meaning flakiness. That would be a defect in the oracle, to
fix rather than silence.
