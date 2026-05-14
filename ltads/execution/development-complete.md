# Development Complete — P1 Stage 1 (Foundation Contracts + PII Oracle)

**Session**: S001 (continued PAUSED; no new session started)
**Stage**: P1 Stage 1 — Foundation Contracts + PII Oracle (Task Zero)
**Plan**: `docs/planning/plans/20260513T231115-feedbackr-p1-closes-the-loop.md`
**Completed**: 2026-05-13 (autopilot:continuous)
**Worker**: Orchestrated Execution worker
**Outcome**: All success criteria met. Both Verification Oracles GREEN.

## Tasks completed

| ID | Status | Notes |
|---|---|---|
| S1-T0 — `pii-scrub-audit` oracle | ✅ | Python 3.8+ canonical impl + bash shim + `manifest.json` (+ TOML mirror) + `expected_hash.txt` + `README.md`. PASS on Probe A + Probe B. |
| S1-T1 — `feedbackr-tracing` crate | ✅ | 20-pattern set ported byte-for-byte from GitCellar. `install_global_subscriber` wired in `bin/feedbackr-api/src/main.rs`. 41 unit tests + 7 integration tests. SHA-256 locked. |
| S1-T2 — Migrations 00003 + 00005 | ✅ | Both apply cleanly to a fresh dev DB (`sqlx migrate run` output captured below). Migration 00003 also adds the `feedback.status` column (documented deviation; see Deviations). |
| S1-T3 — Repository extensions | ✅ | `FeedbackRepo::list_for_admin` + `get_with_history`. New `FeedbackStatusHistoryRepo` module. `TenantRepo` brand surface (`get_brand` + `update_brand` + `EmailTenantBrand`). All scope-discipline clean (multi-tenant-isolation-check oracle STILL GREEN). |
| S1-T4 — Frozen contracts handoff doc | ✅ | `docs/planning/handoffs/p1-stage1-to-stage2.md` — Contracts C6 / C7 / C8 / C9 / C10 / C11 verbatim + TypeScript type mirror + pre-authorized self-mediation widenings. |

## Files created

```
.claude/oracles/pii-scrub-audit/
  ├── README.md
  ├── expected_hash.txt          (SHA-256: bf1355b982a56848789412e4f273f4f8f77ce83c47fccf8de22c5111ccd430e3)
  ├── manifest.json
  ├── manifest.toml              (TOML mirror; runtime uses manifest.json)
  ├── oracle.py
  └── oracle.sh

crates/feedbackr-tracing/
  ├── Cargo.toml
  ├── README.md                  (ULADP module standard: Synopsis, File Index, Public API, Constraints, Decision Log)
  ├── src/
  │   ├── lib.rs                 (install_global_subscriber, LogLevel, LogFormat, TracingError)
  │   ├── layer.rs               (ScrubbingMakeWriter — StdoutScrubbing prod + SharedBufferScrubbing test)
  │   └── scrubber.rs            (CANONICAL_PATTERNS, scrub(), canonical_serialised(), 28 unit tests)
  └── tests/
      └── scrubber_patterns.rs   (7 end-to-end integration tests + bilateral SHA-256 check)

crates/feedbackr-core/src/
  └── status.rs                  (FeedbackStatus enum, legal_transitions_from, TransitionError; 7 unit tests)

crates/feedbackr-repository/src/
  └── feedback_status_history.rs (FeedbackStatusHistoryRepo trait + impl; 4 sqlx::test cases)

migrations/
  ├── 00003_feedback_status_history.sql  (feedback.status column + feedback_status_history table)
  └── 00005_tenant_email_brand.sql       (brand_name/email_subject_prefix/support_email/unsubscribe_url/footer_signature)

docs/planning/handoffs/
  └── p1-stage1-to-stage2.md     (Contracts C6/C7/C8/C9/C10/C11 frozen verbatim + TS type mirror)
```

## Files modified

```
Cargo.toml                                  (add feedbackr-tracing member; regex + once_cell workspace deps)
crates/feedbackr-api/Cargo.toml             (add feedbackr-tracing dep)
crates/feedbackr-api/src/main.rs            (replace inline tracing_subscriber::fmt() with install_global_subscriber)
crates/feedbackr-core/src/lib.rs            (pub mod status; re-exports)
crates/feedbackr-core/src/models.rs         (Feedback gains `status: FeedbackStatus` field)
crates/feedbackr-repository/src/lib.rs      (re-export feedback_status_history + EmailTenantBrand + FeedbackListItem + StatusHistoryRow)
crates/feedbackr-repository/src/feedback.rs (add list_for_admin + get_with_history + FeedbackListItem + StatusHistoryRow; new tests)
crates/feedbackr-repository/src/tenants.rs  (add get_brand + update_brand + EmailTenantBrand; extend `create` to populate brand defaults; new tests)
crates/feedbackr-repository/README.md       (note the P1 Stage 1 additions in File Index)
.claude/oracles/multi-tenant-isolation-check/allowlist.toml  (add SqlxFeedbackStatusHistoryRepo::new + EmailTenantBrand::from_db constructor entries)
.sqlx/                                      (regenerated via `cargo sqlx prepare --workspace`)
```

## Test counts (cargo test --workspace, DATABASE_URL set to fresh migrated DB)

| Crate / target | Tests | Result |
|---|---|---|
| feedbackr-anon                            | 11 | ✅ all pass |
| feedbackr-api (unit + lib)                | 40 | ✅ all pass |
| feedbackr-api (binary)                    |  0 | (no tests in the binary itself) |
| feedbackr-api integration `tests/handlers`| 13 | ✅ all pass |
| feedbackr-core                            | 13 | ✅ all pass (6 existing + 7 new status tests) |
| feedbackr-jwt                             |  3 | ✅ all pass |
| feedbackr-jwt integration                 | 24 | ✅ all pass |
| feedbackr-repository                      | 33 | ✅ all pass (P0 + new admin/history/brand tests) |
| feedbackr-tracing unit                    | 41 | ✅ all pass |
| feedbackr-tracing integration             |  7 | ✅ all pass |
| **TOTAL**                                 |**185** | **✅ 0 failures** |

P0 reported 118 tests; Stage 1 added ~67 (status enum + repo extensions + tracing crate + history repo + brand surface).

## Command output

### cargo test --workspace (summary)

```
running 11 tests
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.20s
running 40 tests
test result: ok. 40 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.20s
running 13 tests
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 21.31s
running 13 tests
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
running 3 tests
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
running 24 tests
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
running 33 tests
test result: ok. 33 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 17.73s
running 41 tests
test result: ok. 41 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
running 7 tests
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
```

(All other crates' Doc-tests + binary tests report 0/0, omitted for brevity.)

### cargo clippy --workspace --all-targets -- -D warnings

```
    Checking feedbackr-repository v0.1.0 (E:\Developer\SourceControlled\Apps\Feedbackr\crates\feedbackr-repository)
    Checking feedbackr-api v0.1.0 (E:\Developer\SourceControlled\Apps\Feedbackr\crates\feedbackr-api)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.07s
```

(Zero clippy warnings across the workspace; one near-miss `explicit_auto_deref` was caught during the develop loop and fixed in `crates/feedbackr-tracing/src/scrubber.rs`.)

### python .claude/oracles/pii-scrub-audit/oracle.py

```
PASS pii-scrub-audit
  Probe A (no tracing setup outside crates/feedbackr-tracing/): clean
  Probe B (CANONICAL_PATTERNS hash matches expected_hash.txt): clean
```

### bash .claude/oracles/multi-tenant-isolation-check/oracle.sh (regression)

```
PASS multi-tenant-isolation-check
  Probe A (raw SQL outside repository): clean
  Probe B (repository-method scope discipline): clean
```

### sqlx migrate run (fresh DB)

```
Applied 1/migrate p0 schema (116.817ms)
Applied 2/migrate email verifications (78.6366ms)
Applied 3/migrate feedback status history (60.6415ms)
Applied 5/migrate tenant email brand (7.5651ms)
```

## SHA-256

```
.claude/oracles/pii-scrub-audit/expected_hash.txt
  → bf1355b982a56848789412e4f273f4f8f77ce83c47fccf8de22c5111ccd430e3
```

Hash is over the line-serialised `name\tregex\treplacement\n` rows of
`CANONICAL_PATTERNS` (20 patterns × 1 row each = 20 newline-terminated
records). Bilateral check in `crates/feedbackr-tracing/tests/scrubber_patterns.rs::canonical_hash_matches_expected_file`.

## Deviations from the brief (with rationale)

1. **`feedback.status` column added in migration 00003.**
   The brief enumerated only the `feedback_status_history` table for 00003,
   but the C6 backing methods (`list_for_admin` status_filter,
   `get_with_history` returning `Feedback { status, .. }`) require the
   column to exist. Adding it in the same migration keeps the
   "status workflow" feature cohesive and removes a forced Stage 2 widening.
   The column has a CHECK constraint on the six canonical kebab-case
   values from Contract C6.

2. **`CANONICAL_PATTERNS` is a 3-tuple `(name, regex, replacement)`.**
   The brief's illustrative shape was `(&str, &str)`. GitCellar's source
   stores the name only in comments. Promoting `name` to a slice field
   lets the `pii-scrub-audit` oracle name offenders and detects re-orderings
   in the SHA-256 even if regex+replacement are unchanged. The byte-for-byte
   port load-bearing invariant (regex strings + replacement strings
   match GitCellar exactly) is preserved.

3. **PII scrubbing at the WRITE boundary, not a `Layer` impl.**
   The brief calls for an `impl Layer<...> for ...` applying scrub to event
   field values. We chokepoint at the formatted-byte boundary via
   `ScrubbingMakeWriter` so EVERY emitted line passes through scrub
   (including JSON-encoded field values, level prefixes, span metadata),
   not just specific string fields. Same end-user property; more stable
   seam. `pii-scrub-audit` Probe A still forbids `impl Layer<...> for ...`
   outside the crate as a defense-in-depth.

4. **Both `manifest.json` and `manifest.toml` shipped.**
   The brief specified `manifest.toml`; the existing
   `multi-tenant-isolation-check` oracle uses `manifest.json` (the file
   the harness reads). Both files exist; `manifest.json` is the runtime
   metadata; `manifest.toml` is a literal-brief-compliance mirror.

5. **`pii-scrub-audit` Probe A regex tightened.**
   The brief's `impl.*Layer.*for` would false-positive on
   `TraceLayer::new_for_http()` in tower-http usage. Tightened to
   `\bimpl\b[^;{]*\bLayer\s*<[^>]*>\s+for\b` which requires
   `impl ... Layer<...> for ...` block-opener syntax.

6. **Probe B parser does NOT strip line comments.**
   The DSN regex (`https?://...`) contains `//`. A naive line-comment
   stripper would mangle it. Kept comment-stripping for Probe A (line
   patterns + multi-line `/* */` blocks); skipped it for Probe B's
   pattern-set extraction.

7. **TenantRepo brand surface uses `get_brand` (NOT widening of `find_by_email`).**
   Choice spelled out in the handoff doc + the source comments.
   `find_by_email` is pre-auth allow-listed; widening it with brand columns
   would unnecessarily widen the pre-auth surface. `get_brand` takes
   `&TenantScope`, keeping the rest of the post-auth surface uniform.

8. **`TenantRepo::create` extended to populate brand defaults.**
   Migration 00005's NOT NULL brand columns would otherwise reject every
   new signup. Default values mirror migration 00005's backfill logic
   exactly (brand_name = email local-part; support_email = full email;
   footer_signature = `"— The {local-part} team"`).

9. **Cookie name documented as `feedbackr_session`, not `feedbackr_admin_session`.**
   The plan's Contract C11 wording referenced `feedbackr_admin_session`;
   the actual P0 cookie is `feedbackr_session` (verified in
   `crates/feedbackr-api/src/auth/session.rs::SESSION_COOKIE_NAME`).
   Handoff doc records the actual value.

## Carry-forward notes for Stage 2

### Worker A (backend — Status workflow + Status emails)

- **Migration numbering**: 00004 is RESERVED for `feedback_replies`.
- **`transitioned_by` UUID has no FK** to `tenant_users` (the table doesn't exist
  yet). Add the FK in a follow-up migration once `tenant_users` lands; use
  `ADD CONSTRAINT … NOT VALID; VALIDATE CONSTRAINT …` for online safety.
- **Atomic audit-row insert**: Stage 1's
  `FeedbackStatusHistoryRepo::append` operates against `&self.pool` (no
  transaction). For Contract C6 Hard Invariant #4 (atomic same-transaction
  insert of status column UPDATE + history row), add an Executor-aware
  overload — this is pre-authorized per PODS Coordination Protocol.
- **State-machine consumer**: `feedbackr_core::legal_transitions_from`
  + `TransitionError` are frozen; reuse, don't reimplement.
- **Brand parameters**: load via `TenantRepo::get_brand(&scope)` in the
  email-template renderer. `EmailTenantBrand::from_db` is the constructor;
  `sender_display_name` is COMPUTED (`{brand_name} via Feedbackr`) — never
  stored in a column.
- **Display ID**: Stage 1 returns `short_code` (the alphanumeric
  `FB-XXXXXX` form from P0). The P1 plan's Deferred Decisions chose
  `FB-NNNNNN` numeric sequential — if Worker A switches, mirror the change
  in `admin-ui/src/shared/types.gen.ts`.

### Worker B (frontend — Admin UI)

- **TypeScript type-mirror** is embedded in the handoff doc; copy it into
  `admin-ui/src/shared/types.gen.ts`.
- **Cookie name**: `feedbackr_session` (NOT `feedbackr_admin_session`).
- **Vite port**: 14204 with `strictPort: true` (claimed in
  `~/.claude/MACHINE_CONFIG.md` Dev Port Registry).
- **Backend port**: 14304 (proxy `/api` → `http://localhost:14304`).
- **Status workflow UI invariant**: only render buttons for
  `LEGAL_TRANSITIONS[currentStatus]` — illegal transitions are never
  shown.
- **Body rendering**: plain-text only (`textContent`/React default
  escaping); never `dangerouslySetInnerHTML`. Defends against stored-XSS
  from submitter content.

### Both workers

- **`install_global_subscriber` is the SOLE tracing setup**. Tests should
  use per-test subscribers (or `SharedBufferScrubbing` for PII-assertion
  tests); never call `install_global_subscriber` from a test, it'll
  conflict with the integration-test runner.
- **`pii-scrub-audit` is a CI gate**. Pattern-set changes require
  refreshing `expected_hash.txt` in a deliberate commit.
- **Pre-authorized widenings** (handoff doc §Pre-authorized self-mediation
  widenings): `list_for_admin` optional filter parameters, `EmailTenantBrand`
  additional `Option<String>` fields, `FeedbackListItem` additional read-only
  fields, `FeedbackStatusHistoryRepo::append` executor-aware overload.

## Open issues / known limitations

- **`scripts/e2e-p0-curl.sh` regression run** is OUT OF SCOPE for Stage 1
  per brief (orchestrator owns at arc level). All Rust integration tests
  pass; the only main.rs change is the tracing-init swap which is
  exercised by `tests/handlers.rs::*` (router-bootstrap + signup +
  verify-email all pass).
- **sqlx offline cache regenerated**. `.sqlx/` was refreshed via
  `cargo sqlx prepare --workspace`. Commit `.sqlx/` alongside the source
  changes (orchestrator owns the commit).
- **Test DB**: created at `postgres://postgres:dev@localhost:5433/feedbackr_p1_s1`
  for Stage 1 verification. The shared `feedbackr_dev` DB is in a stale
  state (no `_sqlx_migrations` entries); orchestrator may want to migrate
  it or drop-and-recreate before Stage 2 work.
- **`docs/specs/SPECIFICATION.md`**: not modified. The new Contracts
  (C6/C7/C8/C9/C10) live in the handoff doc; orchestrator may want to
  reconcile to spec via `/0-uldf-conform-specs` at arc close.

## Compliance with brief's Completion Protocol

- ✅ Completion report written to `ltads/execution/development-complete.md` (this file).
- ✅ Test counts + command output captured.
- ✅ Oracle PASS evidence (both oracles).
- ✅ Deviations documented with rationale.
- ⏭ Will EXIT this session (close terminal) after writing this file.
- ⏭ Will NOT run `/0-uldf-ltads-stop` / `/0-uldf-finalize` / `git commit`
  (orchestrator owns).
- ⏭ Did NOT modify `ltads/execution/spec-progress.md`, `commit-log.md`,
  `task-queue.md`, or `ltads/sessions/current-session.md` (orchestrator owns).
