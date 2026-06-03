# cors-allowlist-enforcement

## Synopsis

Verification Oracle (`kind: verification`) that defends the credentialed-CORS posture for the public widget endpoints (feedback submission + attachment upload) as a **code-state invariant**. It reads the router wiring in `main.rs` and the policy in `cors.rs` to confirm the CORS layer DEC-FBR-IMPL-09 introduced is still built, still fed by `FEEDBACKMONK_CORS_ORIGINS`, still applied to both public endpoints, and still credentialed echo-origin (never `*`). Fast (~60ms, static, no compile/DB).

## Why this exists (the gap it closes)

`crates/feedbackmonk-api/tests/cors_preflight.rs` exercises `public_cors_layer(...)` **in isolation**. It therefore stays green even if someone deletes the `.layer(cors)` wiring from `main.rs::build_app` — i.e. it cannot catch a *wiring-removal* regression, which is exactly how the original GitCellar embed bug (preflight `OPTIONS` → `405`) would silently return. This oracle reads the wiring and policy from source, closing that gap. Together: the test proves the layer is *correct*; the oracle proves the layer is *applied*.

## Probes

### Probe A — Wiring (static, `main.rs`)

Confirms `build_app`:
- calls `public_cors_layer(...)` (the layer is built),
- reads `FEEDBACKMONK_CORS_ORIGINS` (the allowlist source exists),
- applies `.layer(<cors>)` to **both** `submission_router(...)` and `attachments_router(...)`.

A failure here means the browser preflight will `405`-regress on the affected endpoint.

### Probe B — Policy (static, `cors.rs`)

Confirms `public_cors_layer`:
- uses `.allow_credentials(true)` (the anonymous `credentials: include` path needs `Access-Control-Allow-Credentials`),
- uses `AllowOrigin::list(...)` (per-request echo from an explicit allowlist),
- uses **no** wildcard origin (`AllowOrigin::any()` / `allow_origin(Any)` / `cors::Any`). A wildcard with credentials is invalid per the Fetch spec (and `tower-http` panics), and the spec requires the specific origin to be echoed.

### Probe C — Integration smoke (gated behind `--full`)

Runs `cargo test -p feedbackmonk-api --test cors_preflight`. NOT run in the inner loop unless `--full` is passed (keeps Probes A+B sub-100ms for every-commit consultation).

## Invocation

```bash
# Unix / Git Bash / WSL — inner-loop fast path (A + B only):
.claude/oracles/cors-allowlist-enforcement/oracle.sh

# Full loop (adds Probe C integration smoke):
.claude/oracles/cors-allowlist-enforcement/oracle.sh --full

# Windows (PowerShell):
.claude\oracles\cors-allowlist-enforcement\oracle.ps1

# Direct Python (cross-platform):
python .claude/oracles/cors-allowlist-enforcement/oracle.py [--full]
```

## Output schema

`PASS cors-allowlist-enforcement` (exit 0) or `FAIL cors-allowlist-enforcement (<n> probe(s) failed)` (exit 1) with per-probe offender detail and remediation. Exit 2 on environment failure (e.g. Python missing).

## Lineage

- **DEC-FBR-IMPL-09** — CORS on public widget endpoints + cross-site anon cookie (the change this oracle defends).
- **DEC-FBR-04** — domain allowlist for widget embed (CORS at the submission endpoint); this oracle is the code-state guard half.
- **DEC-FBR-IMPL-03** — canonical-Python + bash/ps1-shim oracle implementation pattern.

## Decision log

### Static probes over a router-level integration test

**Decision**: Probes A/B read source statically rather than building the real `app` router and sending a live preflight.

**Rationale**: a router-level test would need a full `AppState` + Postgres, pushing it into the slow lane and making it DB-dependent. The wiring-removal regression is fully observable from `main.rs` source, so a static read catches it at ~60ms with no infrastructure — appropriate for every-commit inner-loop consultation. The behavioral guarantee (does the layer actually answer a preflight) is already covered by `tests/cors_preflight.rs`; Probe C re-runs it on demand under `--full`.
