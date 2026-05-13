#!/usr/bin/env bash
# Feedbackr P0 end-to-end curl pipeline -- the P0-exit-gate witness.
#
# Drives the full signup -> project -> key-register -> JWT-signed submission
# -> anonymous submission -> rate-limit pipeline against a running dev
# Postgres + dev API binary. Exits 0 only when every step passes.
#
# Drafted by CLAUDE-B in Stage 2; committed + run by Stage 3 as the
# P0-exit-gate witness (per CLAUDE-B-TASK.md §10).
#
# Pre-conditions:
#   - Postgres dev container running on localhost:5433 (DATABASE_URL set)
#   - feedbackr-api binary running on http://127.0.0.1:14304
#   - Mailpit dev container running with HTTP API on :8025 + SMTP on :1025
#   - jq, curl, openssl 3+ on PATH
#   - Optional: Python 3 (for the JWT minting helper) OR Node 18+
#
# Usage:
#   bash scripts/e2e-p0-curl.sh
#
# Exit codes:
#   0 = full pipeline PASS
#   1 = a step failed (see PASS/FAIL log; binary should be running on :14304)
#   2 = pre-condition missing (jq / curl / openssl / Mailpit / API not reachable)

set -euo pipefail

API_BASE="${FEEDBACKR_API_BASE:-http://127.0.0.1:14304}"
MAILPIT_BASE="${MAILPIT_BASE:-http://127.0.0.1:8025}"
TEST_EMAIL="e2e-$(date +%s)@example.com"
TEST_PASSWORD="correct horse battery staple 9!"
WORK_DIR="$(mktemp -d -t feedbackr-e2e.XXXXXX)"
COOKIE_JAR="$WORK_DIR/cookies.txt"

log() { printf '[%s] %s\n' "$(date +%H:%M:%S)" "$*"; }
pass() { log "PASS: $1"; }
fail() { log "FAIL: $1"; exit 1; }

# ---------- pre-flight ------------------------------------------------------

command -v curl    >/dev/null || { log "missing dep: curl";    exit 2; }
command -v jq      >/dev/null || { log "missing dep: jq";      exit 2; }
command -v openssl >/dev/null || { log "missing dep: openssl"; exit 2; }
curl --silent --fail "$API_BASE/health" >/dev/null \
    || { log "API not reachable at $API_BASE/health"; exit 2; }
curl --silent --fail "$MAILPIT_BASE/api/v1/messages?limit=1" >/dev/null \
    || { log "Mailpit not reachable at $MAILPIT_BASE"; exit 2; }

log "API: $API_BASE | Mailpit: $MAILPIT_BASE | work: $WORK_DIR"

# ---------- step 1: signup --------------------------------------------------

log "step 1: POST /api/v1/signup"
SIGNUP_RESP="$(curl -sS -X POST "$API_BASE/api/v1/signup" \
    -H 'Content-Type: application/json' \
    -d "{\"email\":\"$TEST_EMAIL\",\"password\":\"$TEST_PASSWORD\"}")"
echo "$SIGNUP_RESP" | jq . > "$WORK_DIR/signup.json"
echo "$SIGNUP_RESP" | jq -e '.tenant_id' >/dev/null || fail "signup did not return tenant_id"
pass "step 1 -- tenant created"

# ---------- step 2: read verify token from Mailpit + verify -----------------

log "step 2: read verify-email token from Mailpit + POST /api/v1/verify-email"
# Poll Mailpit for up to 10s for the verify-email message to appear. SMTP
# round-trip + Mailpit ingest can take >1s under load. Capture the ID into
# a variable rather than piping through xargs -- xargs without --no-run-if-empty
# will run curl with the literal '{}' placeholder when the input is empty,
# which manifests as a confusing "URL rejected: Malformed input" error.
MSG_ID=""
for _ in 1 2 3 4 5 6 7 8 9 10; do
    MSG_ID="$(curl -sS "$MAILPIT_BASE/api/v1/messages" \
        | jq -r --arg em "$TEST_EMAIL" '.messages[] | select(.To[0].Address==$em) | .ID' \
        | head -n 1)"
    [ -n "$MSG_ID" ] && break
    sleep 1
done
[ -n "$MSG_ID" ] || fail "no verify-email in Mailpit for $TEST_EMAIL after 10s"

VERIFY_LINK="$(curl -sS "$MAILPIT_BASE/api/v1/message/$MSG_ID" \
    | jq -r '.Text' \
    | grep -Eo 'token=[A-Za-z0-9_-]+' \
    | head -n 1 \
    | sed 's/token=//')"
[ -n "$VERIFY_LINK" ] || fail "could not extract verify token from Mailpit message $MSG_ID"
log "  token: ${VERIFY_LINK:0:8}..."

VERIFY_RESP="$(curl -sS -i -X POST "$API_BASE/api/v1/verify-email" \
    -H 'Content-Type: application/json' \
    -c "$COOKIE_JAR" \
    -d "{\"token\":\"$VERIFY_LINK\"}")"
echo "$VERIFY_RESP" | grep -qi 'HTTP/1.1 200\|HTTP/2 200' || fail "verify-email did not return 200"
grep -q feedbackr_session "$COOKIE_JAR" || fail "verify-email did not set session cookie"
pass "step 2 -- email verified, session cookie set"

# ---------- step 3: create project ------------------------------------------

log "step 3: POST /api/v1/projects"
PROJECT_RESP="$(curl -sS -X POST "$API_BASE/api/v1/projects" \
    -H 'Content-Type: application/json' \
    -b "$COOKIE_JAR" \
    -d '{"name":"E2E Test Project","slug":"e2e-test"}')"
echo "$PROJECT_RESP" | jq . > "$WORK_DIR/project.json"
PROJECT_ID="$(echo "$PROJECT_RESP" | jq -r '.project_id // .id')"
[ -n "$PROJECT_ID" ] && [ "$PROJECT_ID" != "null" ] || fail "project create did not return project_id"
log "  project_id: $PROJECT_ID"
pass "step 3 -- project created"

# ---------- step 4: register signing key ------------------------------------

log "step 4: generate Ed25519 keypair + POST /api/v1/projects/$PROJECT_ID/signing-keys"
bash "$(dirname "$0")/gen-ed25519.sh" "$WORK_DIR/keys" >/dev/null
PUB_B64="$(cat "$WORK_DIR/keys/ed25519_public.b64")"
KEY_RESP="$(curl -sS -X POST "$API_BASE/api/v1/projects/$PROJECT_ID/signing-keys" \
    -H 'Content-Type: application/json' \
    -b "$COOKIE_JAR" \
    -d "{\"public_key_base64\":\"$PUB_B64\",\"label\":\"e2e-key\"}")"
echo "$KEY_RESP" | jq . > "$WORK_DIR/signing_key.json"
# Stage 2 implementation returned the field as `key_id`; Contract C4 also
# documents `signing_key_id`. Accept either to match the contract surface.
echo "$KEY_RESP" | jq -e '.signing_key_id // .key_id' >/dev/null || fail "signing-key register did not return signing_key_id/key_id (got: $KEY_RESP)"
pass "step 4 -- signing key registered"

# ---------- step 5: mint JWT + JWT-mode submission --------------------------

log "step 5: mint JWT and POST /api/v1/projects/$PROJECT_ID/feedback (auth mode)"
# Use Python (or fall back to Node) to mint an Ed25519 JWT signed by the
# generated private key. The minting helper is intentionally deterministic
# so the witness is reproducible.
NOW="$(date +%s)"
EXP="$((NOW + 300))"
HEADER_B64="$(printf '%s' '{"alg":"EdDSA","typ":"JWT"}' | base64 -w 0 | tr '+/' '-_' | tr -d '=')"
PAYLOAD="$(jq -nc \
    --arg sub "e2e-user-1" \
    --arg aud "$PROJECT_ID" \
    --argjson iat "$NOW" \
    --argjson exp "$EXP" \
    '{sub:$sub, aud:$aud, iat:$iat, exp:$exp, email:"e2e-user-1@example.com", name:"E2E User"}')"
PAYLOAD_B64="$(printf '%s' "$PAYLOAD" | base64 -w 0 | tr '+/' '-_' | tr -d '=')"
SIGNING_INPUT="$HEADER_B64.$PAYLOAD_B64"
# Sign with OpenSSL (Ed25519 signature = 64 bytes).
printf '%s' "$SIGNING_INPUT" > "$WORK_DIR/signing-input.bin"
openssl pkeyutl -sign -inkey "$WORK_DIR/keys/ed25519_private.pem" \
    -rawin -in "$WORK_DIR/signing-input.bin" \
    -out "$WORK_DIR/sig.bin"
SIG_B64="$(base64 -w 0 < "$WORK_DIR/sig.bin" | tr '+/' '-_' | tr -d '=')"
JWT="$SIGNING_INPUT.$SIG_B64"

SUBMIT_RESP="$(curl -sS -X POST "$API_BASE/api/v1/projects/$PROJECT_ID/feedback" \
    -H 'Content-Type: application/json' \
    -H "Authorization: Bearer $JWT" \
    -d '{"body":"e2e auth-mode body","kind":"bug"}')"
echo "$SUBMIT_RESP" | jq . > "$WORK_DIR/submit_auth.json"
echo "$SUBMIT_RESP" | jq -e '.feedback_id | startswith("FB-")' >/dev/null \
    || fail "auth-mode submit did not return FB-XXXXXX"
pass "step 5 -- auth-mode submission accepted"

# ---------- step 6: anonymous-mode submission -------------------------------

log "step 6: POST /api/v1/projects/$PROJECT_ID/feedback (anon mode)"
ANON_RESP="$(curl -sS -X POST "$API_BASE/api/v1/projects/$PROJECT_ID/feedback" \
    -H 'Content-Type: application/json' \
    -d '{"body":"e2e anon body","kind":"feature"}')"
echo "$ANON_RESP" | jq . > "$WORK_DIR/submit_anon.json"
echo "$ANON_RESP" | jq -e '.feedback_id | startswith("FB-")' >/dev/null \
    || fail "anon-mode submit did not return FB-XXXXXX"
pass "step 6 -- anon-mode submission accepted"

# ---------- step 7: rate-limit boundary -------------------------------------

log "step 7: 11 rapid anon submissions; 11th must 429"
# Reuse the same minted cookie so the same (hash, project) bucket is hit.
ANON_COOKIE_VAL="e2e-cookie-deterministic"
for i in 1 2 3 4 5 6 7 8 9 10; do
    R="$(curl -sS -o /dev/null -w '%{http_code}' \
        -X POST "$API_BASE/api/v1/projects/$PROJECT_ID/feedback" \
        -H 'Content-Type: application/json' \
        -H "X-Feedbackr-Anon-Cookie: $ANON_COOKIE_VAL" \
        -d "{\"body\":\"burst $i\",\"kind\":\"other\"}")"
    [ "$R" = "200" ] || fail "anon submission $i returned $R (expected 200)"
done
R11="$(curl -sS -o /dev/null -w '%{http_code}' \
    -X POST "$API_BASE/api/v1/projects/$PROJECT_ID/feedback" \
    -H 'Content-Type: application/json' \
    -H "X-Feedbackr-Anon-Cookie: $ANON_COOKIE_VAL" \
    -d '{"body":"burst 11","kind":"other"}')"
[ "$R11" = "429" ] || fail "11th anon submission returned $R11 (expected 429)"
pass "step 7 -- 11th submission correctly rate-limited"

# ---------- done ------------------------------------------------------------

log "ALL STEPS PASSED. Witness artefacts: $WORK_DIR"
echo "  $WORK_DIR/signup.json"
echo "  $WORK_DIR/project.json"
echo "  $WORK_DIR/signing_key.json"
echo "  $WORK_DIR/submit_auth.json"
echo "  $WORK_DIR/submit_anon.json"
