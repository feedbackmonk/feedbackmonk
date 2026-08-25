---
id: DEFER-004
title: Redeploy feedback.gitcellar.com with migration 00029, then arm the GitCellar rating E2E
status: PROPOSED
origin: inject
source-project: GitCellar
source-session-id: 837dabb5-e4e3-4c51-8d6d-fff1dbeef245
injected-at: 2026-08-24T22:25:00Z
autonomy-hint: supervised
suggested-entry-point: implement
scope-estimate: single-session
content-hash: fbm-redeploy-00029-20260824
---

# DEFER-004: Redeploy feedback.gitcellar.com with migration 00029, then arm the GitCellar rating E2E

## Idea

Commits `73301e6` + `f4b7a61` are on `main` and add the additive 1-5 `rating` field
(migration `00029`) plus a status-aware solicitation snooze. The live instance at
`feedback.gitcellar.com` has NOT been redeployed, so neither is reachable in production.
Redeploy it carrying `00029`, then arm the consuming test on the GitCellar side.

> Timings and file references are snapshots from 2026-08-24; re-measure before acting. The
> fix shape is a hypothesis from outside the deploy — re-derive it against the live system.

## Originating Context

GitCellar's Desktop feedback card was reworked from a 3-emoji tap to a 1-5 rating
(GitCellar `7550faf3cd`). The card sends BOTH `rating` and the 3-point `sentiment` it derives
locally, specifically so that an un-redeployed FeedbackMonk keeps working — it ignores the
unknown `rating` field and records the correct sentiment. So there is NO user-facing breakage
today; the cost is silent and twofold:

1. **The finer-grained data is being discarded.** Every rating submitted between now and the
   redeploy collapses to 3 buckets and the 1-5 value is lost, unrecoverably — the whole point
   of the change.
2. **A leftover red signal.** `tests/e2e/test-e2e-solicitation.ps1` § 9-10 (added in the same
   GitCellar commit) assert the rating contract and the snooze policy. Against a
   pre-`00029` server they go RED — capability absent, out-of-range values returning 200
   instead of 400. That is correct behaviour for a bar set ahead of the deploy, but a reader
   seeing it must interpret it as "not deployed yet" rather than "regression". The longer the
   gap, the likelier someone mis-reads it. The ARHG-06 judge that reviewed those assertions
   flagged this explicitly as the residual operational cost of shipping them early.

## Success Criterion

`GET /api/v1/capabilities` on the live instance lists `feedback.rating`; a
`POST .../feedback {"rating": 4}` returns 200 with `echo.sentiment == "positive"` and
`echo.rating == 4`; an out-of-range rating returns 400; and `GET .../me/solicitation` carries
`snooze_days` and `applied_cooldown_days` in its `policy` object. Then GitCellar's
`test-e2e-solicitation.ps1` passes with `GITCELLAR_E2E_SOLICITATION_ARMED=1`.

## Dependencies

Blocked on a production deployment of `feedback.gitcellar.com`, which needs the owner's word
and their deployment access — it is not something an agent session performs unprompted. The
code side is already done, committed and pushed; nothing else gates it.

## Related Artifacts

- `migrations/00029_feedback_rating.sql` (this repo)
- `crates/feedbackmonk-core/src/rating.rs` — the `Rating` type and `to_sentiment`
- `crates/feedbackmonk-api/src/handlers/solicitation.rs` — status-aware `cooldown_for`
- `docs/integrations/gitcellar-adoption.md` § 12 — the full contract for both changes
- GitCellar: `docs/specs/feedback-solicitation/FEEDBACKMONK_CONTRACT.md` (Addendum 2026-08-24)
- GitCellar: `tests/e2e/test-e2e-solicitation.ps1` § 9-10 — the assertions this unblocks
