# `deploy/backup/` — nightly off-provider backup of the production feedbackmonk database

## Purpose

Version-controlled source of truth for the nightly `feedbackmonk` Postgres backup that runs on
GitCellar's Railway as the cron service **`feedbackmonk-pg-backup`**. The live deployment runs this
exact script, base64-wrapped into the service's start command, on the public `postgres:16` image —
the same deployment shape GitCellar uses for its own backup cron, and for the same reason: no
built artifact is needed for a job whose whole body is one shell script.

**If you change the script here, you must re-deploy the start command, or the two silently diverge.**
The recipe is in `docs/planning/feedbackmonk-deploy-state.md` § Stage H.

## Files

| File | What it is |
|---|---|
| `feedbackmonk-pg-backup.sh` | the nightly job (04:30 UTC): dump → gzip → GPG-encrypt → upload to R2, then read back and verify |
| `feedbackmonk-backup-verify.sh` | the daily Tier-1 verifier (06:30 UTC): freshness, size and OpenPGP structure of the newest `fbm/` object |

## What it does

`pg_dump` the `feedbackmonk` database, gzip, encrypt to `security@gitcellar.com`
(key `FF02A40CF17791EF`), upload to `s3://gitcellar-backups-eu/fbm/feedbackmonk-<TS>.sql.gz.gpg`,
then **download the object again** and check its size and OpenPGP structure before reporting success.
Runs at **04:30 UTC daily**. Emits `FBM_BACKUP_COMPLETE <key>` on success and `FBM_BACKUP_FAIL` on
any failure.

## Constraints — each of these is load-bearing

- **Writes under the `fbm/` prefix, never `pg/`.** GitCellar's `gitcellar-backup-verify` cron
  verifies *the newest object under `pg/`*. A second database's dumps landing there would silently
  point that verification at the wrong file on alternating days, weakening a guarantee that already
  exists. Separate prefix, separate job, no interference in either direction.
- **No `set -e`.** `gpg --list-packets` exits non-zero on a file we hold no secret key for — which
  is every file this job writes — while still printing the packet listing the check needs. Under
  `set -e` the read-back check killed the script silently, so the upload succeeded, nothing was
  verified, and no completion line printed. Grade on the listing, never on gpg's exit code, and
  check every other exit code explicitly. (Found and fixed on the first proving run, 2026-09-10.)
- **`GNUPGHOME` is pinned to a writable path.** The bare `postgres:16` cron container has no usable
  `HOME`, so gpg 2.2 cannot create `~/.gnupg` and downstream commands print nothing to stdout. That
  exact trap made GitCellar misread healthy backups as corrupt for two weeks in 2026.
- **Encrypt to the same recipient as the GitCellar dumps.** Verified by comparing packet listings:
  both prefixes report `keyid FF02A40CF17791EF`, so one private key restores both. Changing the
  recipient here would create a second restore path that nobody holds a key for.

## Relationships

- **Shares GitCellar's R2 bucket and its `r2-backup-writer` credentials** (Railway variables copied
  from `gitcellar-pg-backup`). **Coupling worth knowing:** if that token is rotated, this job breaks
  too, and its failure is only visible in its own logs.
- The private half of the encryption key is GitCellar's, held for `security@gitcellar.com`. Restoring
  a feedbackmonk backup therefore needs GitCellar's key custody — see its
  `docs/operations/SIGNING_KEY_CUSTODY.md`.
- GitCellar's equivalent job and its restore-verification tiers: `ci/pg-backup/` in that repo.

## Verification, and the one gap that is left

`feedbackmonk-backup-verify.sh` runs at **06:30 UTC**, two hours after the backup, as its own Railway
cron service (`717daf20-603a-48b5-8977-99b8fc44d5b1`). It lists `fbm/`, takes the newest object, and
fails unless it is under 26 h old, at least 1 KiB, downloads to exactly its listed size, and parses
as OpenPGP. Tier 1 only: it does **not** prove restorability, which needs a real decrypt-and-restore
against a private key that is GitCellar's.

Its failure messages distinguish the two diagnoses that matter, because they point at different
services: an empty listing means **the backup job never uploaded** (go read that job's logs), while a
listing *error* means **this verifier cannot see the bucket** (go check its credentials). All three
paths were exercised before deployment — pass, empty prefix, unreachable bucket — and the two
failures exit non-zero.

**The gap that remains, and it needs one owner action.** `FBM_VERIFY_HEARTBEAT_URL` is unset, so the
script prints a NOTE and pings nothing. Until a heartbeat exists, a cron that *stops firing
altogether* is still silent: it produces no logs to fail in, and a failed verify and a verify that
never ran are the same silence. Create a Better Stack heartbeat (daily period, generous grace) and
set its URL as `FBM_VERIFY_HEARTBEAT_URL` on the verify service — the ping code is already written
and gated on that variable, exactly as GitCellar's verifier does it. Body:
`docs/pending/feedbackmonk-backup-alerting.md`.

## Decision log

- **2026-09-10 — a separate service, not a second dump inside GitCellar's job.** Extending
  `gitcellar-pg-backup` to dump both databases was the smaller diff, but it modifies a critical
  service on another product's backup path, creates sync debt against that repo's committed
  `backup.sh`, and couples two failure domains. A separate cron costs a few seconds of compute a day
  and keeps both blast radii intact.
- **2026-09-10 — read-back verification inside the backup job**, kept even now that a separate
  verifier exists: it fails at the moment of writing rather than up to 26 h later, and the two checks
  are independent.
- **2026-09-10 — a separate verifier rather than extending GitCellar's.** Extending theirs was the
  obvious move and is wrong twice over. Their verifier pings a **single** Better Stack heartbeat, so
  folding these checks in would let a feedbackmonk failure silence GitCellar's backup alarm and read
  as *their* backup breaking — one product's noise degrading another's signal. And their script is
  committed in the GitCellar repo under a "keep in sync" warning, so changing the live job means
  either editing that tree, which DEC-FBR-07 forbids from here, or knowingly leaving drift in a
  critical verifier. A second cron costs seconds of compute a day and keeps both blast radii and
  both alarms intact.
