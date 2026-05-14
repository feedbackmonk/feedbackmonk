# Commit Log

_Append-only. Newest at bottom._

---

## dbbe04a -- 2026-05-13 -- P0 Stage 1

**Message**: `feat(p0): Stage 1 foundation -- multi-tenant data model + repository layer`

**Scope**: P0 Stage 1 (FR-FBR-01 + Task Zero oracle). Initial commit; 264 files added.

**Spec deltas**:
- FR-FBR-01 -> DONE (was NOT_STARTED).
- ARCHITECTURE.md component table populated with CMP-FBR-CORE-01, CMP-FBR-REPO-01, CMP-FBR-API-01, CMP-FBR-SCHEMA-01, CMP-FBR-ORACLE-01 (Stage 1 SHIPPED) and forward references for CMP-FBR-JWT-01, CMP-FBR-ANON-01 (Stage 2).
- DEC-FBR-IMPL-01..04 added to DECISIONS.md (Contract C1 extensions; scope_for allowlist; Python-canonical oracle pattern; dev-port deconfliction).

**Quality witnesses**:
- `cargo build --workspace --all-targets`: GREEN
- `cargo clippy --workspace --all-targets -- -D warnings`: GREEN
- `cargo test --workspace`: 19/19 pass (6 core + 13 repository) incl. cross-tenant invariants
- `python .claude/oracles/multi-tenant-isolation-check/oracle.py`: PASS

**Arc state**: Mid-arc Stage 1 -> Stage 2 boundary. NOT arc-terminus. CSI-06 wrote Mid-arc Checkpoint to `ltads/sessions/current-session.md`; Status remains ACTIVE; BoundConsent remains valid.

**Next**: `/0-uldf-proceed` -> likely PODS topology for Stage 2 fan-out (Worker A signup + Worker B submission path).

---

## b9a672a -- 2026-05-13 -- P0 Foundation CLOSE (Stage 2 + Stage 3)

**Message**: `feat(p0): close P0 Foundation -- multi-tenant submission API + signup/onboarding + observability`

**Scope**: P0 Stage 2 (PODS convergence: Worker A signup/onboarding + Worker B submission path) + Stage 3 (FR-FBR-18 health + structured logging + critic C-001 fix + e2e P0-exit-gate witness). 53 entries staged (1 deletion, 39 additions, 13 modifications).

**Spec deltas**:
- FR-FBR-02 -> DONE (was PROPOSED). Customer signup + onboarding live.
- FR-FBR-03 -> DONE (was PROPOSED). Submission API live with JWT + anonymous dispatch.
- FR-FBR-05 -> DONE (was PROPOSED). JWT EdDSA verifier with 24-test fixture corpus.
- FR-FBR-06 -> DONE (was PROPOSED). Anonymous-mode governor + BLAKE3 hash + cookie dedup.
- FR-FBR-18 -> DONE (was PROPOSED). /health + /health/ready + tracing JSON + request-id.
- SPECIFICATION.md, spec-progress.md, PROJECT_TRAJECTORY.md, DISCOVERIES.md all reconciled.
- DEC-PODS-001 + DEC-PODS-002 extracted to `crates/feedbackr-repository/README.md` Decision Log.

**Quality witnesses**:
- `cargo build --workspace`: GREEN
- `cargo clippy --workspace --all-targets -- -D warnings`: GREEN
- `cargo test --workspace` (DATABASE_URL set): 118/118 pass (11 anon + 40 api-unit + 13 api-integration + 6 core + 3 jwt-lib + 24 jwt-corpus + 21 repository)
- `.claude/oracles/multi-tenant-isolation-check/oracle.sh`: PASS (Probe A + Probe B both clean)
- `scripts/e2e-p0-curl.sh` P0-exit-gate witness: PASS 7/7 end-to-end against live binary on :14304 + Postgres :5433 + Mailpit :1025/:8025 (LD-verified pre-convergence; not re-run in this finalize)

**Arc state**: Mid-arc P0 -> P1 phase boundary. **NOT arc-terminus**. autopilot:continuous arc grant in `.claude/session-state/task-arc-autonomy.json` remains valid until 2026-05-14T21:06:21Z and carries onto P1. CSI-06 three-signal detection: mid-arc (no `--complete-arc` flag; no chain-endpoint signal; not final-stage of arc). Wrote Mid-arc Checkpoint to `ltads/sessions/current-session.md`; Status remains PAUSED (awaiting next session resume); BoundConsent remains valid.

**Next**: User action — replace `LICENSE` stub with full AGPL-3.0 text + finalize GitHub org + domain registration (pre-public-commit gate, orthogonal to P1 implementation). Then `/0-uldf-proceed` at P0->P1 boundary; likely HANDOFF topology to fresh `/0-uldf-ldis-plan "Feedbackr P1 -- Closes the Loop"`.

---

## (pending) -- 2026-05-14 -- P3 Stage 1 CLOSE (backend commercial-gate)

**Message**: `feat(p3-s1): close P3 Stage 1 -- backend tier model + caps + tier-enforcement-status oracle (Polar deferred)`

**Scope**: P3 Stage 1 backend (orchestrator HANDOFF to single worker; STAGED strategy per plan). 14 worker tasks GREEN. Stage 2 admin UI ahead via `/0-uldf-proceed` at this commit's tail.

**Spec deltas**:
- FR-FBR-14 -> DONE (S1 backend) (was PROPOSED). Tier enforcement: backend tier model + cap-firing predicate + free-tier footer + admin tier-status endpoint + Verification Oracle. Stage 2 ships admin UI surface.
- FR-FBR-15 -> DEFERRED (was PROPOSED) per DEC-FBR-DEFER-01. Stub at `docs/deferred/polar-integration.md`; operator promotion via SQL helper at `docs/operations/TIER_OVERRIDE.md` until Polar lands.
- DEC-FBR-DEFER-01 added to `docs/specs/DECISIONS.md` (Polar deferral with rationale, trade-offs, implementation pointer).
- D-FBR-19 / D-FBR-20 / D-FBR-21 added to `docs/specs/DISCOVERIES.md` (defense-in-depth pairing pattern; eager Probe C smoke-test pattern; test-mod artifact upfront-enumeration + cross-check pattern).
- Module Decision Log entries added: `feedbackmonk-core/README.md` (Tier enum as single source of truth), `feedbackmonk-repository/README.md` (`check_tier_quota` chokepoint discipline; `get_widget_brand` tier-aware footer enforcement).
- `tier-enforcement-status` Verification Oracle goes LIVE (fourth project oracle); CLAUDE.md table flipped from "scheduled" to "LIVE".

**Quality witnesses**:
- `cargo build --workspace`: GREEN (1m 34s)
- `cargo clippy --workspace --all-targets -- -D warnings`: GREEN (cached)
- `cargo test --workspace --no-fail-fast` (DATABASE_URL set): 302/302 pass (P2 closed at 271; +31 net-new — 11 tier-core + 6 tier_quota + 4 tenant tier extensions + 3 admin_tier + 3 smoke + 4 ApiError)
- `.claude/oracles/multi-tenant-isolation-check/oracle.sh`: PASS
- `.claude/oracles/pii-scrub-audit/oracle.sh`: PASS
- `.claude/oracles/widget-bundle-size/oracle.sh`: PASS (16,829B / 30,720B)
- `.claude/oracles/tier-enforcement-status/oracle.sh --full`: PASS (Probe A + Probe B + Probe C smoke-trio active-PASS)

**Arc state**: Mid-arc P3 Stage 1 -> Stage 2 boundary. **NOT arc-terminus**. autopilot:continuous BoundConsent remains valid (scope=open-ended, expires on `/0-uldf-ltads-stop` OR spec-exhaustion). CSI-06 three-signal detection: mid-arc (no `--complete-arc` flag; no chain-endpoint signal; not final-stage of arc). Mid-arc Checkpoint appended to `ltads/sessions/current-session.md`; Status remains ACTIVE.

**Push**: SKIPPED (--skip-push flag). PF-REGISTER-01 (github.com/feedbackmonk org + feedbackmonk.com purchase) gates first public push; not yet cleared.

**Next**: `/0-uldf-proceed` -> auto-spawn P3 Stage 2 (admin UI tier display + stub Upgrade button) per autopilot:continuous chain. Handoff brief: `docs/planning/handoffs/p3-stage1-to-stage2.md` (Contracts C17/C18/C19 frozen verbatim + TS starter kit).

---

## f4491d3 -- 2026-05-14 -- P4 Stage 2 (v1-arc-terminus)

**Message**: `feat(p4-s2): close P4 v1-arc-terminus -- marketing site + self-host docker compose (FR-FBR-16/17 DONE)`

**Scope**: P4 Stage 2 convergence commit. PODS collab-20260514-170323 (LD + CLAUDE-A + CLAUDE-B) converged via `/0-uldf-pods-converge --inline --finalize`. Two parallel workers shipped FR-FBR-16 (marketing site) + FR-FBR-17 (self-host docker compose) in one atomic v1-arc-terminus commit. Net-additive: two new top-level directories (`marketing/`, `deploy/docker/`), one new oracle (`.claude/oracles/selfhost-compose-smoke/`), one new pricing-export binary (`crates/feedbackmonk-core/examples/export_tier_quotas.rs`), one operator runbook (`docs/operations/SELFHOST.md`). One pre-existing file modified at code level: `crates/feedbackmonk-api/src/main.rs` (+16 lines for `FEEDBACKMONK_BIND_ADDR` per DEC-FBR-IMPL-07).

**Spec deltas**:
- FR-FBR-16 -> DONE (was PROPOSED) — Astro marketing site at `marketing/`, 7 pages, brand kit C20 applied, build-time pricing-parity SSOT, /docs/self-host content-mirror, Playwright + axe-core 11/11 PASS.
- FR-FBR-17 -> DONE (was PROPOSED) — `deploy/docker/` stack with api + admin-ui nginx edge + postgres + migrate one-shot + operator scripts (backup/restore); `selfhost-compose-smoke` Verification Oracle 3-probe active-PASS; `docker compose down -v && up -d --build --wait` GREEN with `/health/ready` 200 in <90s.
- DEC-FBR-IMPL-07 added to DECISIONS.md (FEEDBACKMONK_BIND_ADDR widening).
- D-FBR-25 (PricingCard SSOT-asymmetry post-Polar) + D-FBR-26 (`/docs/self-host` drift-risk; verification-oracle candidate `marketing-selfhost-page-parity` queued) added to DISCOVERIES.md.
- INDEX.md updated to include `selfhost-compose-smoke` oracle.
- v1 spec is content-complete: 17/18 FRs DONE, FR-FBR-15 (Polar) remains DEFERRED per DEC-FBR-DEFER-01.

**Quality witnesses**:
- Astro `npm run build`: GREEN (7 pages, clean)
- Playwright + axe-core (`marketing/tests/a11y.spec.ts`): 11/11 PASS, 0 a11y violations
- `docker compose down -v && up -d --build --wait`: GREEN end-to-end with `/health/ready` 200 in <90s
- `.claude/oracles/multi-tenant-isolation-check/oracle.sh`: PASS (no domain code changes)
- `.claude/oracles/pii-scrub-audit/oracle.sh`: PASS (no tracing changes)
- `.claude/oracles/widget-bundle-size/oracle.sh`: PASS (widget unchanged at 16,829B / 30,720B)
- `.claude/oracles/tier-enforcement-status/oracle.sh`: PASS (tier-stack unchanged; pricing export consumes same Contract C19)
- `.claude/oracles/selfhost-compose-smoke/oracle.sh --full`: PASS (Probe A yaml-lint clean; Probe B env-refs ⊆ C21 catalog; Probe C `/health/ready` 200 in <90s)
- Critic verdict at convergence: CONCERN (2 low-severity findings — D-FBR-25 SSOT-asymmetry pre-authorized per DEC-FBR-DEFER-01, D-FBR-26 drift-risk surfaced with named follow-up oracle; both within GUIDE §8 pre-authorized envelope)

**Arc state**: **ARC-TERMINUS**. P4 is the final phase of the v1 arc; Stage 2 is the final stage of P4. CSI-06 three-signal detection FIRES (final-stage-detection + chain-endpoint-metadata + this `/0-uldf-ltads-stop` invocation). BoundConsent expires per its `boundUntil=on /0-uldf-ltads-stop OR spec-exhaustion` clause — both conditions satisfied. `current-session.md` Status flips ACTIVE -> CONCLUDED; `Concluded-By: S002 at <ISO-8601> via /0-uldf-ltads-stop arc-terminus`. Autopilot:continuous cascades back to project default (collaborative).

**Push**: SKIPPED (--skip-push flag). PF-REGISTER-01 (github.com/feedbackmonk org + feedbackmonk.com purchase) gates first public push; not yet cleared.

**Next**: User-action only — no autonomous code work remains for v1. (1) PF-REGISTER-01 register org + buy domain; (2) PF-RENAME-02 working-dir rename `Apps\Feedbackr` -> `Apps\feedbackmonk`; (3) first public push once #1 clears; (4) post-launch: build `marketing-selfhost-page-parity` oracle (D-FBR-26), un-defer FR-FBR-15 Polar billing (DEC-FBR-DEFER-01 reversal addresses D-FBR-25).
