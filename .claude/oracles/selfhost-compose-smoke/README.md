# selfhost-compose-smoke

**Kind**: Verification Oracle (Probandurgy — P4 Stage 2 Task Zero, leg 2 of three-leg defense for FR-FBR-17).

**Question**: Does `deploy/docker/docker-compose.yml` parse cleanly (yaml-lint),
do its application env-var references match the canonical C21 catalog in
`docs/operations/SELFHOST_ENV.md`, and (with `--full`) does
`docker compose down -v && up -d` from a clean state bring the stack to
HTTP 200 on `/health/ready` within 90s?

## Synopsis

Verification Oracle (P4 Task Zero) defending FR-FBR-17 self-host distribution: `deploy/docker/docker-compose.yml` parses cleanly, its app env-var references match the canonical C21 catalog (`docs/operations/SELFHOST_ENV.md`), and (with `--full`) a clean `down -v && up -d` reaches HTTP 200 on `/health/ready` within 90s. Re-run after editing the compose file or the env catalog.

## Probes

### Probe A — yaml-lint

Runs `docker compose -f deploy/docker/docker-compose.yml config --quiet`
when the `docker` CLI is on `PATH`. Exits clean (PASS) on success;
surfaces compose-config errors verbatim on failure (FAIL).

**Fallback when `docker` is not installed**: pure-Python `yaml.safe_load`
via the optional `pyyaml` dependency. If pyyaml is also unavailable, the
probe SKIPs with a WARN message instructing the operator to install
Docker Desktop or `pip install pyyaml`. The fallback is structural only —
it cannot validate `${VAR}` interpolation, `depends_on` references, or
profile semantics; the `docker compose config` path is preferred.

**Cold-start**: if `deploy/docker/docker-compose.yml` does not exist yet
(P4 Stage 2 Phase 1 hasn't authored it), Probe A reports SKIP (treated
as vacuous PASS for exit-code purposes).

### Probe B — env-doc cross-reference against Contract C21

Parses `deploy/docker/docker-compose.yml` to extract every application
env-var reference: `${FEEDBACKMONK_*}`, `${FEEDBACKMONK_*:-default}`,
`${FEEDBACKMONK_*:?error}`, and bareword `FEEDBACKMONK_*` /
`DATABASE_URL` / `RUST_LOG` occurrences (covers the
`environment: - DATABASE_URL` host-passthrough shorthand). `POSTGRES_*`
vars are excluded — they are container-level postgres-image
initialization keys, not application-side configuration. `RUST_LOG`
and `DATABASE_URL` are the two C21-documented non-prefixed vars
(tracing EnvFilter and Postgres connection string respectively, both
standard non-project conventions).

Parses the C21 catalog out of `docs/operations/SELFHOST_ENV.md` by walking
markdown table rows above the `## Out of Scope` cutoff (deferred-vars
section excluded). Names are extracted from the first-cell backtick
pattern: `` | `NAME` | ... | ``.

Set-compares the two:

- **`compose ref ∉ C21 catalog`** → **FAIL** (load-bearing invariant).
  Catches typos (`FEEDBACKMONK_MAILEER` → not in catalog) and undocumented
  knobs (a knob in compose that operators have to discover by reading the
  yaml — a docs gap). Remediation: APPEND a row to SELFHOST_ENV.md
  (GUIDE §8 pre-authorized self-mediated widening), do NOT silence the oracle.
- **`C21 catalog entry ∉ compose ref`** → **WARN** (not FAIL).
  Operators may legitimately omit optional vars. Surfaced for visibility.

**Cold-start**: if the compose file is absent, Probe B is vacuous PASS
(no compose to scan).

### Probe C — clean-state smoke (gated behind `--full`)

`docker compose -f deploy/docker/docker-compose.yml down -v && up -d
--build --wait`, then polls `http://localhost:${FEEDBACKMONK_PORT:-14304}/health/ready`
every 2s for up to 90s. Asserts:

1. HTTP 200 response within the timeout.
2. JSON body parseable.
3. `body.status == "ok"` AND `body.db_connected == true`.

On failure, captures the last response + `docker compose logs --tail 60 api`
for diagnosis. **Always** runs `docker compose down -v` in a `finally`
block so failure paths don't leave dangling containers / volumes (no
clean-up burden on the operator).

Probe C is **off by default**. Inner-loop cost is Probe A + B only
(<500ms). `/0-uldf-finalize` Phase 11 and the post-launch CI gate run
with `--full`.

**Cold-start**: if `--full` is passed but compose file is absent, Probe C
is SKIPPED (treated as vacuous PASS).

## Three-leg defense (per P4 plan § Testability Gate FR-FBR-17)

| Leg | Mechanism | File / location |
|---|---|---|
| 1. Type-system / fail-fast chokepoint | Env-reader `.context("…")` chains in `crates/feedbackmonk-api/src/main.rs` produce startup errors naming the missing var. `FEEDBACKMONK_MAILER` validates against `mailpit`/`smtp` literal; `FEEDBACKMONK_SESSION_SECRET` validates length. | `crates/feedbackmonk-api/src/main.rs` |
| 2. AST / artifact oracle (this file) | Probe A (yaml-lint) + Probe B (env-doc-xref against C21) + Probe C (`--full` clean-state smoke) | `.claude/oracles/selfhost-compose-smoke/` |
| 3. Operator runbook | Cold-readable `docs/operations/SELFHOST.md` — operator-cold-start path documented step-by-step | `docs/operations/SELFHOST.md` |

## Invocation

```bash
# Unix / Git Bash / WSL — inner-loop fast path (A + B only):
bash .claude/oracles/selfhost-compose-smoke/oracle.sh

# Full loop (adds Probe C clean-state smoke; ~60-180s with cold pull/build):
bash .claude/oracles/selfhost-compose-smoke/oracle.sh --full

# Windows (PowerShell):
pwsh .claude/oracles/selfhost-compose-smoke/oracle.ps1
pwsh .claude/oracles/selfhost-compose-smoke/oracle.ps1 --full

# Direct Python (cross-platform):
python .claude/oracles/selfhost-compose-smoke/oracle.py
python .claude/oracles/selfhost-compose-smoke/oracle.py --full
```

Exit `0` on PASS, `1` on FAIL, `2` on environment failure (Python not found).

## Output schema

PASS (post-Phase-1, default mode):

```
PASS selfhost-compose-smoke
  Probe A (yaml-lint): `docker compose config --quiet` on deploy/docker/docker-compose.yml: clean
  Probe B (env-doc-xref): compose env-refs (8) ⊆ C21 catalog (16); 8 catalog entries unreferenced (acceptable)
  Probe C (full-smoke): skipped (pass --full to run clean-state smoke)
```

PASS (with `--full`):

```
PASS selfhost-compose-smoke
  Probe A (yaml-lint): `docker compose config --quiet` on deploy/docker/docker-compose.yml: clean
  Probe B (env-doc-xref): compose env-refs (8) ⊆ C21 catalog (16); 8 catalog entries unreferenced (acceptable)
  Probe C (full-smoke): clean-state up → /health/ready 200 in <90s (status='ok', db_connected=True, version='0.1.0')
```

PASS (cold-start, pre-Phase-1):

```
PASS selfhost-compose-smoke
  Probe A (yaml-lint): vacuous PASS — compose file absent at deploy/docker/docker-compose.yml — cold-start state, run after Phase 1 authors it
  Probe B (env-doc-xref): vacuous PASS — compose file absent at deploy/docker/docker-compose.yml — cold-start state, Probe B vacuous (no compose to scan)
  Probe C (full-smoke): skipped (pass --full to run clean-state smoke)
```

FAIL example (env-doc drift):

```
FAIL selfhost-compose-smoke (1 probe(s) failed)

Probe B failure (env-doc-xref): compose env-refs (9) ⟂ C21 catalog (16) — 1 undocumented, 8 unreferenced
  FEEDBACKMONK_NEW_KNOB  referenced in compose but missing from C21 catalog (docs/operations/SELFHOST_ENV.md). Remediation: APPEND a catalog row (pre-authorized self-mediated widening per GUIDE §8) — do NOT silence the oracle.
```

## Why `--full` is gated

Per P4 plan § Strategy Rationale, the inner-loop cost should stay low
enough that agents re-run the oracle after every edit without paying
docker-pull-build-up-down cost. Probe A + B together run in <500ms;
Probe C is 60-180s on cold pull/build and amortizes to ~30s on warm
cache. Gating Probe C behind `--full` keeps Tier-1 yaml-lint + Tier-2
env-doc-drift cheap while preserving the operator-visible "does it
actually come up" check at finalize / CI time.

## Why POSTGRES_* is excluded

The `POSTGRES_USER` / `POSTGRES_PASSWORD` / `POSTGRES_DB` env vars
configure the official `postgres:*` container image's database
initialization at first-boot (see the postgres image docs). They are
NOT part of the application-side env-var surface — feedbackmonk's
binary reads `DATABASE_URL`, not the components. C21 catalogs the
application surface; the postgres-image surface is fixed by upstream
docs and doesn't belong in C21.

## Why /health/ready (and not /health) for Probe C

`/health` is a liveness probe and always returns HTTP 200 (the body's
`status` flips to `"degraded"` on DB failure but the HTTP code stays
200 so load balancers can distinguish "alive but degraded" from
"dead"). `/health/ready` is the readiness probe and returns **503**
when the DB ping fails. Probe C wants binary go/no-go semantics, so
it asserts HTTP 200 against `/health/ready` AND inspects the body's
`status == "ok"` + `db_connected == true` for defense-in-depth.

## Cold-start vacuous-PASS plan

This oracle ships in Task Zero — **before** the compose stack itself is
authored. Cold-start state (no `deploy/docker/docker-compose.yml`):

- Probe A: SKIP (treated as vacuous PASS — "file absent, run after
  Phase 1 authors it").
- Probe B: SKIP (treated as vacuous PASS — no compose to scan).
- Probe C: SKIP (treated as vacuous PASS — `--full` is moot without
  a compose file).

Exit code: **0**. The oracle's lineage and trigger-list activate as
soon as Phase 1 lands `deploy/docker/docker-compose.yml`, at which
point Probes A + B execute against the real artifact.

## Lineage

- **FR-FBR-17** — self-host `docker compose up` distribution
- **DEC-FBR-IMPL-06** — three-probe smoke oracle decision
- **Contract C21** — env-var catalog SSOT (`docs/operations/SELFHOST_ENV.md`)
- **Contract C24** — three-probe schema
- **P4 plan §Oracle Pre-Build Plan** — Probe A + Probe B + Probe C-gated
- **P4 plan §Testability Gate FR-FBR-17** — composite ~14 → scaffolding pairing
- **Three-leg defense pattern** — env-reader chokepoints + this oracle + operator runbook

## Decision log

- **File-naming**: `oracle.{py,sh,ps1}` matches the existing oracle
  conventions in `widget-bundle-size`, `multi-tenant-isolation-check`,
  `pii-scrub-audit`, `tier-enforcement-status`. `manifest.toml` is a
  brief-named TOML mirror; `manifest.json` is authoritative at runtime.
- **Probe A fallback to pyyaml**: docker CLI may not be installed in
  every agent's environment (CI runners, fresh dev VMs). The pyyaml
  fallback gives a partial-but-useful structural check; full validation
  requires Docker Desktop. WARN messaging guides operators.
- **Probe B excludes POSTGRES_***: those vars belong to the postgres
  image's container-init protocol, not the application surface. Including
  them in Probe B would force-add them to C21, polluting the
  application catalog.
- **Probe B FAIL direction is asymmetric**: undocumented compose ref →
  FAIL (the load-bearing invariant); catalog entry not in compose → WARN
  (operators legitimately omit optional vars).
- **Probe C polls `/health/ready` not `/health`**: `/health` always
  returns 200; `/health/ready` is the binary go/no-go (per the handler
  at `crates/feedbackmonk-api/src/handlers/health.rs:57`).
- **Probe C 90s timeout**: cold pull + first-build of the api container
  (cargo-chef cooks the dep layer once, then incremental) can take 60s+
  on a fresh machine. 90s is conservative; subsequent runs (image
  cached) typically <30s.
- **Probe C teardown in `finally`**: PEP-recommended pattern; ensures
  failure paths (compose up errors, health-check timeouts) leave a
  clean state. The cleanup itself is best-effort — a `down -v` failure
  surfaces a stderr warning, the operator may need `docker compose
  down -v` manually.
- **No `c21-catalog.expected-keys.txt` hash anchor**: the task brief
  mentioned this as optional. We parse SELFHOST_ENV.md directly on each
  run, which keeps the oracle in single-source-of-truth alignment with
  C21 — appending a row to SELFHOST_ENV.md immediately changes Probe B's
  expected set with zero ceremony. Trade-off: no hash-detection of
  catalog drift, but C21 is frozen Stage 1 (LD-ratified appends only)
  so drift detection is governance-layer, not oracle-layer.
