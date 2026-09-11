# CLOSED 2026-09-11 - the feedbackmonk backup is now alerted on, not just verified

Kept as the record of what was built and what remains true. **Nothing here is outstanding.**

## What runs

| Job | When (UTC) | What it proves |
|---|---|---|
| `feedbackmonk-pg-backup` | 04:30 daily | dumps, encrypts, uploads, then re-downloads its own object and checks size + OpenPGP |
| `feedbackmonk-backup-verify` | 06:30 daily | independently re-checks the newest dump: < 26 h old, >= 1 KiB, downloads to its listed size, parses as OpenPGP |

Both scripts are version-controlled in `deploy/backup/`; the Railway services inline them, so editing
one does not change the other. Full record: `docs/planning/feedbackmonk-deploy-state.md` Stage H/I/J.

## How absence is detected

Better Stack heartbeat **`feedbackmonk-backup-verify`, id 492293** - period 86400 s, grace 3600 s.
The verifier pings it **only on a real PASS**, so a failed verify and a verify that never ran are
both silence, and silence alerts after about an hour past the expected time. Proven by watching the
heartbeat go `pending` -> `up` on a one-shot run.

The ping URL lives only in the service's `FBM_VERIFY_HEARTBEAT_URL` variable on Railway. **It is a
secret and this repo is public - never paste it into the tree.**

## What is still NOT proven, honestly

Tier 1 proves a well-formed encrypted artifact exists and is fresh. It does **not** prove the dump
restores. Only a decrypt-and-restore does that, and the private key belongs to GitCellar
(`security@gitcellar.com`, key `FF02A40CF17791EF`). Their `ci/pg-backup/verify-restore.sh` Tier 2 is
the shape of that check if it is ever wanted for this database.

## Couplings that remain

- Both jobs reuse GitCellar's `r2-backup-writer` credentials and bucket. **A rotation there breaks
  both**, and the verifier's own error message says so when it cannot list the bucket.
- No lifecycle or retention policy applies to the `fbm/` prefix; objects accumulate until someone
  sets one. That is GitCellar's bucket.
