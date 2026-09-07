# The one redeploy of `feedback.gitcellar.com` — four items stack on it

**Not this repo's code.** No feedbackmonk source change is outstanding for any of these; each is
GitCellar-Railway ops. Ordered runbook: `docs/operations/RAILWAY_GITCELLAR.md` § 8. Currently blocked
upstream of all four by `docs/planning/deferred/DEFER-009_railway-deploy-blocked-feedbackmonk-api.md`
(Railway cannot create containers for the service at all).

Authoritative live-deployment record (IDs, credential names, stage history):
`docs/planning/feedbackmonk-deploy-state.md`.

**Measured 2026-08-30**: `curl -sS https://feedback.gitcellar.com/api/v1/capabilities` returns
`"version":"0.2.0"` with 5 capabilities (`/health/ready` 200). Current code is **0.4.0 with 15**.

## What is waiting

1. **Phase-A A6** — ≥ v0.3.0 + migrations `00020`+`00021`, which turn on the six Phase-A capabilities
   `feedback.delete | reply_state | export | severity | idempotency | attachments`. **This one gates
   GitCellar's Phases B/C** (delete its internal Cloud-API feedback backend, unify its two feedback
   screens), so it stands on its own merits even if the rest waits.
2. **DEFER-004** — migration `00029` → `feedback.rating`.
3. **FR-FBR-32/33** — v0.4.0 + migration `00030` → the `hosting.*` capabilities.
4. **A live production defect (2026-09-01)** — `FeedbackStatus::WontFix` serialised as `wont-fix`
   (serde `rename_all = "kebab-case"`) while the DB CHECK, Contract C6 and every client status union
   use `wontfix`. On the live admin at `triage.gitcellar.com`, any `wontfix` feedback rendered a blank
   status pill and opening it **white-screened the page** (`LEGAL_TRANSITIONS["wont-fix"]` is
   `undefined` → `undefined.length` in `StatusControls`); `?status=wontfix` filtering and
   `to_status: "wontfix"` transitions were rejected as an unknown variant. **78 of 79 prod feedback
   rows are `wontfix`**, so the triage inbox is ~99% unusable. Fixed at HEAD
   (`#[serde(rename = "wontfix")]` + an all-six-variants JSON⇔DB round-trip test, plus `?? []` /
   label-fallback hardening in `StatusControls` + `StatusBadge` so UI-vs-wire drift can never
   white-screen the admin again). **Only a redeploy clears it for the operator.**

## Verification after the redeploy

`GET https://feedback.gitcellar.com/api/v1/capabilities` advertises the six Phase-A capabilities, then
smoke each new route. That verification is what unblocks GitCellar Phases B/C.

## The alternative

The **DEC-FBR-14 cutover retires all four at once** — a SaaS instance runs current code with every
migration applied. See `docs/pending/saas-standup.md`. Filed to GitCellar as **DEFER-084**.

## Phase A, for reference

All five contract surfaces are BUILT and locally verified (CI-parity green); the crate is past v0.3.0.
Everything was ADDITIVE to the frozen contract and each is advertised via `GET /api/v1/capabilities`;
`docs/integrations/gitcellar-adoption.md` carries the contract. Full delivery record and the confirmed
decisions (D-A1 hard-delete + attachment-byte purge, D-A4 optional `severity`, D-A5 export included):
`docs/pending-followups.md` § PF-PHASEA-01.
