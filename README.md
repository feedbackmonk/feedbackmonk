# Feedbackr

Privacy-first product feedback. Hear your users without spying on them.

**Status**: pre-P0. Spec READY-FOR-PLANNING; build arc planned. See `docs/specs/SPECIFICATION.md` and `docs/planning/plans/`.

**License**: AGPL-3.0-or-later (see `LICENSE`).

---

## What this is

Feedbackr is a standalone open-source SaaS user-feedback platform for indie developers and privacy-conscious teams: submission widget + status-workflow triage + public roadmap with voting + status emails. Multi-product per tenant.

Elevator pitch: *Plausible Analytics for product feedback.*

## Read first

- `docs/specs/SPECIFICATION.md` — 18 functional requirements across 5 phases (P0-P4)
- `docs/specs/DECISIONS.md` — DEC-FBR-01..10 (all RESOLVED)
- `docs/specs/ARCHITECTURE.md`
- `docs/planning/plans/` — latest build-arc plan

## Origin

Spec session and build-arc plan were authored in the GitCellar repo's working tree (the working reference implementation lives in `gitcellar-cloud/src/feedback/`) and migrated here per DEC-FBR-07. GitCellar will be Feedbackr's customer #1 via widget embed; there is no source-level dependency between the two repos.

## Status

This repo is currently a skeleton awaiting framework setup (`/0-uldf-setup-project`) and the P0 planning round.
