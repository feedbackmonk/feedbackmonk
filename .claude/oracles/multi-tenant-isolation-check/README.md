# multi-tenant-isolation-check

<!-- agent-synopsis -->
Verification Oracle (P0 Task Zero). Polices DEC-FBR-03's "raw SQL outside the repository layer is a security incident" rule at AST grade on every commit. Leg 2 of the three-leg tenant-isolation defense.
<!-- /agent-synopsis -->

## Purpose

Answers the question: *Does every domain-touching code path go through tenant-scoped repository methods?*

Two probes:

- **Probe A** — raw SQL outside the repository crate: greps every `.rs` file under `crates/` (except `crates/feedbackr-repository/`) for forbidden patterns: `sqlx::query`, `&mut Connection`, `&mut PgConnection`, `&mut Transaction`, `Pool<Postgres>`, `pool.acquire(`.
- **Probe B** — repository-method scope discipline: parses every `pub fn` / `pub async fn` signature in `crates/feedbackr-repository/src/**.rs` and verifies the first non-`&self` argument is `&TenantScope` or `&ProjectScope` (or appears in `allowlist.toml`).

Output: `PASS` (exit 0) when both probes are clean; `FAIL <count>` with `file:line` offenders and exit 1 otherwise.

## File Index

| File | Purpose |
|---|---|
| `manifest.json` | Oracle metadata: name, kind (`verification`), triggers, freshness strategy, consumer scope. |
| `allowlist.toml` | Methods that legitimately deviate from the first-arg-scope rule. Each entry carries an inline rationale. Currently 3 pre-auth trait methods + 4 inherent constructors. |
| `oracle.py` | **Canonical implementation.** Python 3.8+ — performs balanced-paren parsing of multi-line Rust signatures with context tracking the shells cannot do reliably. |
| `oracle.ps1` | Thin shim that invokes `oracle.py`. Windows entry point. |
| `oracle.sh` | Thin shim that invokes `oracle.py`. Unix entry point. |

## Public API & Usage

```bash
# From repo root, any of:
python .claude/oracles/multi-tenant-isolation-check/oracle.py
bash   .claude/oracles/multi-tenant-isolation-check/oracle.sh
pwsh   .claude/oracles/multi-tenant-isolation-check/oracle.ps1

# Exit 0 + "PASS" on success; exit 1 + "FAIL <count>" with offender lines on failure.
```

Triggered by changes to: `migrations/**`, `crates/feedbackr-repository/**`, `crates/feedbackr-core/**`, `crates/feedbackr-api/**`, `crates/feedbackr-jwt/**` (Stage 2), `crates/feedbackr-anon/**` (Stage 2), `.claude/oracles/multi-tenant-isolation-check/allowlist.toml`.

## Constraints & Business Rules

- **Probe A has NO allowlist.** DEC-FBR-03 declares any raw SQL outside `crates/feedbackr-repository/` a security incident — there is no legitimate use case. Don't add one without a written DEC-FBR-* amendment.
- **Probe B allowlist requires inline rationale.** Adding an entry to `allowlist.toml` without a rationale comment is forbidden; the oracle's value comes from making each exception explicit.
- **CI gate from commit 1.** The build fails on oracle red. This is by design — a passing CI with a red isolation oracle is worse than no oracle at all.
- **Speed contract**: <2s end-to-end on a clean tree. Currently ~250ms.

## Relationships & Dependencies

- **Type-system half (leg 1)**: `crates/feedbackr-repository/src/scope.rs` — `TenantScope` / `ProjectScope` with `pub(crate)` constructors.
- **Lint half (leg 3)**: `Cargo.toml` workspace `clippy::all = deny` + per-crate `clippy::pedantic`, plus `cargo-deny` checks in `deny.toml`.
- **Consumed by**: every commit in P0+; CI workflow at `.github/workflows/ci.yml`; Stage 2 Workers A and B (gates their commits); future P1+ code (gates indefinitely).

## Decision Log

### Three-leg defense: type system + AST oracle + clippy

**Decision**: Tenant isolation is enforced by three independent mechanisms — `TenantScope`/`ProjectScope` newtypes at the type-system layer, this oracle at the AST layer, and clippy/cargo-deny at the static-analysis layer.

**Rationale**: Q2=5 (silent fidelity risk) on FR-FBR-01: a passing unit test does NOT prove cross-tenant isolation under all future query paths. One mechanism is brittle; two is fragile; three is resilient — the legs are independent (a bug that defeats the type system likely doesn't defeat AST grep, and vice versa). This is the canonical Probandurgy pattern for high-Q2 surfaces.

**Trade-offs**: Three legs to maintain. The maintenance cost is bounded — the oracle is `<300` lines of Python, the newtypes are `~60` lines of Rust, and clippy/cargo-deny are config-only. The cost of one undetected cross-tenant leak (months in the wild, customer trust damage, GDPR exposure) dwarfs the maintenance.

**Implementation**: All three legs live in this repo and are exercised on every commit. See `crates/feedbackr-repository/README.md` for the leg-1 details and `Cargo.toml` + `deny.toml` for leg-3.

### Canonical implementation in Python, not pure shell

**Decision**: `oracle.py` is the canonical implementation; `oracle.ps1` and `oracle.sh` are thin shims that delegate to it.

**Rationale**: Probe B requires balanced-paren multi-line Rust signature parsing with context tracking. The initial bash port produced 25 false positives on a clean tree due to POSIX shell's context-tracking limitations (`grep` cannot follow signatures across lines without significant gymnastics). Python 3.8+ is ubiquitous on CI Ubuntu and developer machines; the dependency cost is real but small. The trade-off favors correctness — a false-positive oracle is not just annoying, it's *trained-to-ignore*, which silently degrades to no oracle at all over a few weeks.

**Trade-offs**: Adds Python to the oracle dependency set. Documented in file headers. CI workflow explicitly installs Python 3.8+ if absent.

**Implementation**: `oracle.py` is the implementation; shims forward `python3 oracle.py "$@"` to it. Both shims verified PASS on clean tree and FAIL on a planted `sqlx::query` violation.

### Allowlist entries require inline rationale

**Decision**: Every entry in `allowlist.toml` must carry an inline `rationale = "..."` field documenting WHY the method deviates from the first-arg-scope discipline.

**Rationale**: Allowlists drift. Without per-entry rationale, future maintainers cannot tell whether an entry is load-bearing (e.g. `TenantRepo::create` — pre-auth signup, no scope exists yet) or vestigial (e.g. an entry added during debugging and never removed). Forcing the WHY at entry time pays back at audit time. This is the same principle as `cargo deny` advisories carrying explanations.

**Trade-offs**: Adding an allowlist entry is slightly more work. By design — the friction is the feature.

**Implementation**: `allowlist.toml` schema: `[[methods]]` or `[[inherent_methods]]` blocks, each with `trait`/`type_name`, `method`, and `rationale` fields. Oracle code does not check the rationale string content (that's a human review job), only its presence.

### Freshness contract triggers on allowlist changes

**Decision**: `allowlist.toml` is listed in `manifest.json` `freshness.triggers` — editing it invalidates the oracle and forces re-run.

**Rationale**: An allowlist edit is precisely the kind of action that should re-trigger the oracle, because it changes the rules. If allowlist edits did NOT invalidate, a developer could add an over-broad entry and commit while the oracle's cached result still showed green from before the edit — defeating the audit trail.

**Trade-offs**: Slightly more frequent oracle invocations. Cost is ~250ms per run; negligible.

**Implementation**: `manifest.json` `freshness.triggers` line 24 includes the allowlist path.
