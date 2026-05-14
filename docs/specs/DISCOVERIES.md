# DISCOVERIES

Insights surfaced during Feedbackr implementation that are worth preserving beyond the session-completion report. Append-only; newest at bottom.

---

## P0 Stage 1 (2026-05-13)

### D-FBR-01: Verification Oracle authoring — Python beats pure shell when parsing crosses lines

**Surfaced by**: `multi-tenant-isolation-check` oracle authoring (Stage 1 worker).

**What was discovered**: A pure-bash implementation of Probe B (Rust signature first-arg parsing) produced 25 false positives on a clean tree due to POSIX shell's inability to track context across line continuations. The fix — Python 3.8+ as canonical with shell shims — was clean and uncovered a reusable pattern.

**Generalizable insight**: Verification Oracles that need to parse syntax across lines (signatures, brace-balanced blocks, multi-line config) should default to **Python canonical + thin OS shims** rather than pure shell. The mental model:

| Parser need | Tool |
|---|---|
| Single-line grep / pattern match | Pure shell (`grep`, `awk`) |
| Cross-line / balanced-delimiter parsing | Python (`re` + manual tokenization), shell shims forward |
| Structural AST manipulation | tree-sitter or syn-cli (Rust-specific) |

**Where this pays off again**: Future Feedbackr Verification Oracles likely to need this pattern — `pii-scrub-audit` (P1, drift-detection over a canonical pattern set with regex alternation requiring balanced parens), `tier-enforcement-status` (P3, parses Rust enum + match-arm structure to verify cap-check completeness). Documented in DEC-FBR-IMPL-03.

---

### D-FBR-02: Three-leg defense pattern generalizes — type system + AST oracle + lint baseline

**Surfaced by**: P0 Stage 1's tenant-isolation defense design.

**What was discovered**: The strongest defenses against silent fidelity failures (Q2=5 surfaces in Testability Gate language) combine **three independent legs**:

1. **Type system** — newtypes / sealed traits / `pub(crate)` constructors that make the invariant unrepresentable in incorrect code (when it works).
2. **AST oracle** — Verification Oracle that greps for forbidden patterns and checks structural invariants on every commit (catches what types miss, e.g. raw-SQL strings that the type system cannot see through).
3. **Lint baseline** — clippy / cargo-deny / project-specific lints that catch foot-guns at compile time and refuse to let the build pass with warnings.

The legs are **independent** — a bug that defeats one likely does not defeat the others. Two legs alone is fragile (every safety-critical exploit history is "the one mechanism failed"); three legs is resilient.

**Generalizable insight**: For any future high-Q2 invariant in Feedbackr (JWT verifier alg-confusion, PII scrubber pattern drift, tier-cap enforcement, AGPL header presence), design the defense as a three-leg architecture from the start. Pick what each leg checks; pick patterns that are genuinely independent (a single underlying bug should not invalidate two legs at once).

**Where this pays off again**:

- **FR-FBR-05 JWT verification** (Stage 2 Worker B): leg 1 = JWT-library API + per-aud type guard; leg 2 = JWT fixture corpus with named tests per invariant; leg 3 = clippy + a possible drift-detection oracle over the corpus signature.
- **FR-FBR-10 PII scrub** (P1): leg 1 = central scrubber function with a single chokepoint; leg 2 = `pii-scrub-audit` oracle; leg 3 = clippy/build-time pattern-count check.
- **FR-FBR-14 tier enforcement** (P3): leg 1 = enum-exhaustive `match` on tier; leg 2 = `tier-enforcement-status` oracle; leg 3 = workspace pattern check that every cap is paired with a tier check.

---

### D-FBR-03: Pre-auth allowlist is a recurring shape — name the boundary explicitly

**Surfaced by**: `scope_for` allowlist debate during P0 Stage 1.

**What was discovered**: Type-system isolation (e.g. `TenantScope` with `pub(crate)` constructor) **necessarily** has a small set of methods that mint the first scope from a verified caller. The naive instinct ("just have private fields and let the compiler enforce it") doesn't work because *something* has to bridge "verified credential" → "scope value" at the auth boundary.

The pattern: name those bridge methods explicitly, gate them through an allowlist with per-entry rationale, and trigger oracle re-runs when the allowlist changes. This makes the "what crosses the boundary" question audit-friendly (a 30-line `allowlist.toml` shows the entire attack surface) instead of buried in source code.

**Generalizable insight**: Any type-system isolation boundary in Feedbackr (signing-key access, anonymous-mode rate-limit counters, future RBAC scopes, future organization-level admin scopes) should follow the same pattern: `pub(crate)` constructor + named allowlist of bridge methods + oracle that enforces.

**Where this pays off again**: Stage 2 Worker B's `verify()` on the JWT crate is a similar boundary — only one method mints "verified payload" from raw token bytes. That allowlist would be a single entry but the same shape (explicit, rationale-bearing, oracle-triggered).

---

### D-FBR-04: Test-categorization opportunity — property-based tests for cross-tenant rejection

**Surfaced by**: Phase 5 (Test Maintenance) categorization during this finalize.

**What was discovered**: Stage 1's 19 tests are all example-based (specific tenants A and B with specific projects). They are **correct** and pass per the matrix as `MATRIX-CAT-DIFFERENTIAL` (cross-tenant: same input, two implementations — implementations of "isolate by tenant" via repo trait — must produce non-overlapping outputs). What is **not yet present**:

- **`MATRIX-CAT-PBT` test (property-based)** for `ProjectRepo::open()` — assert: "for all `(tenant_a, tenant_b)` with `a ≠ b` and any `project_id` belonging to `b`, `open(&tenant_a_scope, project_id)` returns `TenantProjectMismatch`." Currently covered by 1 example; PBT with `proptest` would generalize.
- **`MATRIX-CAT-PBT` test for `FeedbackId::generate`** — assert: "for all 1000 generated IDs, none collide and all match `FB-\d+` format." Currently covered by uniqueness test that runs once.

**Generalizable insight**: Whenever the Testability Gate scores Q2=5 on a requirement, follow up Stage-N example tests with Stage-N+1 property tests using the same trait surface. The PBT companion crate (`proptest`) is a cheap add (one Cargo.toml line) and converts each invariant into a "for all" statement instead of a "for this one" statement.

**Status**: Recorded as a recommendation for Stage 2 Worker B's test addition (matches the JWT fixture corpus shape mandated by the Testability Gate for FR-FBR-05). Existing 19 tests stay IMMUTABLE per project CLAUDE.md byte-for-byte rule.

---

## P0 Stage 2 + Stage 3 (2026-05-13) — P0 Close

### D-FBR-05: Pre-auth allowlist as a repeatable widening mechanism (DEC-PODS-001/002)

**Surfaced by**: Stage 2 PODS convergence. Two independent workers (CLAUDE-A signup, CLAUDE-B submission) each hit the same pattern — needed a single pre-auth method on the repository surface to bridge an externally-verified credential into a first scope value. Both widenings (`ProjectRepo::open_for_submission(project_id)` for JWT-mode submission; `EmailVerificationRepo::redeem(token)` for verify-email) were pre-specified in the P0 plan task briefs, self-mediated by the workers under autopilot:continuous, then LD-ratified after review.

**Generalizable insight**: The pre-auth allowlist (`.claude/oracles/multi-tenant-isolation-check/allowlist.toml`) is now a **proven repeatable mechanism** for legitimate Contract-C1 widening. The pattern:

1. **Plan-time** — the `/0-uldf-ldis-plan` round identifies the bridge method by exact signature in the task brief, citing which verified credential the method consumes.
2. **Worker-time** — worker adds the entry to `allowlist.toml` with `rationale = "Pre-auth: ..."` explaining what is verified upstream.
3. **Oracle-time** — `multi-tenant-isolation-check` re-runs (allowlist is a trigger file) and confirms the new entry's first-arg signature passes type discipline.
4. **LD-time** — convergence-stage critic confirms (a) the rationale parallels existing entries and (b) the widening is upstream-mandated.

**Where this pays off again**:
- **P1 webhook delivery** (FR-FBR-07/08): inbound provider webhooks need an `Outbound{Status,Reply}Repo::create_for_external_event(verified_signature, …)` pattern — same shape.
- **P3 tier enforcement** (FR-FBR-14): tier-cap counter reads under public-roadmap voting (anonymous mode) — same boundary, same pattern.
- **P2 widget ingestion** (FR-FBR-04): per-origin CSP-validated submissions where the origin-check IS the credential.

The pattern's resilience comes from the rationale-bearing allowlist being **fast to audit** (~30 lines after P0; will stay under 100 lines through P4). Future agents reviewing the file can answer "what crosses the tenant-isolation boundary in this repo?" in one read.

---

### D-FBR-06: PODS LD-in-monitor coordination latency — alerts.md should signal self-mediation authority

**Surfaced by**: Stage 2 collaboration `collab-20260513-221600`. CLAUDE-B alerted on a needed Contract C1 widening (`open_for_submission`) and waited approximately 50 minutes for LD response before self-mediating under its autopilot:continuous BoundConsent. The widening was pre-specified in CLAUDE-B's task brief — self-mediation was structurally correct — but the wait time wasted worker context budget.

**What was discovered**: When the LD is in **script-monitor mode** (polling an orchestrated worker via blocking-agent script), it has no awareness of PODS `alerts.md` writes from sibling workers. The PODS coordination model assumes the LD is interactively reading channels; the orchestrated-execution model assumes the LD is asleep in a polling wait. The two models don't compose well when both run concurrently.

**Generalizable insight**: When LD is in monitor mode AND has live PODS workers, the alerts.md write protocol should include:

```markdown
**LD-state**: script-monitor (orchestrated-execution polling; not actively reading channels)
**Self-mediation**: AUTHORIZED if change matches a pre-specified plan-time API signature; ratification deferred to LD's natural channel-check (post-monitor-wakeup) OR to convergence.
```

This shifts the question from "wait for LD" → "is this change one the LD already pre-specified?" Worker checks plan + task brief; if signature-match, proceeds and tags `decisions.md` with `self_mediated=true; ratification_pending=true`.

**Where this pays off again**: Any future PODS where LD is also coordinating a serial worker (Stage 3 in this case did not have this issue because LD was actively at the keyboard). Pattern recommendation: bake into `.claude/skills/0-uldf-pods-collab-sync/` workflow as a `--ld-monitor-mode` signaling flag, or into the `/0-uldf-pods-parallelize` skill so the alerts.md template carries the LD-state field automatically.

---

### D-FBR-07: Fixture-corpus-first pattern proved its value for crypto-verifier surfaces

**Surfaced by**: Stage 2 Worker B Task Zero — JWT fixture corpus (24 named tests across 8 categories a-h + boundary/leeway/RS256-attack cases) authored BEFORE the verifier implementation. Each test corresponds to one Contract C2 hard invariant; the fixture is hermetic-deterministic (no clock dependencies; no shared mutable state).

**What was discovered**: Building the fixture corpus first produced two surprising payoffs:

1. **Error-precedence design fell out naturally**: The verifier's "manual base64url + ed25519-dalek for precise error precedence" design (alg-allowlist enforced BEFORE signature work, aud-check BEFORE temporal-check, missing-claim BEFORE wrong-claim) was DRIVEN by the fixture order — each test expected an exact `JwtError` variant, and the only implementation that satisfied them all was one where checks happen in the documented precedence order. The corpus IS the spec.

2. **"alg=none + HS256 confusion" attack class becomes a single-line test**: `fixture_rs256_attack_rejected` and `header_with_no_alg_field_is_algorithm_not_allowed` are each 5 lines including assertions. Without the corpus-first discipline, these would have been afterthoughts; with it, they are first-class verification surfaces.

**Generalizable insight**: For ANY crypto-verifier surface in Feedbackr (P3 webhook signatures FR-FBR-14, future API request signing, future organization-scoped HMAC for self-host customers), apply the same pattern:

1. **Author the fixture corpus first** with a name per invariant + Contract C-N invariant ID in the test name.
2. **Use a single canonical verifier crate** (ed25519-dalek for EdDSA; future: ring or hmac for symmetric) and reject the urge to abstract over algorithm-families.
3. **Manual base64url decode for header parsing** — `serde_json` parsing the decoded header is fine, but defer signature work until alg-allowlist + claim-presence is confirmed. This is the only way to get clean error precedence.

**Where this pays off again**: Forward-binding for P3 webhook signing (planned at `/0-uldf-ldis-plan "Feedbackr P3 — Self-Service Distribution"`). Likely shape: HMAC-SHA256 over canonical request body + `x-feedbackr-timestamp` + `x-feedbackr-signature` header. Author the fixture corpus first with the same 8-category structure (a-h: valid, valid-with-rotation, expired, wrong-secret, missing-header, oversize, attack-class-1, attack-class-2).

---

### D-FBR-08: Anonymous-mode in-memory rate-limiter — restart-loses-state is acceptable for v1.0

**Surfaced by**: Stage 2 Worker B `feedbackr-anon` design. The crate uses `governor::keyed::DefaultKeyedRateLimiter` over a `BLAKE3` hash of `(project_id, ip, salt)` — entirely in-process. Restart of the API binary loses all rate-limit state.

**What was discovered**: For P0 (single-instance dogfood deployment), this is acceptable: an attacker would need to detect a restart in real time to exploit, and the 11-burst-then-429 window is 60 seconds. Even an adversary aware of the restart pattern would only gain 10 extra anonymous submissions per restart — well below noise floor for genuine spam protection.

**Risk surface for v1.1+** (post-launch):
- Multi-instance horizontal scaling **WILL** require shared state. Redis is the obvious target.
- Self-host customers running 24/7 single-instance will see the in-memory limiter behave correctly; v1.0 self-host docs should mention "restart resets anonymous rate-limit counters" as a known property, not a bug.

**Generalizable insight**: Component design decisions like "in-memory vs distributed state" need an explicit **graduation criterion** documented at design-time. The `feedbackr-anon` crate's `RateLimitConfig` already takes `requests_per_minute` and `burst_capacity` — adding a hidden `backend: enum { InMemory, Redis(RedisConfig) }` field in v1.1 is a non-breaking change because the public surface (`gate(project_id, ip) -> Result<()>`) doesn't change.

**Where this pays off again**: ALL stateful components in P1+ (status email scheduler, public-roadmap voting cache, tier-cap counters in P3) should follow the same pattern: in-memory v1.0 with documented graduation criterion + non-breaking backend swap in v1.1.

---

### D-FBR-09: Axum `into_make_service_with_connect_info` is load-bearing for IP-aware handlers

**Surfaced by**: Late Stage-3 e2e debugging. The submission handler's anonymous-mode flow uses `axum::extract::ConnectInfo<SocketAddr>` to extract the source IP for BLAKE3 hashing. Default `axum::serve(listener, app)` does NOT make connection info available — the extractor returns 500. Fix: `axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())`.

**What was discovered**: This is a known axum gotcha (documented in axum's README under "extracting connection info"), but the error surface is **silent** — handlers compile, server starts, only the actual extraction fails at request time. The e2e P0-exit-gate witness caught it; no unit test would have.

**Generalizable insight**: Any axum handler that uses `ConnectInfo<_>`, `MatchedPath`, `OriginalUri`, or other "extracts that need server-level wiring" should have a startup smoke test (or e2e witness) that exercises ONE request per such handler. The Stage 3 e2e script (`scripts/e2e-p0-curl.sh`) is the witness mechanism going forward.

**Where this pays off again**: P1 admin UI handlers (FR-FBR-07: feedback list view) will use `MatchedPath` for OpenTelemetry span naming. P2 widget CDN serving (FR-FBR-04) will use `OriginalUri` for CSP nonce binding. Both need the same wiring; both need e2e witnesses.

---

## P1 Stage 1 (2026-05-13)

### D-FBR-10: Write-boundary scrubbing beats Layer-field scrubbing for log-PII chokepoints

**Surfaced by**: `feedbackr-tracing` PII scrubber design (S1-T1).

**What was discovered**: The P1 plan brief called for a `tracing_subscriber::Layer<...>` impl that applies the canonical-pattern scrub to event field values. The implementation chose a different seam — `ScrubbingMakeWriter` at the formatted-byte WRITE boundary — and the property holds more cleanly. A `Layer` impl scrubs field values, but the formatter still emits level prefixes, span metadata, JSON-encoded structured fields, and timestamp data through paths the Layer never sees. The writer-boundary chokepoint covers ALL emitted bytes because every byte of every log line passes through `MakeWriter::make_writer`.

**Generalizable insight**: For "every X passes through Y" invariants in a stacked subscriber/middleware pattern, prefer the **terminal seam** (where bytes/values cross into the external sink) over an **intermediate transform seam** (where you can be sidestepped by other transforms operating on different fields). The terminal seam is harder to bypass because the transport mechanism itself converges through it. The defense-in-depth check (AST oracle Probe A forbidding `impl Layer<...> for ...` outside the crate) catches the case where a future maintainer is tempted to add a parallel Layer impl that bypasses the writer.

**Trade-off captured**: Writer-boundary scrubbing means the scrub function operates on already-formatted bytes (UTF-8 strings with JSON-escapes possibly applied), not on raw field values. This is fine for regex-based pattern matching but would be the wrong choice for structured field-aware scrubbing (e.g., "redact a specific field name across all log events regardless of value"). Our 20 canonical patterns are all value-pattern-based, so the trade is clean.

**Where this pays off again**: Any future "every emitted X passes through canonical sanitizer" surface — email outbound (FR-FBR-09 P1 Stage 2: every email body passes through canonical link-encoder + footer-injection — Stage 2 should consider the smtp-transport seam, not a per-template transform seam), widget event egress (FR-FBR-04 P2: every widget telemetry event passes through CSP/origin-validation), webhook delivery (P3).

---

### D-FBR-11: Three-tuple pattern records — promote diagnostic identity into the data shape

**Surfaced by**: `CANONICAL_PATTERNS: &[(&str, &str, &str)]` shape design in `feedbackr-tracing`.

**What was discovered**: GitCellar's source stores `Rule { re, replacement }` with the pattern name only in a `//` comment. Porting verbatim would have made the `pii-scrub-audit` Probe B oracle unable to name offenders ("a pattern drifted" rather than "the `aws-access-key-id` pattern drifted") and would have made re-ordering invisible to a hash check (if the regex+replacement pair is unchanged, the bytes are the same regardless of position). Promoting `name` to the first slot of the tuple made both problems disappear: the oracle reports `pattern-set drift: actual=X expected=Y; parsed 20 patterns; review every tuple in <path>`, and the hash includes the human label.

**Generalizable insight**: When a data slice will be consumed by both runtime code AND a Verification Oracle that hash-locks the slice, the slice's **shape itself** has a diagnostic dimension — the oracle's error-message quality depends on what identifiers travel inside the hash. Identifiers that live only in adjacent comments are invisible to the oracle. If you're tempted to put a label in a comment "for clarity," ask: is this label load-bearing for any verification surface? If yes, promote it into the data shape.

**Where this pays off again**: Any future canonical-set-with-drift-detection — `tier-enforcement-status` (P3, canonical tier-cap rules), JWT-verifier hardcoded-alg-list (P0+, currently a const slice in `feedbackr-jwt`), email-template-id manifest (P1 Stage 2, FR-FBR-09).

---

### D-FBR-12: Post-orchestrated-worker finalize needs `--all` (orchestrator owns the convergence commit)

**Surfaced by**: P1 Stage 1 mid-arc commit finalize protocol.

**What was discovered**: In autopilot:continuous Orchestrated Execution, the worker session is spawned in a separate CLI process, produces all code changes, and terminates. The orchestrator session (this one) never *itself* writes the changes via tool calls — it only edits LTADS coordination files (`current-session.md`, `development-brief.md`, etc.) and the plan doc. The standard finalize "Session Files" list would only enumerate orchestrator-side edits, leaving the worker's code changes unstaged.

**Generalizable insight**: The Session Scope Guard in the finalizer agent is the right default (it prevents cross-session contamination in PODS/peer-parallel modes), but Orchestrated Execution needs an explicit `--all` flag because the worker's writes are by-design owned by the orchestrator at convergence. The autopilot loop documented in `start_autopilot.md` ("spawn worker → auto-monitor → auto-finalize → repeat") implicitly requires `--all` at the auto-finalize step.

**Framework-level recommendation** (not project-level): The autopilot loop's auto-finalize invocation should pass `--all` automatically when the topology is Orchestrated Execution (detected via `ltads/orchestration/mode.json` or equivalent), with the rationale rendered into the commit context. Today's flow requires the user (or chain coordinator) to remember the flag.

**Where this pays off again**: Every Orchestrated Execution mid-arc checkpoint commit. P1 Stage 2 PODS converge is a different shape (PODS has its own `/0-uldf-pods-converge` flow with explicit ownership-trail), but a fresh orchestrated worker in P1 Stage 3 or any subsequent phase will hit the same boundary.

---
