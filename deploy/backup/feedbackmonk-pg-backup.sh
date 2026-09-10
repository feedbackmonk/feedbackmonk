#!/usr/bin/env bash
# Nightly off-provider backup of the PRODUCTION `feedbackmonk` Postgres database.
#
# Deliberately SEPARATE from gitcellar-pg-backup, which dumps the `railway`
# database. Two reasons, both load-bearing:
#   1. gitcellar-backup-verify verifies the NEWEST object under the `pg/`
#      prefix. Writing a second database's dump there would silently point that
#      verification at the wrong file on alternating days, weakening a guarantee
#      that already exists. This job writes under `fbm/` instead, so `pg/` and
#      its verifier are untouched.
#   2. A failure here cannot affect GitCellar's backup, and vice versa.
#
# Encrypted to the SAME recipient as the GitCellar dumps (key FF02A40CF17791EF),
# so the existing private key restores both. Same bucket, different prefix.
#
# NO `set -e`, deliberately -- and this is the whole reason the first version of
# this script was wrong. `gpg --list-packets` on a file we hold no secret key
# for exits NON-ZERO ("decryption failed: No secret key") even though the packet
# listing it printed is exactly what we want. Under `set -e` the read-back check
# killed the script silently: the upload succeeded, nothing was verified, and
# no completion line was ever printed -- so a genuinely corrupt upload would
# have died just as quietly. Check every exit code explicitly instead, the same
# way verify-cron-inline.sh does.
set -uo pipefail

# gpg needs a WRITABLE homedir. The bare postgres:16 cron container runs with
# HOME unset or pointing nowhere, so gpg 2.2 cannot create ~/.gnupg, writes its
# error to stderr, and downstream commands print NOTHING to stdout -- which is
# how this project spent 2026-06-16..07-01 misreading healthy backups as corrupt.
export GNUPGHOME=/tmp/gpghome
mkdir -p "$GNUPGHOME" && chmod 700 "$GNUPGHOME"

for v in DATABASE_URL GPG_PUBLIC_KEY GPG_RECIPIENT R2_ENDPOINT R2_BUCKET R2_ACCESS_KEY_ID R2_SECRET_ACCESS_KEY; do
  eval "val=\${$v:-}"
  if [ -z "$val" ]; then echo "FAIL: required env var $v is unset on the service"; echo FBM_BACKUP_FAIL; exit 1; fi
done

# Toolchain on the bare image; retry once (a transient apt mirror error is the
# single most common one-off cron failure).
if ! command -v aws >/dev/null || ! command -v gpg >/dev/null; then
  for attempt in 1 2; do
    apt-get update -qq && apt-get install -y -qq --no-install-recommends gnupg awscli ca-certificates >/dev/null && break
    echo "WARN: toolchain install attempt $attempt failed; retrying"; sleep 5
  done
fi
if ! command -v aws >/dev/null || ! command -v gpg >/dev/null; then
  echo "FAIL: toolchain install failed (aws/gpg unavailable)"; echo FBM_BACKUP_FAIL; exit 1
fi

printf "%s" "$GPG_PUBLIC_KEY" | gpg --batch --import >/dev/null 2>&1
if ! gpg --batch --list-keys "$GPG_RECIPIENT" >/dev/null 2>&1; then
  echo "FAIL: recipient $GPG_RECIPIENT not in keyring after import -- GPG_PUBLIC_KEY is wrong or malformed"
  echo FBM_BACKUP_FAIL; exit 1
fi

TS=$(date -u +%Y%m%d-%H%M%S)
KEY="fbm/feedbackmonk-$TS.sql.gz.gpg"
OUT=/tmp/fbm.gz.gpg

# pipefail is on, so a pg_dump or gzip failure inside the pipeline is caught
# here rather than uploading a truncated blob.
pg_dump "$DATABASE_URL" | gzip | gpg --batch --yes --trust-model always \
  --encrypt --recipient "$GPG_RECIPIENT" > "$OUT"
rc=$?
if [ "$rc" -ne 0 ]; then
  echo "FAIL: dump|gzip|encrypt pipeline rc=$rc -- nothing uploaded"; echo FBM_BACKUP_FAIL; exit 1
fi

SZ=$(stat -c%s "$OUT" 2>/dev/null || echo 0)
echo "dump encrypted: ${SZ}B"
if [ "$SZ" -lt 1024 ]; then
  echo "FAIL: encrypted dump implausibly small (${SZ}B) -- refusing to upload"; echo FBM_BACKUP_FAIL; exit 1
fi

export AWS_ACCESS_KEY_ID="$R2_ACCESS_KEY_ID" AWS_SECRET_ACCESS_KEY="$R2_SECRET_ACCESS_KEY" AWS_DEFAULT_REGION=auto
up_err=$(aws --endpoint-url "$R2_ENDPOINT" s3 cp "$OUT" "s3://$R2_BUCKET/$KEY" 2>&1)
up_rc=$?
if [ "$up_rc" -ne 0 ]; then
  echo "FAIL: upload rc=$up_rc -- $up_err"; echo FBM_BACKUP_FAIL; exit 1
fi
echo "uploaded $KEY"

# --- read-back verification, in this same run ---
# "It uploaded" and "it is retrievable and well-formed" are different claims,
# and there is no dead-man's-switch on this prefix yet, so prove it here where
# a failure still fails the job LOUDLY.
F=0
RB=/tmp/fbm-readback.gpg
cp_err=$(aws --endpoint-url "$R2_ENDPOINT" s3 cp "s3://$R2_BUCKET/$KEY" "$RB" 2>&1)
cp_rc=$?
if [ "$cp_rc" -ne 0 ]; then
  echo "FAIL: read-back download rc=$cp_rc -- $cp_err"; echo FBM_BACKUP_FAIL; exit 1
fi
RB_SZ=$(stat -c%s "$RB" 2>/dev/null || echo 0)
if [ "$RB_SZ" != "$SZ" ]; then
  echo "FAIL: read-back size ${RB_SZ}B != uploaded ${SZ}B -- $KEY"; F=1
fi

# EXPECT a non-zero exit here: we hold no secret key, so gpg reports decryption
# failure while still printing the packet listing we actually want. Grade on the
# listing, never on gpg's exit code.
lp_out=$(gpg --list-packets "$RB" 2>/tmp/gpg.err)
if printf '%s' "$lp_out" | grep -qiE 'encrypted data|pubkey enc'; then
  echo "openpgp OK ($(printf '%s' "$lp_out" | grep -m1 -oE 'keyid [0-9A-F]+'))"
else
  echo "FAIL: bad openpgp -- gpg stderr: $(tr '\n' ' ' </tmp/gpg.err)"
  echo "  first-bytes: $(head -c 32 "$RB" | od -An -tx1 | tr -s ' ')"
  F=1
fi

rm -f "$OUT" "$RB"
if [ "$F" -ne 0 ]; then echo FBM_BACKUP_FAIL; exit 1; fi
echo "FBM_BACKUP_COMPLETE $KEY (${SZ}B, R2-EU, read-back verified)"
