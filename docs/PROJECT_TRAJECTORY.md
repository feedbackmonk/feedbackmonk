# Project Trajectory — Feedbackr

Rolling high-level state. Auto-maintained by `/0-uldf-finalize` Phase 12. Cheap orientation for fresh sessions; for detail, go to `docs/specs/` and `docs/planning/`.

**Last updated**: 2026-05-13 (P1 Stage 1 mid-arc checkpoint — Foundation Contracts + PII Oracle DONE; Stage 2 PODS pending)

---

## Current Focus

**P1 Stage 2** (PODS, 2 workers — backend + admin UI) after Stage 1 mid-arc checkpoint commit.

Stage 2 spawn surface is the frozen handoff doc `docs/planning/handoffs/p1-stage1-to-stage2.md`, which contains Contracts C6/C7/C8/C9/C10/C11 verbatim + TypeScript type mirror + pre-authorized self-mediation widenings. Workers consume this as the library surface — no signature deviation without LD ratification.

- **Worker A (backend)**: status workflow transition handlers (FR-FBR-08), status emails plain-text rendering through PII-scrubbed paths (FR-FBR-09), audit-row atomicity contract (C7).
- **Worker B (frontend admin UI)**: React port from `gitcellar-cloud/admin-ui/` (FR-FBR-07), feedback list view + drawer detail + reply composer with public/internal visibility tabs + status transition controls. Binds dev-port 14204 with `strictPort: true`.

## Active Threads

- **P1 Stage 1 — DONE** (commit pending this finalize): `pii-scrub-audit` oracle built (Task Zero leg 2) + `feedbackr-tracing` crate shipped + migrations 00003 (`feedback.status` + audit history) + 00005 (tenant email brand) applied + repository surface extended (`FeedbackRepo::list_for_admin`/`get_with_history` + `FeedbackStatusHistoryRepo` + `TenantRepo::get_brand`/`update_brand`/`EmailTenantBrand`). 118 → 185 tests (+67). Both Verification Oracles GREEN.
- **P1 Stage 2 PODS pending**: spawn via `/0-uldf-pods-parallelize` consuming `docs/planning/handoffs/p1-stage1-to-stage2.md`.
- **P0 Foundation — COMPLETE** (closed at commit `b9a672a`): all 5 P0 FRs DONE; e2e P0-exit-gate witness PASS 7/7; 118 tests pass.
- **AGPL LICENSE pre-public-commit ratification gate — PENDING USER ACTION**: LICENSE file still a stub; repo stays local-only until user replaces with full AGPL-3.0 text + finalizes GitHub org + domain registration. P1 work continues local-only.
- **LTADS S001** — autopilot:continuous, mid-arc. Arc grant valid until 2026-05-14T21:06:21Z; continues into P1 Stage 2.
- **GitCellar widget-embed touchpoint — DEFERRED to late P2 / early P3**. No P1 cross-repo blockers.

## Recent Decisions

- **Contracts C6 / C7 / C8 / C9 / C10 / C11 frozen** (P1 Stage 1) — captured verbatim in `docs/planning/handoffs/p1-stage1-to-stage2.md` for Stage 2 fan-out. Spec-side reconciliation deferred to P1 arc-close `/0-uldf-conform-specs` pass; the handoff doc is itself durable.
- **DEC-FBR-IMPL-03 reapplied: Python-canonical oracle pattern** — `pii-scrub-audit` Probe B parses Rust `CANONICAL_PATTERNS` source across lines to extract `(name, regex, replacement)` tuples and hash them. Pure shell would false-positive on near-line-continuation patterns (same lesson surfaced by `multi-tenant-isolation-check` in P0). Documented in oracle README + DISCOVERIES.md D-FBR-10.
- **Write-boundary scrubbing chokepoint** (P1 Stage 1, deviation from brief) — PII scrub applied at `ScrubbingMakeWriter` instead of a `tracing_subscriber::Layer<...>` impl. Captures formatter prefixes + span metadata + JSON-encoded fields the Layer would miss. Three-leg defense: `install_global_subscriber` chokepoint (type system) + Probe A AST oracle (forbids `impl Layer<...> for ...` outside crate) + clippy baseline. Documented in `crates/feedbackr-tracing/README.md` Decision Log + DISCOVERIES.md D-FBR-11.
- **`TenantRepo::get_brand` separation from `find_by_email`** (P1 Stage 1) — pre-auth surface kept minimal; brand reads route through normal `&TenantScope`-scoped path. Documented in `crates/feedbackr-repository/README.md` Decision Log.
- **Migration 00003 cohesion** (P1 Stage 1) — `feedback.status` column + `feedback_status_history` table land in the SAME migration; removes a forced Stage 2 widening. Documented in the migration file header comment.
- **DEC-PODS-001 / DEC-PODS-002** (P0 Stage 2) — pre-auth allowlist as a repeatable Contract-C1 widening mechanism. Pattern still active for P1+.

## Risks

| Risk | Stage | Notes |
|---|---|---|
| **AGPL LICENSE stub** | Pre-public | User-action: replace `LICENSE` file with full AGPL-3.0 text before first public push. Repo MUST stay local-only until then per DEC-FBR-05 + project CLAUDE.md `--skip-push` invariant. |
| **GitHub org + domain registration** | Pre-public | User-action pending. Working name "Feedbackr" through P3; brand pass at P4 (DEC-FBR-09). |
| **In-memory anonymous rate-limiter loses state on restart** | P0 (deferred to v1.1) | Acceptable for P0 single-instance dogfood; non-breaking Redis backend swap planned for v1.1. See DISCOVERIES.md D-FBR-08. |
| **PII scrub drift in P1** | P1 | **MITIGATED** — `pii-scrub-audit` oracle shipped (Task Zero leg 2). Probe A (AST) + Probe B (SHA-256 over canonical pattern set). PASS on this commit. |
| **GitCellar peer repo coordination** | Late P2 / early P3 | First cross-repo touchpoint when GitCellar embeds the widget. Forward-looking only; not a P1 blocker. |

## Next-Best-Steps

1. **User action** — replace `LICENSE` stub with full AGPL-3.0 text; register GitHub org + domain. (Orthogonal to P1 implementation.)
2. **`/0-uldf-pods-parallelize "Feedbackr P1 Stage 2 — Status Workflow + Admin UI"`** — Stage 2 PODS spawn consuming `docs/planning/handoffs/p1-stage1-to-stage2.md` as the freeze surface. Two workers: Worker A (backend status workflow + emails, FR-FBR-08+09) + Worker B (frontend admin UI on port 14204, FR-FBR-07).
3. After Stage 2 converges: **Stage 3 single session in converging tree** — `scripts/e2e-p1-curl.sh` witness + carry-forward critic findings + ULADP module READMEs for any new modules.
4. **P1 exit gate** → `/0-uldf-finalize --skip-push` → `/0-uldf-ldis-plan "Feedbackr P2 — Customer-Facing"`.
