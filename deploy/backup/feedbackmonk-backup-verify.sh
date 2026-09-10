#!/usr/bin/env bash
# Daily Tier-1 verification of the newest feedbackmonk backup under the `fbm/` prefix.
#
# Runs 06:30 UTC, two hours after feedbackmonk-pg-backup (04:30) and thirty minutes after
# GitCellar's own verifier. Deliberately a SEPARATE service from `gitcellar-backup-verify`:
#   - that job pings ONE Better Stack heartbeat, so folding these checks into it would let a
#     feedbackmonk failure silence GitCellar's backup alarm and read as THEIR backup breaking;
#   - its script is committed in the GitCellar repo under a "keep in sync" warning, and this
#     project may not modify that tree (DEC-FBR-07), so changing the live job would either
#     require editing their repo or knowingly leave drift in a critical verifier.
#
# Tier 1 = presence, freshness, size and OpenPGP structure. It does NOT prove restorability;
# only a real decrypt-and-restore does, and the private key for that is GitCellar's.
#
# NO `set -e`, deliberately: `gpg --list-packets` exits non-zero on a file we hold no secret key
# for -- which is every file this checks -- while still printing the listing we need. Under
# `set -e` that kills the script silently. Grade on the listing, never on gpg's exit code.
set -uo pipefail
F=0

export GNUPGHOME=/tmp/gpghome
mkdir -p "$GNUPGHOME" && chmod 700 "$GNUPGHOME"

for v in R2_ENDPOINT R2_BUCKET R2_ACCESS_KEY_ID R2_SECRET_ACCESS_KEY; do
  eval "val=\${$v:-}"
  if [ -z "$val" ]; then echo "FAIL: required env var $v is unset on the service"; echo FBM_VERIFY_FAIL; exit 1; fi
done
export AWS_ACCESS_KEY_ID="$R2_ACCESS_KEY_ID" AWS_SECRET_ACCESS_KEY="$R2_SECRET_ACCESS_KEY" AWS_DEFAULT_REGION=auto

if ! command -v aws >/dev/null || ! command -v gpg >/dev/null || ! command -v curl >/dev/null; then
  for attempt in 1 2; do
    apt-get update -qq && apt-get install -y -qq --no-install-recommends gnupg awscli ca-certificates curl >/dev/null && break
    echo "WARN: toolchain install attempt $attempt failed; retrying"; sleep 5
  done
fi
if ! command -v aws >/dev/null || ! command -v gpg >/dev/null; then
  echo "FAIL: toolchain install failed (aws/gpg unavailable)"; echo FBM_VERIFY_FAIL; exit 1
fi

# `aws s3 ls` exits 1 with NO output when a prefix is simply empty, and 254/255 with an error
# message when the credentials or endpoint are wrong. Those are completely different diagnoses --
# "the backup never ran" vs "this verifier cannot see the bucket" -- so say which one is actually
# indicated instead of guessing at credentials and sending the reader down the wrong path. An
# error message that names the wrong culprit is what cost this project a week in September 2026.
ls_out=$(aws --endpoint-url "$R2_ENDPOINT" s3 ls "s3://$R2_BUCKET/fbm/" 2>&1); ls_rc=$?
if [ "$ls_rc" -ne 0 ] && [ -n "$ls_out" ]; then
  echo "FAIL: listing r2://$R2_BUCKET/fbm/ errored (rc=$ls_rc) -- this verifier cannot see the bucket."
  echo "  Check the R2 credentials/endpoint on THIS service. They are GitCellar's r2-backup-writer"
  echo "  key, so a rotation there breaks this job. Error was: $ls_out"
  echo FBM_VERIFY_FAIL; exit 1
fi
line=$(printf '%s\n' "$ls_out" | awk 'NF>=4{print $NF"\t"$3}' | sort | tail -1)
if [ -z "$line" ]; then
  echo "FAIL: no dumps at all in r2://$R2_BUCKET/fbm/ (the listing succeeded and was empty)."
  echo "  That means feedbackmonk-pg-backup has never successfully uploaded -- check THAT"
  echo "  service's logs, not this one's credentials."
  echo FBM_VERIFY_FAIL; exit 1
fi
key="fbm/$(echo "$line" | cut -f1)"; sz=$(echo "$line" | cut -f2)

stamp=$(basename "$key" | sed -nE 's/^feedbackmonk-([0-9]{8})-([0-9]{6}).*/\1 \2/p')
if [ -z "$stamp" ]; then echo "FAIL: unparseable key $key"; echo FBM_VERIFY_FAIL; exit 1; fi
d=${stamp% *}; t=${stamp#* }
e=$(date -u -d "${d:0:4}-${d:4:2}-${d:6:2} ${t:0:2}:${t:2:2}:${t:4:2}" +%s)
age=$(( ( $(date -u +%s) - e ) / 3600 ))
echo "latest=$key age=${age}h size=${sz}"
if [ "$age" -gt 26 ]; then echo "FAIL: stale ${age}h -- the nightly backup has not run"; F=1; fi
if [ "${sz:-0}" -lt 1024 ]; then echo "FAIL: tiny ${sz}B"; F=1; fi

cp_err=$(aws --endpoint-url "$R2_ENDPOINT" s3 cp "s3://$R2_BUCKET/$key" /tmp/v.gpg 2>&1); cp_rc=$?
if [ "$cp_rc" -ne 0 ]; then echo "FAIL: download rc=$cp_rc -- $cp_err"; echo FBM_VERIFY_FAIL; exit 1; fi
local_sz=$(stat -c%s /tmp/v.gpg 2>/dev/null || echo 0)
if [ "$local_sz" != "$sz" ]; then echo "FAIL: truncated download (local ${local_sz}B != listed ${sz}B) -- $key"; F=1; fi

lp_out=$(gpg --list-packets /tmp/v.gpg 2>/tmp/gpg.err)
if printf '%s' "$lp_out" | grep -qiE 'encrypted data|pubkey enc'; then
  echo "openpgp OK ($(printf '%s' "$lp_out" | grep -m1 -oE 'keyid [0-9A-F]+'))"
else
  echo "FAIL: bad openpgp -- gpg stderr: $(tr '\n' ' ' </tmp/gpg.err)"
  echo "  first-bytes: $(head -c 32 /tmp/v.gpg | od -An -tx1 | tr -s ' ')"
  F=1
fi
rm -f /tmp/v.gpg

if [ "$F" -ne 0 ]; then echo FBM_VERIFY_FAIL; exit 1; fi
echo FBM_VERIFY_PASS

# Dead-man's switch: ping ONLY on a real PASS. If this line is never reached, or the cron never
# fires at all, the heartbeat goes silent and alerts after its grace window -- which is the whole
# point, because a failed verify and a verify that never ran are the same silence.
# Inert until FBM_VERIFY_HEARTBEAT_URL is set; the URL is a secret-ish token so it lives in the
# service env, not here. Best-effort: a failed ping warns but does not fail the verify.
if [ -n "${FBM_VERIFY_HEARTBEAT_URL:-}" ]; then
  curl -fsS --max-time 15 "$FBM_VERIFY_HEARTBEAT_URL" >/dev/null 2>&1 && echo "heartbeat pinged" || echo "WARN: heartbeat ping failed (verify still PASSED)"
else
  echo "NOTE: FBM_VERIFY_HEARTBEAT_URL unset -- artifact is verified daily, but NOTHING alerts if this cron stops running"
fi
