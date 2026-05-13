# Task Queue — Feedbackr P0 Foundation

## Active Stage: Stage 1 — Foundation Contract (SEQUENTIAL, 1 agent)

### Task Zero (precedes all data-write code)
- [ ] Build `multi-tenant-isolation-check` Verification Oracle in `.claude/oracles/`
  - Skeleton + freshness contract (trigger-invalidate on migrations/, crates/feedbackr-repository/, crates/feedbackr-core/)
  - AST/grep probe over API + handler crates (no raw sqlx outside repository layer)
  - Repository-method audit probe (every public fn takes `&TenantScope` or `&ProjectScope` first non-`&self`)
  - Wire to CI before any data-write code lands

### Sub-task 1 — FR-FBR-01 (data model + tenant-scoped repository)
- [ ] Cargo workspace skeleton: `crates/feedbackr-core`, `crates/feedbackr-repository`, `crates/feedbackr-api` (placeholder), `migrations/`
- [ ] `migrations/00001_p0_schema.sql` — tenants (incl. `tier` column defaulted `'free'`), projects, signing_keys, feedback, anon_submissions, rate_limit_counters
- [ ] `TenantScope` / `ProjectScope` newtypes in `crates/feedbackr-repository/src/scope.rs` with `pub(crate)` constructors; `open()` as the SOLE `ProjectScope` constructor
- [ ] Repository traits + impls in `crates/feedbackr-repository/src/`: `TenantRepo`, `ProjectRepo`, `SigningKeyRepo`, `FeedbackRepo` — every method takes `&TenantScope` or `&ProjectScope` as first non-`&self` arg
- [ ] `cargo sqlx prepare` to generate `.sqlx/` offline cache; commit cache
- [ ] Verify three-leg defense: type system (newtypes) + oracle (AST + repo-method audit) + clippy/cargo-deny rules

### Stage 1 exit witnesses (durable)
- [ ] Contract C1 frozen exactly as documented in P0 plan §Contract C1 (any deviation → plan revision before Stage 2 fan-out)
- [ ] `multi-tenant-isolation-check` oracle GREEN on the Stage 1 commit; CI fails on oracle red
- [ ] `cargo build` + `cargo test` green; `cargo clippy -- -D warnings` green
- [ ] `docs/planning/handoffs/stage1-to-stage2.md` carry-state document listing frozen contract paths for Workers A + B
- [ ] Backend dev port `14304` claimed in `~/.claude/MACHINE_CONFIG.md` Dev Port Registry; Feedbackr row's API column updated

## Upstream (Stage 2, post-Stage-1 convergence)
- Worker A: FR-FBR-02 signup/onboarding + Contract C4 signing-key registration
- Worker B: FR-FBR-03 submission API + FR-FBR-05 JWT EdDSA verifier + FR-FBR-06 anonymous mode + Contracts C2 + C3 + JWT fixture corpus (Worker B Task Zero)

## Upstream (Stage 3, single agent in converging session)
- Sub-task 4: FR-FBR-18 health endpoint + structured logging + Contract C5
