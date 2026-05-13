# dispatchable-sessions/test-fixtures — Oracle-Local Expected-State Fixtures

## Synopsis

Pattern A paired-oracle fixtures (Oraculurgy Part 11) for `dispatchable-sessions`: documented expected-state scenarios for the `active-sessions.json` registry that the live oracle is asserted against by the `csi-01-fixture-drift.py` verifier. Come here when adding a CSI-01 registration scenario, debugging a smoke-harness failure in `claude-template/scripts/csi-tests/csi-01-smoke.{sh,ps1}`, or looking up the `<MATCH:regex>` predicate syntax for variant fields. Don't come here for cross-oracle generated-artifact fixtures — those live under `claude-template/templates/fixtures/`.

## Purpose & Responsibilities

Expected-state fixtures specific to the `dispatchable-sessions` oracle
(`claude-template/oracles/dispatchable-sessions/`). Each fixture documents
what the oracle (or the substrate it reads — the dispatch registry at
`.claude/collaboration/active-sessions.json`) should look like in a labeled
scenario. Paired with a drift-detection verifier so the fixture is a
load-bearing contract rather than an inert document.

Scope: fixtures of registry shape and oracle output shape. Tests that drive
the oracle end-to-end live in
`claude-template/oracles/dispatchable-sessions/validate.{sh,ps1}`.

## File Index

| File | One-line purpose |
|---|---|
| `csi-01-registration-fixture.json` | Three scenarios for the CSI-01 self-registration mechanism: interactive-session-fresh (no env, no pre-existing entry), spawn-script-then-hook-confirm (env-supplied identity merged with pre-existing entry per DEC-24), re-invocation-stable-identity (this-session.json present; identity unchanged). Match syntax uses `<MATCH:regex>` for variant fields and literal values for invariants. Pattern A paired oracle (Oraculurgy Part 11). |
| `wt-03-sibling-group-fixture.json` | Two scenarios for the WT-03 additive `siblingGroup` field on peer objects: two-pods-workers-same-sibling-group-different-workdirs (worktree-mode case — both peers emit `siblingGroup`, workDirs diverge), non-pods-worker-no-sibling-group (additive contract — key absent, not null). v2 fixture; v1 fixtures (`csi-01-*`) MUST continue to validate against the unchanged base shape. Lineage: DEC-61 (Arc 1, 2026-05-10). |

## Public API & Usage

To validate the live registry against this fixture in a sandbox:

```bash
# Bash smoke (calls the Python verifier per scenario):
bash claude-template/scripts/csi-tests/csi-01-smoke.sh

# Or invoke the verifier directly against an existing sandbox:
python claude-template/scripts/csi-tests/csi-01-fixture-drift.py \
    --fixture claude-template/oracles/dispatchable-sessions/test-fixtures/csi-01-registration-fixture.json \
    --scenario interactive-session-fresh \
    --sandbox <path-to-sandbox-with-.claude/-populated>
```

Verifier exits `0` on no-drift, `1` on drift detected, `2` on invocation
error.

## Constraints & Business Rules

- **Pattern A paired oracle** (Oraculurgy `FOUNDATIONS/ORACULURGY_DESIGN.md`
  Part 11): the fixture documents expected state; the
  `dispatchable-sessions` oracle reads the live state. Drift between the
  two surfaces a CSI-01 regression in the producing code (the session-start
  hook).
- **Verifier-mandated** (F1-RT): no fixture in this directory ships without
  a consuming verifier. The verifier lives in
  `claude-template/scripts/csi-tests/` (cross-mechanism) and is invoked by
  the CSI-01 smoke harnesses on every scenario sandbox.
- **Match syntax**: fields whose values begin with `<MATCH:regex>` are
  validated as Python-flavored regular expressions. Fields without the
  prefix are matched literally. Lists are order-sensitive unless wrapped in
  a predicate key (`sessionsContainsExactlyOneWith`, `sessionsAt0`).
- **Sample literals are illustrative**: cross-scenario invariants (sessionId
  stable across re-invocation, spawnedAt preserved from input) are asserted
  by the smoke harness's literal-value tests, not duplicated as fixture
  values. The fixture's role is shape-level enforcement.

## Relationships & Dependencies

- **Consuming verifier**: `claude-template/scripts/csi-tests/csi-01-fixture-drift.py`
- **Consuming smokes**: `claude-template/scripts/csi-tests/csi-01-smoke.{sh,ps1}`
- **Oracle**: `claude-template/oracles/dispatchable-sessions/run.{sh,ps1}`
  (paired oracle — reads the real registry; drift between this fixture and
  the oracle's output flags a regression).
- **Schema authority**: DISPATCH-01 (`docs/specs/SPECIFICATION.md`, locked
  at commit `8077fa6`) defines the registry shape this fixture documents.

## Decision Log

- **Why a per-oracle test-fixtures directory rather than a global tests
  fixtures directory** — fixtures of an oracle's input/output shape are
  tightly coupled to the oracle's contract and should live next to it.
  Cross-oracle fixtures (or fixtures of generated artifacts the framework
  writes broadly, like `current-session.md`) live in
  `claude-template/templates/fixtures/`.
- **Why this directory was missing a README in Stage 1** — the fixture was
  authored by STAGE1-CSI-01 as the first occupant of a new path. Stage 4
  (R-A A1) closed the ULADP gap with this README and Stage 4 (F1-RT) wired
  the verifier so the fixture is now an enforced contract.
- **Why the fixture documents three scenarios rather than one** — each
  scenario captures a distinct registration code path (no env vars, env
  vars + pre-existing entry, re-invocation idempotence). One scenario would
  validate one path; together they validate the full self-registration
  state machine.
