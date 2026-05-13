# Feedbackr — Project Context for Claude Code

Project-specific context. The global ULDF framework guidance lives at `~/.claude/CLAUDE.md` and is the authoritative reference for framework commands, autonomy levels, propagation rules, and the agentic disciplines (Contexturgy, Oraculurgy, Probandurgy). This file ONLY documents what is specific to **Feedbackr**.

---

## What Feedbackr is

Standalone open-source SaaS user-feedback platform: submission widget + status-workflow triage + public roadmap with voting + status emails. Multi-product per tenant.

- **Elevator pitch**: *Plausible Analytics for product feedback.*
- **License**: AGPL-3.0-or-later (see `LICENSE` — currently a stub; replace with full text before first public commit, per DEC-FBR-05).
- **Stage**: pre-P0. Build arc planned across 5 phases (P0–P4), ~12 weeks FTE / ~6 months calendar with GitCellar context-switching.

## Read first (always, for any session in this repo)

| File | What it tells you |
|---|---|
| `docs/specs/SPECIFICATION.md` | 18 functional requirements (FR-FBR-01..18) across phases P0–P4, status RFP |
| `docs/specs/DECISIONS.md` | DEC-FBR-01..10, all RESOLVED — load-bearing context for every implementation choice |
| `docs/specs/ARCHITECTURE.md` | System architecture (currently skeletal, adequate for planning) |
| `docs/planning/plans/20260513T185711-feedbackr-v1-build-arc.md` | Full build-arc plan: phase ordering, gates, interface contracts, Oracle Pre-Build Plan, Testability Gate findings |

The arc plan is the single most important downstream artifact — it pre-commits phase ordering and exit gates but defers intra-phase topology to each phase's own `/0-uldf-ldis-plan` round.

## Stack (load-bearing for tooling decisions)

- **Backend**: Rust. Reference implementation is `gitcellar-cloud/src/feedback/` in the peer GitCellar repo — read-only reference, NOT a base to extract from (DEC-FBR-07).
- **Admin UI / widget**: TypeScript. React for admin UI (P1, port pattern from `gitcellar-cloud/admin-ui/`); vanilla JS+CSS for the embeddable widget (P2, <30KB bundle cap per FR-FBR-04).
- **Marketing site**: Astro (P4 only).
- **Database**: PostgreSQL. Multi-tenant via `tenant_id` + `project_id` on every domain row; tenant-scoped repository layer is the **sole** query path (raw SQL is a security incident — DEC-FBR-03).
- **Distribution**: SaaS + self-host via `docker compose up` (P4).
- **Testing**: Vitest for unit tests, Playwright + axe-core for widget a11y (mandated by P2 Testability Gate finding).
- **Billing**: Polar (P3), pattern from GitCellar's existing setup.

## Dev Port

**Frontend**: `14204` (claimed at framework setup 2026-05-13; registered in `~/.claude/MACHINE_CONFIG.md` Dev Port Registry). P1 admin UI must bind this with `strictPort: true` in `vite.config.ts`. Backend will claim a `143xx` port at the P0 plan round.

## Workflow

- Use `/0-uldf-ldis-plan "Feedbackr P<N> — <Phase Name>"` at each phase boundary to author the per-phase plan (intra-phase topology + interface contracts + Oracle build/freshness rules) before starting implementation.
- Use `/0-uldf-proceed` at phase boundaries — let it pick HERE / HANDOFF / PODS topology based on context budget and work shape; don't manually invoke the next command.
- LTADS is **not yet initialized** for this repo. The arc plan flags that LTADS state lives in *this* repo (not GitCellar). Initialize with `/0-uldf-ltads-admin init` when ready to begin P0 implementation.
- Per DEC-FBR-07, this repo is greenfield — there is no source-level dependency on GitCellar. Do NOT modify GitCellar code from this working tree.

## Oracles

This project has `.claude/oracles/` with the universal starter pack installed at setup. The session-start hook runs every-session fast oracles and emits an ORACLE BRIEFING (git state, LTADS state, project type, pending follow-ups, etc.) — read it before investigating manually. Audit via `/0-uldf-oracle`.

Four Verification Oracles are scheduled in the arc plan (built at the phase that needs them):

| Oracle | Phase | Why |
|---|---|---|
| `multi-tenant-isolation-check` | P0 Task Zero | Every P0+ data path goes through this. Cross-tenant leakage is silent without it. |
| `pii-scrub-audit` | P1 | Port from GitCellar's existing oracle; drift-detection over canonical 20-pattern set (FR-FBR-10). |
| `widget-bundle-size` | P2 (start) | Defends the <30KB cap (FR-FBR-04) as a contract, not aspiration. |
| `tier-enforcement-status` | P3 | Defends cap-firing + "powered by Feedbackr" footer (FR-FBR-14). |

## Constraints not in spec artifacts

- **AGPL LICENSE file is a stub.** Replace with the full AGPL-3.0 text from `https://www.gnu.org/licenses/agpl-3.0.txt` before the first public push. Until then, the repo stays local-only.
- **GitHub org + domain are pre-registration pending** — user action, not a blocker for P0 implementation. Working name stays "Feedbackr" through P3; brand pass at P4 (DEC-FBR-09).
- **GitCellar peer repo** is in pre-launch hardening. Feedbackr work neither blocks on nor modifies GitCellar; the only cross-repo touchpoint is late P2 / early P3 when GitCellar embeds Feedbackr's widget as customer #1 (forward-looking integration, NOT extraction).

## Privacy invariants (load-bearing — never silently relax)

- **No third-party trackers in the widget, ever** (no Segment, Mixpanel, GA, Intercom). DEC-FBR-02 brand promise.
- **JWT customer signs is the ONLY identity Feedbackr ever has** for an end-user (DEC-FBR-04). No callbacks to customer auth providers; no long-lived bearer tokens.
- **Q24 invariant** (FR-FBR-12, P2): roadmap items promoted from feedback contain the feedback body verbatim with NO submitter attribution and NO FB-ID reference. Port the byte-for-byte unit test from GitCellar's `roadmap_promote.rs` — same test name, same assertions. Document as untouchable in the module README.

## Pending Follow-Ups

<!-- /0-uldf-schedule writes here -->

_None yet._

---

## License footer

Feedbackr is AGPL-3.0-or-later. Contributors agree via DCO sign-off (no CLA per DEC-FBR-05). Self-host customers receive identical releases to SaaS; there is no proprietary fork.
