# Development Brief — Feedbackr P1 Stage 1

## ⚠️ Completion Protocol (READ FIRST)

When ALL Stage 1 exit criteria are met:
1. Write completion report to `ltads/execution/development-complete.md` (tasks done,
   files created/modified, test counts, oracle PASS evidence, any deviations or issues)
2. Include actual command output for: `cargo test --workspace`,
   `cargo clippy --workspace --all-targets -- -D warnings`,
   `python .claude/oracles/pii-scrub-audit/oracle.py`,
   `bash .claude/oracles/multi-tenant-isolation-check/oracle.sh`
3. **EXIT this session** (close the terminal). Do NOT run `/0-uldf-ltads-stop`,
   `/0-uldf-finalize`, or `git commit` — orchestrator owns those at the arc level.
4. Do NOT modify LTADS tracking files (spec-progress.md, commit-log.md,
   current-session.md, task-queue.md) — orchestrator owns those.

## Session
- **ID**: S001 (continuing PAUSED session — do NOT start new session)
- **Generated**: 2026-05-13T23:25:03Z
- **Strategy**: Orchestrated Execution (Tier 1)
- **Stage**: P1 Stage 1 — Foundation Contracts + PII Oracle (Task Zero)
- **Plan**: `docs/planning/plans/20260513T231115-feedbackr-p1-closes-the-loop.md`
- **Autonomy**: autopilot:continuous (inherited via `.claude/session-state/task-arc-autonomy.json`; BoundConsent valid until 2026-05-14T21:06:21Z)

## Mission

Freeze the contracts (C6/C7/C8/C9/C10/C11) that Stage 2's PODS fan-out (Worker A
backend + Worker B frontend) will consume as a frozen library surface. Build the
`pii-scrub-audit` Verification Oracle as Task Zero so log-drift is policed from
commit 1. Port the byte-for-byte canonical 20-pattern PII scrubber into a new
`crates/feedbackr-tracing/` crate. Land migrations 00003 (status history) and
00005 (tenant email-brand). Extend repository surfaces additively
(`FeedbackRepo::list_for_admin` + `get_with_history`; new `FeedbackStatusHistoryRepo`;
`TenantRepo` brand-parameter surface) while keeping the
`multi-tenant-isolation-check` oracle GREEN.

This is the contract-freeze foundation. Stage 2 workers (in separate sessions, not
your concern) will consume your output as a frozen library surface. **Do NOT
implement Stage 2 scope** — no status-transition handler logic, no email templates,
no admin UI, no reply endpoints. Stage 1 = contracts + oracle + scrubber + migrations
+ repo extensions ONLY.

## Context Files (read in this order)

1. `docs/planning/plans/20260513T231115-feedbackr-p1-closes-the-loop.md` —
   **AUTHORITATIVE source of truth for Stage 1 scope, exit criteria, and the
   carry-state handoff doc location.** Read §Strategy Rationale, §Component
   Decomposition (Stage 1 only), §Interface Contracts (C6, C7, C8, C9, C10, C11),
   §Oracle Pre-Build Plan, §Risks and Mitigations.
2. `docs/specs/SPECIFICATION.md` § FR-FBR-07, FR-FBR-08, FR-FBR-09, FR-FBR-10
3. `docs/specs/DECISIONS.md` § DEC-FBR-04, DEC-FBR-IMPL-01..04, DEC-FBR-IMPL-03
   (Python-canonical oracle pattern)
4. `docs/specs/DISCOVERIES.md` § D-FBR-02 (three-leg defense pattern),
   D-FBR-05..09 (carry-state from P0)
5. `docs/planning/plans/20260513T210133-feedbackr-p0-foundation.md` — structural
   template for migrations, contracts, repo extensions, oracle patterns
6. `.claude/oracles/multi-tenant-isolation-check/` — pattern your new
   `pii-scrub-audit` oracle on this one (manifest.toml + oracle.py + bash shim
   + README.md structure)
7. Existing P0 code (read-only orientation):
   - `crates/feedbackr-repository/src/feedback.rs` — pattern for new methods
   - `crates/feedbackr-repository/src/tenants.rs` — pattern for brand extensions
   - `crates/feedbackr-repository/src/scope.rs` — ProjectScope / TenantScope contracts
   - `crates/feedbackr-api/src/main.rs` (or `bin/feedbackr-api/src/main.rs` —
     check the actual location) — existing inline `tracing_subscriber::fmt()` to replace
   - `migrations/00001_p0_schema.sql`, `migrations/00002_email_verifications.sql` —
     migration style
8. GitCellar reference reads (read-only per DEC-FBR-07):
   - `E:/Developer/SourceControlled/Apps/GitCellar/gitcellar-service/src/feedback_logs/scrubber.rs` — canonical 20-pattern set
     (port byte-for-byte)
   - `E:/Developer/SourceControlled/Apps/GitCellar/gitcellar-cloud/src/feedback/db.rs` — status state-machine reference
   - `E:/Developer/SourceControlled/Apps/GitCellar/gitcellar-cloud/src/feedback/email_templates.rs` — template parameterization
     reference (informs C10; do NOT implement templates — Stage 2 Worker A does)
9. `CLAUDE.md` (project root) — privacy invariants, dev port 14204 reservation,
   AGPL stub status

## Tasks

| ID | Description | Files | Priority |
|----|-------------|-------|----------|
| S1-T0 | pii-scrub-audit Verification Oracle (kind: verification; freshness: trigger-invalidate on changes to crates/feedbackr-tracing/ or migrations/). Probe A: grep AST over all crates for `tracing_subscriber::fmt()` / `tracing_subscriber::registry()` / custom Layer impl OUTSIDE `crates/feedbackr-tracing/` → FAIL with file:line. Probe B: SHA-256 of CANONICAL_PATTERNS slice matches `expected_hash.txt` → FAIL on mismatch. Python 3.8+ canonical impl + bash shim. Output machine-parseable PASS/FAIL. | `.claude/oracles/pii-scrub-audit/oracle.py`, `oracle.sh`, `manifest.toml`, `expected_hash.txt`, `README.md` | P0 |
| S1-T1 | New crate `crates/feedbackr-tracing/`. Port canonical 20-pattern set byte-for-byte from `gitcellar-service/src/feedback_logs/scrubber.rs` into `CANONICAL_PATTERNS: &[(&str, &str)]`. Public `scrub(input: &str) -> String` (idempotent). Public `install_global_subscriber(level: LogLevel, format: LogFormat) -> Result<(), TracingError>` chokepoint. Internal `tracing_subscriber::Layer` impl applying scrubber to every emitted event's string fields. Pattern-by-pattern unit tests + 4-5 integration tests through real tracing emission + idempotence test. Replace inline `tracing_subscriber::fmt()` in `bin/feedbackr-api/src/main.rs` (or wherever P0 Stage 3 placed it). Compute SHA-256 of CANONICAL_PATTERNS and write to `expected_hash.txt`. | `crates/feedbackr-tracing/Cargo.toml`, `src/lib.rs`, `src/scrubber.rs`, `src/layer.rs`, `tests/scrubber_patterns.rs`, `README.md`; workspace `Cargo.toml`; `crates/feedbackr-api/Cargo.toml` (add dep); `crates/feedbackr-api/src/main.rs` (or `bin/feedbackr-api/src/main.rs`) | P0 |
| S1-T2 | Schema migrations. 00003: `feedback_status_history` table (id UUID PK, feedback_id FK → feedback.id, from_status TEXT, to_status TEXT, reason_note TEXT nullable, duplicate_of_feedback_id UUID nullable FK → feedback.id, transitioned_by UUID FK → tenant_users.id, transitioned_at TIMESTAMPTZ default now()) + index on `(feedback_id, transitioned_at DESC)`. 00005: ALTER TABLE tenants ADD COLUMNs `brand_name`, `email_subject_prefix`, `support_email`, `unsubscribe_url` (nullable), `footer_signature` — backfill non-null columns from existing `tenants.email` row. Both migrations idempotent re-run safe per sqlx migrator semantics. | `migrations/00003_feedback_status_history.sql`, `migrations/00005_tenant_email_brand.sql` | P0 |
| S1-T3 | Repository surface extensions (additive). `FeedbackRepo::list_for_admin(&self, scope: &ProjectScope, status_filter: Option<FeedbackStatus>, limit: u32, offset: u32) -> Result<(Vec<FeedbackListItem>, u32 /* total */)>`. `FeedbackRepo::get_with_history(&self, scope: &ProjectScope, feedback_id: FeedbackId) -> Result<(Feedback, Vec<StatusHistoryRow>)>`. New module `feedback_status_history.rs` with `FeedbackStatusHistoryRepo` trait + `SqlxFeedbackStatusHistoryRepo` impl (methods: `append`, `list_for_feedback` — both `&ProjectScope` first). `TenantRepo` brand-parameter surface (`update_brand` + brand fields exposed via existing `find_by_email` widening OR new `get_brand` — pick one and document choice). EVERY method takes `&ProjectScope` (or `&TenantScope` for TenantRepo) FIRST. sqlx::test cross-tenant negative tests (attempting to read from another tenant's scope MUST 0-row return, not error). | `crates/feedbackr-repository/src/feedback.rs`, `feedback_status_history.rs` (NEW), `tenants.rs`, `lib.rs` (re-exports) | P0 |
| S1-T4 | Frozen contracts handoff doc: copy Contracts C6, C7, C8, C9, C10, C11 from the P1 plan VERBATIM into `docs/planning/handoffs/p1-stage1-to-stage2.md` (note: distinct from the existing `stage1-to-stage2.md` which is P0-era; use the P1 suffix). Also include: a hand-rolled TypeScript type-mirror file `admin-ui/src/shared/types.gen.ts.example` (do NOT create the admin-ui/ directory — just include the file content in the handoff doc as a code block for Worker B to copy in Stage 2). Document the P0 admin-session-cookie shape (Contract C11) by reading `crates/feedbackr-api/src/auth/` and transcribing the cookie format verbatim. Document the pre-authorized self-mediation widenings (PODS Coordination Protocol § Pre-authorized widenings) explicitly. | `docs/planning/handoffs/p1-stage1-to-stage2.md` (NEW) | P0 |

## Patterns

### Pattern 1: Verification Oracle (port from multi-tenant-isolation-check)

```python
# .claude/oracles/pii-scrub-audit/oracle.py
# Pattern: read manifest.toml for kind/freshness/probes; emit machine-parseable
# JSON to stdout + exit 0 (PASS) or 1 (FAIL with offending file:line list).
# Probe A (AST/grep): walk repo; flag any call site matching
# r"tracing_subscriber::fmt\(\)|tracing_subscriber::registry\(\)|impl.*Layer.*for"
# OUTSIDE crates/feedbackr-tracing/.
# Probe B (hash): import CANONICAL_PATTERNS via simple regex extraction from
# crates/feedbackr-tracing/src/scrubber.rs; compute sha256; compare to
# expected_hash.txt; FAIL on mismatch.
```

### Pattern 2: Tenant-Scoped Repository Method (existing in P0)

```rust
// crates/feedbackr-repository/src/feedback.rs (existing pattern)
pub async fn list_for_admin(
    &self,
    scope: &ProjectScope,  // <- &ProjectScope FIRST. always. non-negotiable.
    status_filter: Option<FeedbackStatus>,
    limit: u32,
    offset: u32,
) -> Result<(Vec<FeedbackListItem>, u32), RepoError> {
    let rows = sqlx::query_as!(
        FeedbackListItem,
        r#"SELECT ... FROM feedback
           WHERE tenant_id = $1 AND project_id = $2
             AND ($3::feedback_status IS NULL OR status = $3)
           ORDER BY submitted_at DESC LIMIT $4 OFFSET $5"#,
        scope.tenant_id(), scope.project_id(), status_filter, limit as i64, offset as i64,
    ).fetch_all(&self.pool).await?;
    // ... count query for total ...
    Ok((rows, total))
}
```

### Pattern 3: tracing_subscriber Layer chokepoint

```rust
// crates/feedbackr-tracing/src/lib.rs
pub fn install_global_subscriber(level: LogLevel, format: LogFormat)
    -> Result<(), TracingError>
{
    let scrubbing_layer = ScrubbingLayer::new();
    let formatting_layer = match format {
        LogFormat::Json => tracing_subscriber::fmt::layer().json().boxed(),
        LogFormat::Plain => tracing_subscriber::fmt::layer().boxed(),
    };
    tracing_subscriber::registry()
        .with(tracing_subscriber::filter::LevelFilter::from_level(level.into()))
        .with(scrubbing_layer)         // applies scrub() to all string event fields
        .with(formatting_layer)
        .try_init()
        .map_err(TracingError::Init)
}
```

## Testing Context

> **Testing Required**: YES (for S1-T0, S1-T1, S1-T3)
> **Test Commands**:
> - `cargo test --workspace` (DATABASE_URL must be set for sqlx::test)
> - `cargo clippy --workspace --all-targets -- -D warnings`
> - `python .claude/oracles/pii-scrub-audit/oracle.py` (after S1-T1 lands)
> - `bash .claude/oracles/multi-tenant-isolation-check/oracle.sh` (regression check)
> **Test Framework**: cargo test + sqlx::test (DB-backed) + Python 3.8+ for oracle

### Testing Guidance

- **S1-T0 (oracle)**: self-validating; PASS criterion is the oracle returning exit
  0 against your own freshly-written scrubber crate.
- **S1-T1 (scrubber)**: each of the 20 canonical patterns needs at least one
  positive-match unit test (input contains pattern -> output contains
  `[REDACTED:{pattern_name}]`) and one near-miss-no-match test (input looks
  similar but isn't the pattern -> output unchanged). 4-5 integration tests
  install the scrubber via `install_global_subscriber`, emit a `tracing::info!`
  with PII, capture the emitted line, and assert PII is scrubbed. Idempotence:
  `assert_eq!(scrub(&scrub(input)), scrub(input))`.
- **S1-T3 (repo extensions)**: every new method gets a cross-tenant negative test
  (Tenant A creates feedback X; query from Tenant B's ProjectScope returns 0 rows,
  NOT an error). This keeps `multi-tenant-isolation-check` oracle GREEN.
- **Regression**: existing 118 tests stay GREEN. `scripts/e2e-p0-curl.sh` 7/7
  still passes after main.rs tracing-setup replacement.

## Success Criteria

- [ ] `.claude/oracles/pii-scrub-audit/` exists with `oracle.py` (Python 3.8+), bash
      shim `oracle.sh`, `manifest.toml`, `expected_hash.txt`, `README.md`
- [ ] `pii-scrub-audit` oracle runs and returns PASS against the new scrubber crate
- [ ] `crates/feedbackr-tracing/` builds clean (`cargo build --workspace`)
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` GREEN
- [ ] 20 canonical PII patterns match byte-for-byte the GitCellar source (no drift)
- [ ] SHA-256 of CANONICAL_PATTERNS matches `expected_hash.txt`
- [ ] `migrations/00003_feedback_status_history.sql` applies cleanly to dev Postgres
- [ ] `migrations/00005_tenant_email_brand.sql` applies cleanly with sensible
      backfilled defaults for non-null columns
- [ ] `FeedbackRepo::list_for_admin` + `get_with_history` implemented with
      cross-tenant negative tests
- [ ] New `FeedbackStatusHistoryRepo` trait + `SqlxFeedbackStatusHistoryRepo` impl
- [ ] `TenantRepo` brand-parameter surface implemented per Contract C10
- [ ] `bin/feedbackr-api/src/main.rs` wired to `feedbackr_tracing::install_global_subscriber`
- [ ] `cargo test --workspace` passes (118 P0 tests + new Stage 1 tests; target ~130+)
- [ ] `multi-tenant-isolation-check` oracle STILL GREEN (Probe A + Probe B both clean)
- [ ] `scripts/e2e-p0-curl.sh` STILL PASSES 7/7 (P0 regression check)
- [ ] `docs/planning/handoffs/p1-stage1-to-stage2.md` exists with Contracts
      C6/C7/C8/C9/C10/C11 frozen verbatim + TypeScript type-mirror code block
- [ ] `crates/feedbackr-tracing/README.md` follows ULADP module standard
      (Synopsis, File Index, Public API, Constraints, Decision Log)
- [ ] `crates/feedbackr-repository/README.md` updated with new modules (if existing
      README index needs refresh)

## Hard Invariants (load-bearing — DO NOT relax)

1. **Pattern-set byte-for-byte port**: CANONICAL_PATTERNS in feedbackr-tracing
   must match GitCellar source byte-for-byte. Compute SHA-256 of the slice
   *after* finalization and write to expected_hash.txt. Any future drift surfaces
   via oracle FAIL.
2. **No `tracing_subscriber::fmt()` calls outside the scrubber crate**. Oracle
   Probe A enforces this. If P0 Stage 3 emitted any tracing setup outside the
   new chokepoint, REPLACE it (this is the entire point of the chokepoint).
3. **`&ProjectScope` (or `&TenantScope`) first argument on every repo method**.
   `multi-tenant-isolation-check` oracle enforces. Cross-tenant negative tests
   verify behaviorally.
4. **Audit row atomicity** (forward-looking — Stage 2 Worker A's concern but
   the schema supports it): no foreign-key or constraint design that would
   prevent atomic same-transaction insert with the eventual UPDATE of feedback.status.
5. **Stage 1 scope discipline**: NO status-transition handler implementation,
   NO email templates, NO admin UI, NO reply endpoints. If you find yourself
   writing handler logic, STOP — that's Stage 2.

## Completion Instructions

When ALL success criteria are met:

1. Write `ltads/execution/development-complete.md` with:
   - Tasks completed (S1-T0 through S1-T4)
   - Files created/modified (full list)
   - Test counts and command output (cargo test, clippy, both oracles, e2e regression)
   - SHA-256 hash recorded in expected_hash.txt
   - Any deviations from this brief with rationale
   - Carry-forward notes for Stage 2 workers (anything they should know that
     isn't in the handoff doc)
2. **EXIT this session** (close terminal)
3. **CRITICAL RULES**:
   - Do NOT run `/0-uldf-ltads-stop` (orchestrator does this at arc-level)
   - Do NOT run `git commit` (orchestrator does this)
   - Do NOT run `/0-uldf-finalize` (orchestrator does this)
   - Do NOT modify ltads/execution/spec-progress.md, commit-log.md,
     task-queue.md, or ltads/sessions/current-session.md (orchestrator owns these)
   - Implement, test, report to development-complete.md, exit
