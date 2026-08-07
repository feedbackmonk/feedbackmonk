#!/bin/bash
# cloud-sync-coordination-hazard oracle (Unix / Git Bash)
#
# Answers: is this project's coordination store sitting inside a cloud-synced
# folder, where a background sync client can silently discard a write that the
# mkdir lock believed it had serialized?
#
# WHY THIS EXISTS (DISC-CSI-25 / DEFER-016 / DEC-188)
# registry-write.{sh,ps1}'s lock serializes processes against ONE machine's
# local filesystem view. It has no leverage over what happens AFTER a write
# returns success: on synced storage two near-concurrent writers can each write
# and read back their own version locally, and the sync client then picks one
# version as canonical and discards the other -- after the fact, below the lock.
# Observed live 2026-07-12: two writers appended to grant-requests.jsonl ~2
# minutes apart, both verified written at write-time, one record survived.
#
# This oracle does not fix that. It makes the exposure VISIBLE every session, so
# a latent hazard becomes a decision. Silence means "no recognized provider
# segment" -- never "verified safe".
#
# Output: single-line JSON. Empty briefing => session-start suppresses the line.

set -uo pipefail

_json_escape() {
    printf '%s' "$1" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g'
}

emit() {
    printf '{"hosted":%s,"provider":%s,"root":%s,"matched_segment":%s,"at_risk_paths":[%s],"briefing":"%s"}\n' \
        "$1" "$2" "$3" "$4" "$5" "$6"
}

# --- Resolve the project root -------------------------------------------------
ROOT=""
if ROOT_TRY="$(git rev-parse --show-toplevel 2>/dev/null)"; then
    ROOT="$ROOT_TRY"
fi
[ -z "$ROOT" ] && ROOT="$(pwd 2>/dev/null || true)"

if [ -z "$ROOT" ]; then
    # Graceful absence: we could not resolve a root. Say nothing rather than
    # guess -- a false "clean" and a false "hosted" are both worse than silence.
    emit false null null null "" ""
    exit 0
fi

# On Git Bash the POSIX view (/s/SourceControlled/...) hides a Windows path that
# may contain the provider segment, so prefer the Windows form when available.
WINROOT=""
if command -v cygpath >/dev/null 2>&1; then
    WINROOT="$(cygpath -w "$ROOT" 2>/dev/null || true)"
fi
INSPECT="$ROOT"
[ -n "$WINROOT" ] && INSPECT="$WINROOT"

# --- Provider list (env > project config > defaults) ---------------------------
PROVIDERS_RAW="${ULDF_CLOUD_SYNC_PROVIDERS:-}"
if [ -z "$PROVIDERS_RAW" ] && [ -f "$ROOT/.claude/config.json" ]; then
    PROVIDERS_RAW="$(grep -o '"providers"[[:space:]]*:[[:space:]]*\[[^]]*\]' "$ROOT/.claude/config.json" 2>/dev/null \
        | sed -e 's/.*\[//' -e 's/\]//' -e 's/"//g' -e 's/[[:space:]]*,[[:space:]]*/,/g' || true)"
fi
[ -z "$PROVIDERS_RAW" ] && PROVIDERS_RAW="OneDrive,Dropbox,Google Drive,GoogleDrive,iCloud Drive,iCloudDrive"

# --- Match a provider segment -------------------------------------------------
# Segment-aware: match the provider as a PATH SEGMENT, so a project merely named
# e.g. "OneDriveTools" does not trip. Case-insensitive (Windows paths vary).
HOSTED=false
PROVIDER=null
SEGMENT=null

_lower() { printf '%s' "$1" | tr '[:upper:]' '[:lower:]'; }
NORM="$(_lower "$(printf '%s' "$INSPECT" | tr '\\' '/')")"

OLDIFS="$IFS"; IFS=','
for prov in $PROVIDERS_RAW; do
    prov="$(printf '%s' "$prov" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')"
    [ -z "$prov" ] && continue
    provl="$(_lower "$prov")"
    case "/$NORM/" in
        */"$provl"/*)
            HOSTED=true
            PROVIDER="\"$(_json_escape "$prov")\""
            SEGMENT="\"$(_json_escape "$prov")\""
            break
            ;;
    esac
done
IFS="$OLDIFS"

if [ "$HOSTED" != "true" ]; then
    emit false null "\"$(_json_escape "$INSPECT")\"" null "" ""
    exit 0
fi

# --- Which coordination surfaces actually exist under the hosted root? --------
AT_RISK=""
for rel in \
    ".claude/collaboration/active-sessions.json" \
    ".claude/session-state" \
    ".claude/collaboration"
do
    if [ -e "$ROOT/$rel" ]; then
        [ -n "$AT_RISK" ] && AT_RISK="$AT_RISK,"
        AT_RISK="$AT_RISK\"$(_json_escape "$rel")\""
    fi
done

PROV_PLAIN="$(printf '%s' "$PROVIDER" | tr -d '"')"
BRIEF="coordination store is inside $PROV_PLAIN -- concurrent writes to active-sessions.json / touches.json / channels can be silently reconciled away below the lock (DISC-CSI-25). Prefer a local path for this project, or avoid concurrent PODS work here."

emit true "$PROVIDER" "\"$(_json_escape "$INSPECT")\"" "$SEGMENT" "$AT_RISK" "$(_json_escape "$BRIEF")"
exit 0
