---
status: resolved 2026-09-30 (wired on the owner's word, DEC-FBR-IMPL-32)
source: project-checks review, 2026-09-30 (docs/planning/project-checks-review-2026-09.md)
deferred_because: the owner's decision — wiring it adds a product surface GitCellar consumes
harm: the adoption contract promises crash-banner detail once four env vars are set; setting them does nothing
beneficiary: GitCellar (customer #1), and any tenant that links crash events
---

# Crash-event correlation is built but never wired

## What is true today

`crates/feedbackmonk-api/src/crash_correlation.rs` implements `GlitchtipCorrelator`
(pull-mode, best-effort, unit-tested against a mock Glitchtip). Nothing constructs it:
`GlitchtipCorrelator::from_env` has no caller in any crate, and no handler returns the
banner shape `docs/integrations/gitcellar-adoption.md` §5.6 specifies.

So the §5.6 deploy note — "pointing it at GitCellar's live Glitchtip needs the four
`FEEDBACKMONK_GLITCHTIP_{URL,ORG,PROJECT,TOKEN}` env vars set at deploy" — is false:
with all four set, no response carries resolved crash detail. `crash_event_id` is
still stored, which is the part GitCellar depends on today.

`feedback-parity-status` reads gap #2 CLOSED because it looks for `crash_event_id`
in the migrations, not for a wired resolver; its detectors are file-presence checks.

## The decision

1. **Wire it** (recommended if GitCellar Desktop renders the banner): construct the
   correlator once in `main.rs` from env, hold it in `AppState` as
   `Option<Arc<dyn CrashCorrelator>>`, and resolve it in the admin feedback-detail
   response and the `/me/feedback/{id}` read the contract names; add a test with the
   mock Glitchtip. `SELFHOST_ENV.md` then drops its "not wired" note.
2. **Retract the promise**: correct §5.6 and the parity table to say the resolver is
   unwired and the banner is not served, and delete the dead module.

Either way, the sibling brief `gitcellar-crash-event-purge-on-erasure-20260910.md`
depends on the same env-configured client existing.
