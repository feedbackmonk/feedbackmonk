#!/usr/bin/env bash
# feedbackmonk P1 end-to-end curl pipeline -- the P1-exit-gate "closes the loop" witness.
#
# Extends e2e-p0-curl.sh with the status-workflow + admin reply pipeline that
# P1 ships (Stage 2 backend + Stage 2 frontend + Stage 3 witness):
#
#   1. signup
#   2. verify-email   (issues feedbackmonk_session cookie; doubles as admin-login
#                      since no separate /api/v1/admin/login endpoint exists --
#                      per Contract C11, the verify-email cookie IS the admin
#                      session)
#   3. project-create
#   4. signing-key register
#   5. JWT-mode feedback submission (P0 carry-forward; gets the FB-XXXXXX id)
#   6. admin-list contains the submitted FB-id
#   7. admin-transition (submitted -> triaged)
#   8. poll Mailpit for status-change email
#   9. admin-reply (visibility=public)
#  10. poll Mailpit for public-reply email
#
# ALL steps must PASS; the script exits non-zero on any failure. Mailpit
# assertions skip gracefully if Mailpit is unreachable, but the email-emit
# *behavior* still executes server-side.
#
# Pre-conditions:
#   - Postgres dev container on localhost:5433 (DATABASE_URL set in API env)
#   - feedbackmonk-api binary on http://127.0.0.1:14304
#   - Mailpit dev (HTTP API :8025 + SMTP :1025) -- optional but recommended
#   - jq, curl, openssl 3+ on PATH
#
# Usage:
#   bash scripts/e2e-p1-curl.sh
#
# Exit codes:
#   0 = full pipeline PASS
#   1 = a step failed (see PASS/FAIL log)
#   2 = pre-condition missing (jq / curl / openssl / API not reachable)

set -euo pipefail

API_BASE="${FEEDBACKMONK_API_BASE:-http://127.0.0.1:14304}"
MAILPIT_BASE="${MAILPIT_BASE:-http://127.0.0.1:8025}"
TEST_EMAIL="e2e-p1-$(date +%s)@example.com"
SUBMITTER_EMAIL="submitter-p1-$(date +%s)@example.com"
TEST_PASSWORD="correct horse battery staple 9!"
WORK_DIR="$(mktemp -d -t feedbackmonk-e2e-p1.XXXXXX)"
COOKIE_JAR="$WORK_DIR/cookies.txt"

log()  { printf '[%s] %s\n' "$(date +%H:%M:%S)" "$*"; }
pass() { log "PASS: $1"; }
fail() { log "FAIL: $1"; exit 1; }

# ---------- pre-flight ------------------------------------------------------

command -v curl    >/dev/null || { log "missing dep: curl";    exit 2; }
command -v jq      >/dev/null || { log "missing dep: jq";      exit 2; }
command -v openssl >/dev/null || { log "missing dep: openssl"; exit 2; }
curl --silent --fail "$API_BASE/health" >/dev/null \
    || { log "API not reachable at $API_BASE/health"; exit 2; }

MAILPIT_READY=0
if curl --silent --fail "$MAILPIT_BASE/api/v1/messages?limit=1" >/dev/null 2>&1; then
    MAILPIT_READY=1
fi
if [ "$MAILPIT_READY" = "0" ]; then
    log "WARN: Mailpit not reachable at $MAILPIT_BASE -- email-emit assertions will skip;"
    log "      server-side emit still exercised; SMTP failures logged but not surfaced."
fi

log "API: $API_BASE | Mailpit: $MAILPIT_BASE ($([ "$MAILPIT_READY" = "1" ] && echo ready || echo skipped)) | work: $WORK_DIR"

# Helper: poll Mailpit for a message whose subject matches the regex AND is
# addressed to the given recipient. Returns 0 + prints the message ID on
# success, 1 on timeout. Skips with 2 if Mailpit not ready.
poll_mailpit_for_subject() {
    local recipient="$1"
    local subject_re="$2"
    local timeout="${3:-15}"
    [ "$MAILPIT_READY" = "1" ] || return 2

    local msg_id="" i
    for (( i=0; i<timeout; i++ )); do
        msg_id="$(curl -sS "$MAILPIT_BASE/api/v1/messages?limit=50" \
            | jq -r --arg em "$recipient" --arg sre "$subject_re" \
                '[.messages[] | select(.To[0].Address==$em) | select(.Subject|test($sre))] | .[0].ID // empty')"
        [ -n "$msg_id" ] && break
        sleep 1
    done
    [ -n "$msg_id" ] || return 1
    printf '%s' "$msg_id"
    return 0
}

# ---------- step 1: signup --------------------------------------------------

log "step 1: POST /api/v1/signup"
SIGNUP_RESP="$(curl -sS -X POST "$API_BASE/api/v1/signup" \
    -H 'Content-Type: application/json' \
    -d "{\"email\":\"$TEST_EMAIL\",\"password\":\"$TEST_PASSWORD\"}")"
echo "$SIGNUP_RESP" | jq . > "$WORK_DIR/signup.json"
echo "$SIGNUP_RESP" | jq -e '.tenant_id' >/dev/null || fail "signup did not return tenant_id"
pass "step 1 -- tenant created"

# ---------- step 2: verify-email (issues feedbackmonk_session cookie) ----------

log "step 2: read verify-email token + POST /api/v1/verify-email (Contract C11 admin-session cookie)"
if [ "$MAILPIT_READY" = "1" ]; then
    MSG_ID=""
    for _ in 1 2 3 4 5 6 7 8 9 10; do
        MSG_ID="$(curl -sS "$MAILPIT_BASE/api/v1/messages" \
            | jq -r --arg em "$TEST_EMAIL" '.messages[] | select(.To[0].Address==$em) | .ID' \
            | head -n 1)"
        [ -n "$MSG_ID" ] && break
        sleep 1
    done
    [ -n "$MSG_ID" ] || fail "no verify-email in Mailpit for $TEST_EMAIL after 10s"

    VERIFY_TOKEN="$(curl -sS "$MAILPIT_BASE/api/v1/message/$MSG_ID" \
        | jq -r '.Text' \
        | grep -Eo 'token=[A-Za-z0-9_-]+' \
        | head -n 1 \
        | sed 's/token=//')"
    [ -n "$VERIFY_TOKEN" ] || fail "could not extract verify token from message $MSG_ID"
else
    fail "step 2 requires Mailpit (cannot fetch verify token); start Mailpit and retry"
fi
log "  token: ${VERIFY_TOKEN:0:8}..."

VERIFY_RESP="$(curl -sS -i -X POST "$API_BASE/api/v1/verify-email" \
    -H 'Content-Type: application/json' \
    -c "$COOKIE_JAR" \
    -d "{\"token\":\"$VERIFY_TOKEN\"}")"
echo "$VERIFY_RESP" | grep -qi 'HTTP/1.1 200\|HTTP/2 200' || fail "verify-email did not return 200"
grep -q feedbackmonk_session "$COOKIE_JAR" \
    || fail "verify-email did not set feedbackmonk_session cookie (Contract C11)"
pass "step 2 -- verify-email OK; feedbackmonk_session admin cookie set"

# ---------- step 3: create project ------------------------------------------

log "step 3: POST /api/v1/projects"
PROJECT_RESP="$(curl -sS -X POST "$API_BASE/api/v1/projects" \
    -H 'Content-Type: application/json' \
    -b "$COOKIE_JAR" \
    -d '{"name":"P1 E2E Project","slug":"p1-e2e"}')"
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
    -d "{\"public_key_b64\":\"$PUB_B64\",\"label\":\"p1-e2e-key\"}")"
echo "$KEY_RESP" | jq . > "$WORK_DIR/signing_key.json"
echo "$KEY_RESP" | jq -e '.signing_key_id // .key_id' >/dev/null \
    || fail "signing-key register did not return signing_key_id/key_id (got: $KEY_RESP)"
pass "step 4 -- signing key registered"

# ---------- step 5: mint JWT + JWT-mode submission (P0 carry-forward) -------

log "step 5: mint JWT + POST /api/v1/projects/$PROJECT_ID/feedback (auth mode, with submitter email)"
NOW="$(date +%s)"
EXP="$((NOW + 300))"
HEADER_B64="$(printf '%s' '{"alg":"EdDSA","typ":"JWT"}' | base64 -w 0 | tr '+/' '-_' | tr -d '=')"
PAYLOAD="$(jq -nc \
    --arg sub "p1-e2e-user" \
    --arg aud "$PROJECT_ID" \
    --arg email "$SUBMITTER_EMAIL" \
    --argjson iat "$NOW" \
    --argjson exp "$EXP" \
    '{sub:$sub, aud:$aud, iat:$iat, exp:$exp, email:$email, name:"P1 E2E"}')"
PAYLOAD_B64="$(printf '%s' "$PAYLOAD" | base64 -w 0 | tr '+/' '-_' | tr -d '=')"
SIGNING_INPUT="$HEADER_B64.$PAYLOAD_B64"
printf '%s' "$SIGNING_INPUT" > "$WORK_DIR/signing-input.bin"
openssl pkeyutl -sign -inkey "$WORK_DIR/keys/ed25519_private.pem" \
    -rawin -in "$WORK_DIR/signing-input.bin" \
    -out "$WORK_DIR/sig.bin"
SIG_B64="$(base64 -w 0 < "$WORK_DIR/sig.bin" | tr '+/' '-_' | tr -d '=')"
JWT="$SIGNING_INPUT.$SIG_B64"

SUBMIT_RESP="$(curl -sS -X POST "$API_BASE/api/v1/projects/$PROJECT_ID/feedback" \
    -H 'Content-Type: application/json' \
    -H "Authorization: Bearer $JWT" \
    -d '{"body":"P1 closes-the-loop test feedback body","kind":"bug"}')"
echo "$SUBMIT_RESP" | jq . > "$WORK_DIR/submit_auth.json"
FB_ID="$(echo "$SUBMIT_RESP" | jq -r '.feedback_id')"
[[ "$FB_ID" == FB-* ]] || fail "auth-mode submit did not return FB-XXXXXX (got: $FB_ID)"
log "  feedback_id: $FB_ID"
pass "step 5 -- JWT-mode submission accepted ($FB_ID)"

# ---------- step 6: admin list contains FB-id -------------------------------

log "step 6: GET /api/v1/admin/feedback (admin session) -- list must contain $FB_ID"
LIST_RESP="$(curl -sS "$API_BASE/api/v1/admin/feedback?limit=50" -b "$COOKIE_JAR")"
echo "$LIST_RESP" | jq . > "$WORK_DIR/admin_list.json"
echo "$LIST_RESP" | jq -e --arg id "$FB_ID" '.items[] | select(.feedback_id==$id)' >/dev/null \
    || fail "admin list did not contain $FB_ID (got: $(echo "$LIST_RESP" | jq -c '.items|map(.feedback_id)'))"
pass "step 6 -- admin list contains $FB_ID"

# ---------- step 7: admin transition submitted -> triaged -------------------

log "step 7: POST /api/v1/admin/feedback/$FB_ID/transition (-> triaged)"
TRANS_RESP="$(curl -sS -X POST "$API_BASE/api/v1/admin/feedback/$FB_ID/transition" \
    -H 'Content-Type: application/json' \
    -b "$COOKIE_JAR" \
    -d '{"to_status":"triaged","reason_note":"P1 e2e witness triage step"}')"
echo "$TRANS_RESP" | jq . > "$WORK_DIR/admin_transition.json"
TO_STATUS="$(echo "$TRANS_RESP" | jq -r '.to_status')"
[ "$TO_STATUS" = "triaged" ] || fail "transition did not return to_status=triaged (got: $TRANS_RESP)"
EMAIL_QUEUED_TX="$(echo "$TRANS_RESP" | jq -r '.email_queued')"
log "  email_queued (transition): $EMAIL_QUEUED_TX"
pass "step 7 -- transition to triaged committed"

# ---------- step 8: poll Mailpit for status-change email --------------------

log "step 8: poll Mailpit for status-change email to $SUBMITTER_EMAIL"
if [ "$MAILPIT_READY" = "1" ]; then
    # Subject shape (templates.rs): "[<prefix> #<FB-id>] Status updated: Triaged"
    if STATUS_MSG_ID="$(poll_mailpit_for_subject "$SUBMITTER_EMAIL" "Status updated: Triaged" 15)"; then
        log "  status-change msg id: $STATUS_MSG_ID"
        STATUS_BODY="$(curl -sS "$MAILPIT_BASE/api/v1/message/$STATUS_MSG_ID" | jq -r '.Text')"
        echo "$STATUS_BODY" | grep -q "$FB_ID" \
            || fail "status-change email body does not reference $FB_ID"
        pass "step 8 -- status-change email delivered (subject + body OK)"
    else
        fail "no status-change email in Mailpit for $SUBMITTER_EMAIL within 15s"
    fi
else
    log "  Mailpit not ready -- skipping email-arrival assertion (server-side emit still exercised)"
    pass "step 8 -- skipped (Mailpit unreachable)"
fi

# ---------- step 9: admin reply (visibility=public) -------------------------

log "step 9: POST /api/v1/admin/feedback/$FB_ID/reply (public)"
REPLY_RESP="$(curl -sS -X POST "$API_BASE/api/v1/admin/feedback/$FB_ID/reply" \
    -H 'Content-Type: application/json' \
    -b "$COOKIE_JAR" \
    -d '{"body":"Thanks -- we have reproduced this and are pushing a fix.","visibility":"public"}')"
echo "$REPLY_RESP" | jq . > "$WORK_DIR/admin_reply.json"
echo "$REPLY_RESP" | jq -e '.reply_id' >/dev/null \
    || fail "reply did not return reply_id (got: $REPLY_RESP)"
EMAIL_QUEUED_RP="$(echo "$REPLY_RESP" | jq -r '.email_queued')"
log "  email_queued (reply): $EMAIL_QUEUED_RP"
pass "step 9 -- public reply committed"

# ---------- step 10: poll Mailpit for public-reply email --------------------

log "step 10: poll Mailpit for public-reply email to $SUBMITTER_EMAIL"
if [ "$MAILPIT_READY" = "1" ]; then
    if REPLY_MSG_ID="$(poll_mailpit_for_subject "$SUBMITTER_EMAIL" "Reply from the team" 15)"; then
        log "  public-reply msg id: $REPLY_MSG_ID"
        REPLY_BODY="$(curl -sS "$MAILPIT_BASE/api/v1/message/$REPLY_MSG_ID" | jq -r '.Text')"
        echo "$REPLY_BODY" | grep -q "$FB_ID" \
            || fail "public-reply email body does not reference $FB_ID"
        echo "$REPLY_BODY" | grep -q "pushing a fix" \
            || fail "public-reply email body does not include reply text"
        pass "step 10 -- public-reply email delivered (subject + body OK)"
    else
        fail "no public-reply email in Mailpit for $SUBMITTER_EMAIL within 15s"
    fi
else
    log "  Mailpit not ready -- skipping email-arrival assertion (server-side emit still exercised)"
    pass "step 10 -- skipped (Mailpit unreachable)"
fi

# ---------- done ------------------------------------------------------------

log "ALL STEPS PASSED. Witness artefacts: $WORK_DIR"
echo "  $WORK_DIR/signup.json"
echo "  $WORK_DIR/project.json"
echo "  $WORK_DIR/signing_key.json"
echo "  $WORK_DIR/submit_auth.json"
echo "  $WORK_DIR/admin_list.json"
echo "  $WORK_DIR/admin_transition.json"
echo "  $WORK_DIR/admin_reply.json"
