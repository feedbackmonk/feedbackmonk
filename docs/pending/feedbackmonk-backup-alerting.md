# The nightly feedbackmonk backup has no dead-man's switch

Since 2026-09-10 the production `feedbackmonk` database is dumped nightly at 04:30 UTC by its own
Railway cron, encrypted to the same key that restores GitCellar's dumps and read-back-verified on
every run. Source of truth: `deploy/backup/`. Full record:
`docs/planning/feedbackmonk-deploy-state.md` § Stage H.

## What is missing

**Nothing alerts if the job silently stops firing.** GitCellar's `gitcellar-backup-verify` cron
checks only its own `pg/` prefix, and a cron that never runs produces no logs to fail loudly in.
That is precisely the failure class that sat undetected for two weeks on the GitCellar side in 2026
— a failed verify and a verify that never ran are the same silence.

The job already re-downloads each object it writes and checks its size and OpenPGP structure, so
what is missing is **liveness alerting, not artifact checking**.

## The two options

1. **Extend GitCellar's verify cron** to check the newest object under `fbm/` as a second assertion.
   Needs its own age/size/structure checks and probably a second heartbeat, since one heartbeat
   cannot say which half failed. Touches a GitCellar service, so it needs their word as well as ours.
2. **Give this job its own heartbeat.** Self-contained on our side, but needs a heartbeat endpoint
   created in the owner's Better Stack account.

Filed to GitCellar as `docs/planning/deferred/feedbackmonk-backup-prefix-and-verification-20260910.md`.

## Two couplings inherited with it

- The job reuses GitCellar's `r2-backup-writer` credentials and bucket. **A rotation there breaks
  this backup**, visibly only in this job's own logs.
- No lifecycle or retention policy applies to the `fbm/` prefix; objects accumulate until someone
  sets one. That is GitCellar's bucket.
