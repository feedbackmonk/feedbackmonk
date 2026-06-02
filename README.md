# feedbackmonk

Privacy-first product feedback. Hear your users without spying on them.

**Status**: P1 Stage 2 complete. P0 + P1 Stage 1 + P1 Stage 2 shipped per the build arc; P1 finalize + P2 plan upcoming.

**License**: AGPL-3.0-or-later (see `LICENSE`).

> **Naming note**: the project was working-named "Feedbackr" through P0 and most of P1. The name was changed to **feedbackmonk** on 2026-05-14 per DEC-FBR-11, enacting DEC-FBR-09's squat-contingency clause after `github.com/Feedbackr` and `feedbackr.com` were found taken by an inactive squatter. Decision and requirement ID prefixes `DEC-FBR-*` / `FR-FBR-*` are stable identifiers and do NOT rename. Working-directory and Cargo-crate code-level renames are tracked as Pending Follow-Ups in `CLAUDE.md`.

## Synopsis

Repository root of feedbackmonk — a privacy-first open-source SaaS feedback platform ("Plausible Analytics for product feedback"): submission widget + status-workflow triage + public roadmap with voting + status emails, multi-product per tenant. Rust backend (`crates/`), React admin UI (`admin-ui/`), vanilla-TS embeddable widget (`widget/`), Astro marketing site (`marketing/`), Postgres migrations (`migrations/`), self-host docker stack (`deploy/docker/`). Start with `docs/specs/SPECIFICATION.md` and `CLAUDE.md`.

---

## What this is

feedbackmonk is a standalone open-source SaaS user-feedback platform for indie developers and privacy-conscious teams: submission widget + status-workflow triage + public roadmap with voting + status emails. Multi-product per tenant.

Elevator pitch: *Plausible Analytics for product feedback.*

## Read first

- `docs/specs/SPECIFICATION.md` — 18 functional requirements across 5 phases (P0-P4)
- `docs/specs/DECISIONS.md` — DEC-FBR-01..11 + DEC-FBR-IMPL-* (all RESOLVED)
- `docs/specs/ARCHITECTURE.md`
- `docs/planning/plans/` — latest build-arc plan and per-phase plans
- `CLAUDE.md` — Claude Code project context (read this if you're an AI agent working in this repo)

## Origin

Spec session and build-arc plan were authored in the GitCellar repo's working tree (the working reference implementation lives in `gitcellar-cloud/src/feedback/`) and migrated here per DEC-FBR-07. GitCellar will be feedbackmonk's customer #1 via widget embed; there is no source-level dependency between the two repos.

## Status (current)

P0 Foundation + P1 Closes-the-Loop Stage 2 shipped. Multi-tenant data model, submission API (JWT-verified + anonymous), customer signup/onboarding, status workflow, status emails, admin UI, and the `multi-tenant-isolation-check` and `pii-scrub-audit` Verification Oracles are all live. Next: P1 finalize → P2 (Customer-Facing: widget, public roadmap, voting, promote).
