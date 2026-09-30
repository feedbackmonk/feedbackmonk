# `.claude/project-oracles/` — this project's Verification Oracles

## Summary

These are feedbackmonk's static checks of code-level invariants that tests cannot see
because the invariant is the absence of something. A router merged without its guard is
one example; raw SQL outside the repository crate, a board query that loses its `approved`
filter, and a log subscriber installed around the scrubber are others. There are fifteen:
twelve `fast`, one `slow` and two `operator` (report-only). Come here when a change touches
wiring, SQL, erasure, the runner's prompt or egress, logging, the widget build or the
self-host settings, or when an oracle fails.

## Index

One directory per oracle. `CLAUDE.md` § Oracles says what each one defends. Each
directory's `oracle.json` and `README.md` record its probes and the mutations it was
proven against.

| Directory | Lane | Defends |
|---|---|---|
| `approval-gate-enforcement/` | fast | no work order reaches an execution state without an owner-authored approval; one writer of `work_orders.state` |
| `feedback-as-data-audit/` | fast | runner: recommendation text only inside the untrusted envelope; every outbound payload through the egress sanitizer |
| `feedback-erasure-completeness/` | fast | erasure purges bytes before rows, scopes each DELETE, reaches every end-user-keyed table, and every FK to feedback cascades |
| `host-tenant-binding/` | fast | every router in `build_app` classified: public = host-bound + rate-limited (+ CORS exactly where required), admin = admin-bound; one host→tenant path |
| `i18n-catalog-integrity/` | fast | catalog shape and the generated locale tables (C35, C41) |
| `i18n-literal-ratchet/` | fast | 0 hard-coded user-facing literals in `widget/src` and `admin-ui/src` (`.ts` and `.tsx`) |
| `multi-tenant-isolation-check/` | fast | the tenant-scoped repository layer is the sole query path (DEC-FBR-03) |
| `pii-scrub-audit/` | fast | no crate but `feedbackmonk-tracing` builds or installs a log subscriber |
| `public-board-moderation-gate/` | fast | every public board query carries the `approved` filter; the board handler calls only board-safe reads |
| `tier-enforcement-status/` | fast | every path that inserts a chargeable row checks the tier quota first |
| `translation-egress-q24-isolation/` | fast | translation provider off by default; `body_translated` never reaches a public router |
| `widget-bundle-size/` | fast | committed widget build present, page-load set ≤ 30,720 B, locale chunks ≤ 4,096 B, no trackers |
| `selfhost-compose-smoke/` | slow | compose validates (Docker); every setting compose or the api reads is in `SELFHOST_ENV.md` |
| `feedback-parity-status/` | operator | GitCellar's cutover gate: the four customer-#1 parity gaps, read from code |
| `translation-gap-status/` | operator | advisory: is a `/1-translate` pass due (kind `project-state`) |

## How they run

- **The finalize proof** runs them by lane (ULDF VER-15). `.claude/config.json` sets
  `verification.projectOracles: gate`, so a red oracle fails the proof. `fast` runs at every
  check, `slow` runs only when the suite is whole, and `operator` never runs. Each oracle's
  `oracle.json` `invocation` names its `oracle.py` directly. Exit 0 passes, 3 is `unknown`,
  and anything else fails.
- **CI** runs `bash scripts/run-verification-oracles.sh` in job `verification-oracles`, and
  so does `scripts/ci-local.sh` step 1. That script lists the thirteen fast and slow oracles
  by name and discovers nothing on its own, so an oracle added here must be added to its
  `ORACLES` array.
- **By hand:** `python .claude/project-oracles/<name>/oracle.py [--full] [--root <copy>]`.
  `--full` adds the behavioural legs, which re-run `cargo test` targets and need a database;
  CI's test job already covers those. `--root` points an oracle at a scratch copy of the
  tree, which is how the mutation self-tests run.

## Constraints

- **Never narrow a scan, weaken a threshold or add an allowlist entry to turn a line
  green.** `multi-tenant-isolation-check/allowlist.toml` requires a `rationale` on every
  entry, and fails on an entry whose function no longer exists.
- **A missing target is a FAIL, not a vacuous PASS.** The cold-start branches were removed
  on 2026-09-30, because the code they waited for has existed since v1.
- Each `oracle.py` resolves the repo root by depth, so moving a directory to another
  nesting level breaks it. Pass `--root` rather than moving one.
- A retired oracle is removed here and named, with its reason, in
  `verification.retired_oracles` in `.claude/config.json`.

## Relationships

- **Consumed by** the ULDF finalize proof (by lane), `scripts/run-verification-oracles.sh`
  (CI job `verification-oracles`), and `scripts/ci-local.sh`.
- **Reads** `crates/`, `migrations/`, `deploy/docker/`, `docs/operations/SELFHOST_ENV.md`,
  `widget/dist/`, `i18n/` and `admin-ui/src/`.
- **Sibling, not parent:** `.claude/oracles/` is the framework starter pack's home, under a
  different contract and runner (see its `INDEX.md`).

## Decisions

**The 2026-09-30 review** (`docs/planning/project-checks-review-2026-09.md`, ULDF DEC-710)
ran and mutation-tested every oracle.
- **What it found:** most passed even with the regression they named present. The causes
  were stale hand-kept lists, word-presence checks that a comment satisfied, a parser that
  read the wrong block, and vacuous passes. Two of the misses hid live defects:
  - four public routers had no rate limit;
  - a runner failure reason reached the wire with only PII scrubbing.
- **What changed:**
  - Seven oracles were rewritten, and every one is now proven to fail on its regression.
  - Five were retired into tests or into `host-tenant-binding`.
  - The shims (`oracle.sh`/`oracle.ps1`) and the `manifest.toml` mirrors were deleted.
  - `manifest.json` became `oracle.json`, and each now declares a `lane`.

**The two contracts get two directories.** These oracles once lived under `.claude/oracles/`,
alongside the framework pack. That runner cannot read their manifests, so it reported
`unknown` for each one. A 2026-09-07 migration deleted them as stale, which turned CI red.
Separate homes end the collision (DEFER-010). Three oracles that had never been on the
runner's list were restored separately on 2026-09-11.
