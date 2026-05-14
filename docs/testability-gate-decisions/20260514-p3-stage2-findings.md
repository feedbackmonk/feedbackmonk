# Testability Gate Findings — P3 Stage 2 (admin UI tier settings)

**Date**: 2026-05-14
**Phase**: P3 Stage 2 (admin UI tier settings + cap-aware error rendering)
**Plan**: `docs/planning/plans/20260514T134816-feedbackmonk-p3-commercial-gate.md`
**Commit**: P3 close (this commit)

## Why this artifact exists

The active plan's `## Testability Gate Findings` section flagged two composite surfaces during P3 planning:

| Finding | Composite | Pairing oracle |
|---|---|---|
| **FR-FBR-14 (Tier enforcement)** | 16/25 | `tier-enforcement-status` (Probe A handler coverage AST + Probe B Contract-C19 shape + Probe C `--full` integration smoke trio) |
| **`tier-enforcement-status` oracle itself** | 15/25 | self-validating via Probe A allowlist + Probe B canonical-shape vs config + Probe C smoke trio (the oracle IS its own scaffolding) |

Both findings called for **Verification Oracle scaffolding** to land at Stage 1 Task Zero so every Stage 1+2 commit closes the develop/test/fix loop against deterministic checks rather than human attention.

## Implementation evidence (verified at finalize)

The Stage 1 commit `d2266ae` shipped `.claude/oracles/tier-enforcement-status/` (Python canonical + ps1/sh shims) with all three probes wired:

| Probe | Status | Evidence |
|---|---|---|
| **A — handler coverage AST** | active-PASS | `oracle.py` parses every `crates/feedbackmonk-api/src/handlers/*.rs` writer and asserts each either calls `check_tier_quota` or appears in `allowlist.toml`. Re-run at Stage 2 finalize: PASS clean. |
| **B — Contract C19 shape** | active-PASS | Asserts `tier_quotas()` returns the canonical four-tier table (`Free | Starter | Pro | SelfHost`) with the canonical fields (projects_per_org, monthly_feedback_volume, custom_branding bool, custom_domain bool, eu_residency bool, footer_text Option). Re-run at Stage 2 finalize: PASS clean. |
| **C — integration smoke trio** | active-PASS (`--full`) | Three smoke tests (Free 2nd project → 409, Free 51st feedback → 402, widget-config footer flip Free→Some/Pro→None). Verified during Stage 1 finalize; not re-run at Stage 2 finalize because Stage 2 is admin UI only — no backend handler / `tier_quotas()` / widget-config touched. |

## Stage 2 pairing — frontend drift surface

Stage 2 adds a **Stage-2-side drift surface** that complements Probe B from the frontend angle:

- `admin-ui/src/pages/settings/__tests__/TierSettings.test.tsx`'s `tierStatus()` helper **inlines the four-tier Contract C19 quotas verbatim**. If Stage 1 rebases `tier_quotas()`, this fixture FAILS — same canonical values, asserted from React-render side.

Documented in detail at `docs/test-modifications/20260514-p3-stage2-new-test-suite.md` § "Stage-2-side drift surface". This is the active pairing the Testability Gate Findings called for: backend AST shape (Probe B) ↔ frontend render shape (TierSettings test fixture).

## Verdict

**Both findings CLOSED.** Oracle scaffolding is in place and active-PASS at every probe; the frontend pairing is wired; finalize Phase 11 revalidation re-runs the oracle clean.
