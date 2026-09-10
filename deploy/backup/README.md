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
| `feedbackmonk-pg-backup.sh` | the deployed job: dump → gzip → GPG-encrypt → upload to R2, then read back and verify |

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

## Known gap — no dead-man's switch on this prefix

`gitcellar-backup-verify` covers `pg/` only. **Nothing alerts if this job silently stops running.**
The in-job read-back fails loudly in the logs, but a cron that never fires produces no logs at all,
and that is precisely the failure class that sat undetected for two weeks on the GitCellar side.
Closing it means either extending that verify cron to check `fbm/` too, or giving this job its own
heartbeat ping — both need a decision that is not this repo's to make alone. Filed to GitCellar as
`docs/planning/deferred/feedbackmonk-backup-prefix-and-verification-20260910.md`.

## Decision log

- **2026-09-10 — a separate service, not a second dump inside GitCellar's job.** Extending
  `gitcellar-pg-backup` to dump both databases was the smaller diff, but it modifies a critical
  service on another product's backup path, creates sync debt against that repo's committed
  `backup.sh`, and couples two failure domains. A separate cron costs a few seconds of compute a day
  and keeps both blast radii intact.
- **2026-09-10 — read-back verification inside the job.** Because no external verifier watches this
  prefix yet, the job proves its own artifact retrievable rather than only reporting that an upload
  call returned zero. It is not a substitute for a dead-man's switch and does not claim to be.
