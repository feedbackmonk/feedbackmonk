#!/usr/bin/env bash
# provision-gitcellar.sh — one-time provisioning of the GitCellar tenant +
# project + Ed25519 signing key against a running feedbackmonk instance.
#
# Implements the procedure in docs/integrations/gitcellar-adoption.md §3 and is
# invoked at the end of docs/operations/RAILWAY_GITCELLAR.md §6.
#
# Flow: signup → verify-email → create project → register signing key.
# The verify-email step needs the token feedbackmonk emailed at signup; retrieve
# it from your mail provider (or Mailpit in dev) and pass it in.
#
# Usage:
#   API_BASE=https://feedback.gitcellar.com \
#   OPS_EMAIL=ops@gitcellar.com OPS_PASSWORD='<strong>' \
#   PUBLIC_KEY_B64='<base64 32-byte ed25519 pubkey>' \
#   VERIFY_TOKEN='<token-from-email>' \
#     ./scripts/provision-gitcellar.sh
#
# If you don't yet have a keypair, generate one (GitCellar keeps the PRIVATE key
# to mint Desktop JWTs; only the public key is registered):
#   openssl genpkey -algorithm ed25519 -out gitcellar_fbm_priv.pem
#   openssl pkey -in gitcellar_fbm_priv.pem -pubout -outform DER | tail -c 32 | base64
#
# Two-phase run: omit VERIFY_TOKEN on the first run to do signup only; it prints
# where to find the token, then re-run with VERIFY_TOKEN set to finish.
set -euo pipefail

API_BASE="${API_BASE:?set API_BASE, e.g. https://feedback.gitcellar.com}"
OPS_EMAIL="${OPS_EMAIL:?set OPS_EMAIL}"
OPS_PASSWORD="${OPS_PASSWORD:?set OPS_PASSWORD}"
PROJECT_NAME="${PROJECT_NAME:-GitCellar}"
PROJECT_SLUG="${PROJECT_SLUG:-gitcellar}"
KEY_LABEL="${KEY_LABEL:-gitcellar-desktop-$(date +%Y%m)}"
COOKIE_JAR="$(mktemp)"
trap 'rm -f "$COOKIE_JAR"' EXIT

req() { curl -fsS -H "Content-Type: application/json" "$@"; }

echo "==> [1/4] signup ($OPS_EMAIL) at $API_BASE"
SIGNUP=$(req -X POST "$API_BASE/api/v1/signup" \
  -d "{\"email\":\"$OPS_EMAIL\",\"password\":\"$OPS_PASSWORD\"}" || true)
echo "    $SIGNUP"
echo "    (409 = tenant already exists; that's fine, continue to verify.)"

if [ -z "${VERIFY_TOKEN:-}" ]; then
  cat <<EOF

==> STOP: VERIFY_TOKEN not provided.
    feedbackmonk emailed a verify-email token to $OPS_EMAIL.
    Retrieve it (your SMTP provider's inbox, or Mailpit web UI in dev),
    then re-run this script with VERIFY_TOKEN=<token> set.
EOF
  exit 0
fi

echo "==> [2/4] verify-email (redeem token → admin session cookie)"
req -c "$COOKIE_JAR" -X POST "$API_BASE/api/v1/verify-email" \
  -d "{\"token\":\"$VERIFY_TOKEN\"}" >/dev/null
echo "    verified; session cookie captured."

echo "==> [3/4] create project ($PROJECT_NAME / $PROJECT_SLUG)"
PROJECT=$(req -b "$COOKIE_JAR" -X POST "$API_BASE/api/v1/projects" \
  -d "{\"name\":\"$PROJECT_NAME\",\"slug\":\"$PROJECT_SLUG\"}")
PROJECT_ID=$(printf '%s' "$PROJECT" | python3 -c 'import sys,json;print(json.load(sys.stdin)["project_id"])')
echo "    project_id = $PROJECT_ID"

if [ -z "${PUBLIC_KEY_B64:-}" ]; then
  cat <<EOF

==> project created, but PUBLIC_KEY_B64 not provided — skipping key registration.
    Generate a keypair (GitCellar keeps the private key) and re-run [4/4] with
    PUBLIC_KEY_B64 set, or register manually per integration contract §3.3.

    project_id=$PROJECT_ID   ← paste into docs/integrations/gitcellar-adoption.md §3.2
EOF
  exit 0
fi

echo "==> [4/4] register Ed25519 signing key ($KEY_LABEL)"
KEY=$(req -b "$COOKIE_JAR" -X POST "$API_BASE/api/v1/projects/$PROJECT_ID/signing-keys" \
  -d "{\"public_key_base64\":\"$PUBLIC_KEY_B64\",\"label\":\"$KEY_LABEL\"}")
KEY_ID=$(printf '%s' "$KEY" | python3 -c 'import sys,json;print(json.load(sys.stdin)["key_id"])')

cat <<EOF

==> DONE.
    tenant:     $OPS_EMAIL
    project_id: $PROJECT_ID
    key_id:     $KEY_ID   (label: $KEY_LABEL)

    NEXT: paste project_id into docs/integrations/gitcellar-adoption.md §3.2
          and flip the contract Status to ACTIVE. GitCellar mints EdDSA JWTs
          with aud=$PROJECT_ID using the matching PRIVATE key (contract §5).
EOF
