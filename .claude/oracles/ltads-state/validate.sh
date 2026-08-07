#!/bin/bash
# ltads-state oracle self-test (Unix)
# Legs: (1) live-project run emits valid JSON with the full field set (pre-ARC
# fields preserved + ARC-01/06 additive fields); (2-5) fixture verdicts:
# valid/CONTINUATION, concluded/FRESH, malformed/invalid, prose-only/legacy.
set -e
ORACLE_DIR="$(cd "$(dirname "$0")" && pwd)"
OUTPUT="$(bash "$ORACLE_DIR/run.sh" 2>&1)" || { echo "FAIL: run.sh exited non-zero" >&2; exit 1; }

json_ok() {
    if command -v jq >/dev/null 2>&1; then
        printf '%s' "$1" | jq -e . >/dev/null 2>&1 && return 0
        return 1
    fi
    printf '%s' "$1" | python3 -c "import sys, json; json.load(sys.stdin)" 2>/dev/null && return 0
    printf '%s' "$1" | python -c "import sys, json; json.load(sys.stdin)" 2>/dev/null && return 0
    return 1
}

if ! json_ok "$OUTPUT"; then
    echo "FAIL: output is not valid JSON" >&2
    exit 1
fi

# Preserved pre-ARC fields + additive ARC-01/06 fields (additive contract:
# renaming/dropping any pre-ARC field breaks consumers -- session-start CSI-02
# correlation, temp-cleanup segments).
for field in state has_ltads_dir is_tracked config_exists is_temporary cleanup_candidate session_id session_status summary arc_state arc_mode topmost_arc size_warning; do
    if ! echo "$OUTPUT" | grep -q "\"$field\""; then
        echo "FAIL: missing schema field '$field'" >&2
        exit 1
    fi
done

# ---- Fixture verdict legs (require jq; skip with note when unavailable) -----
if command -v jq >/dev/null 2>&1; then
    FIX="$(mktemp -d "${TMPDIR:-/tmp}/ltads-state-validate.XXXXXX")"
    trap 'rm -rf "$FIX"' EXIT

    assert_field() { # assert_field <label> <dir> <jq-filter> <expected>
        local label="$1" dir="$2" filter="$3" expected="$4" out got
        out="$(cd "$dir" && bash "$ORACLE_DIR/run.sh" 2>/dev/null)"
        got="$(printf '%s' "$out" | jq -r "$filter" 2>/dev/null)"
        if [ "$got" != "$expected" ]; then
            echo "FAIL: $label: expected '$expected', got '$got' (output: $out)" >&2
            exit 1
        fi
    }

    # (2) valid + topmost ACTIVE -> valid / CONTINUATION / topmost reported
    mkdir -p "$FIX/valid/ltads"
    printf '{"temporary": false}\n' > "$FIX/valid/ltads/config.json"
    printf '%s\n' '{"schemaVersion":1,"arcs":[{"id":"A007","status":"ACTIVE","started":"2026-07-22T10:00:00Z","checkpoints":[{"at":"2026-07-22T11:00:00Z","by":"sess-1"}]}]}' > "$FIX/valid/ltads/arc-state.json"
    assert_field "valid fixture arc_state" "$FIX/valid" '.arc_state' "valid"
    assert_field "valid fixture arc_mode" "$FIX/valid" '.arc_mode' "CONTINUATION"
    assert_field "valid fixture topmost id" "$FIX/valid" '.topmost_arc.id' "A007"
    assert_field "valid fixture session_id from arc" "$FIX/valid" '.session_id' "A007"

    # (3) topmost CONCLUDED -> FRESH (ARC-06 fresh-next-arc)
    mkdir -p "$FIX/fresh/ltads"
    printf '{"temporary": false}\n' > "$FIX/fresh/ltads/config.json"
    printf '%s\n' '{"schemaVersion":1,"arcs":[{"id":"A008","status":"CONCLUDED","started":"2026-07-22T10:00:00Z","checkpoints":[],"concludedBy":{"sessionId":"sess-1","at":"2026-07-22T12:00:00Z","via":"arc-terminus"}}]}' > "$FIX/fresh/ltads/arc-state.json"
    assert_field "concluded fixture arc_mode" "$FIX/fresh" '.arc_mode' "FRESH"

    # (4) malformed -> invalid, arc_mode null (never guess)
    mkdir -p "$FIX/bad/ltads"
    printf '{"temporary": false}\n' > "$FIX/bad/ltads/config.json"
    printf 'garbage{\n' > "$FIX/bad/ltads/arc-state.json"
    assert_field "malformed fixture arc_state" "$FIX/bad" '.arc_state' "invalid"
    assert_field "malformed fixture arc_mode" "$FIX/bad" '.arc_mode' "null"

    # (5) prose-only current-session.md -> legacy (ARC-11 surfacing trigger)
    mkdir -p "$FIX/legacy/ltads/sessions"
    printf '{"temporary": false}\n' > "$FIX/legacy/ltads/config.json"
    printf '# Session S042\n\n**ID**: S042\n**Status**: IN_PROGRESS\n' > "$FIX/legacy/ltads/sessions/current-session.md"
    assert_field "prose-only fixture arc_state" "$FIX/legacy" '.arc_state' "legacy"
else
    echo "NOTE: jq unavailable; fixture verdict legs skipped" >&2
fi

echo "PASS: ltads-state oracle validates"
exit 0
