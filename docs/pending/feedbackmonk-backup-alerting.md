# The nightly feedbackmonk backup is verified daily, but nothing alerts if it stops

Since 2026-09-10 the production `feedbackmonk` database is dumped nightly at 04:30 UTC and the newest
dump is verified daily at 06:30 UTC by two Railway cron services of our own. Source of truth for both
scripts: `deploy/backup/`. Full record: `docs/planning/feedbackmonk-deploy-state.md` § Stage H.

## What is already covered

The backup job read-back-verifies every object it writes. The verifier independently re-checks the
newest object's freshness (≤ 26 h), size, download integrity and OpenPGP structure, and fails
non-zero with a message naming which service is at fault. All three paths were exercised before
deployment.

## The one thing still missing, and it is a five-minute owner action

**`FBM_VERIFY_HEARTBEAT_URL` is unset on the `feedbackmonk-backup-verify` service**, so nothing
watches for absence. If the cron stops firing altogether it produces no logs to fail in — a failed
verify and a verify that never ran are the same silence, and that exact failure class sat undetected
for two weeks on the GitCellar side in 2026.

To close it:

1. Create a heartbeat in the owner's Better Stack account — daily period, generous grace (GitCellar's
   equivalent uses 86400 s period + 86400 s grace, alerting after ~48 h of silence).
2. Set its URL as `FBM_VERIFY_HEARTBEAT_URL` on the `feedbackmonk-backup-verify` Railway service
   (`717daf20-603a-48b5-8977-99b8fc44d5b1`), then deploy that service once.

**The ping code is already written and gated on that variable** — the script pings only on a real
PASS, and treats a failed ping as a warning rather than a verify failure. Nothing else changes.

> Note when setting it: each `variableUpsert` triggers its own deploy, so set the variable and let
> that deploy stand rather than adding an explicit one. `docs/dev-notes/railway-deploy-diagnosis.md`.

## Why we did not just extend GitCellar's verifier

It was the obvious move and it is wrong twice over. Their verifier pings a **single** heartbeat, so a
feedbackmonk failure would silence GitCellar's backup alarm and read as *their* backup breaking. And
their script is committed in their repo under a "keep in sync" warning, so changing the live job means
either editing that tree, which DEC-FBR-07 forbids from here, or leaving drift in a critical verifier.
Filed for their awareness as `docs/planning/deferred/feedbackmonk-backup-prefix-and-verification-20260910.md`.

## Two couplings inherited

- Both jobs reuse GitCellar's `r2-backup-writer` credentials and bucket. **A rotation there breaks
  both**, and the verifier's error message says so explicitly when it cannot list the bucket.
- No lifecycle or retention policy applies to the `fbm/` prefix; objects accumulate until someone sets
  one. That is GitCellar's bucket.
