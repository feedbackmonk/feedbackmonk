#!/bin/bash
# oracle-template-drift oracle (Unix)
# REFRESH-01 — detect when an oracle installed in this project's
# .claude/oracles/ differs from the current framework baseline (a
# starter-pack fix the add-only /0-uldf-setup-project never delivered).
#
# Output schema (FROZEN base + PACK-02 additive extension — see oracle.json):
#   { drifted: bool, drifted_oracles: [{name, files:[...]}],
#     drift_count: int, compared_count: int,
#     missing_starter: [name,...], missing_count: int, briefing: string }
# missing_starter = PACK_MANIFEST.json packs.starter entries with no installed
# project dir (the F10 gap: the drift comparison only sees INSTALLED oracles).
#
# Read-only, idempotent. Compares the CR-normalized content of the 5
# functional files (oracle.json, run.sh, run.ps1, validate.sh,
# validate.ps1). README.md and test-fixtures/ are NOT compared (docs/
# test-data — comparing them would raise drift on harmless edits).
# A project oracle dir with a .local-customized marker is skipped.

set -u

# ---- Tracked functional files ----------------------------------------------
TRACKED="oracle.json run.sh run.ps1 validate.sh validate.ps1"

# ---- Source resolution ------------------------------------------------------
# Test override env vars take precedence (used by validate.sh fixtures).
BASELINE_DIR="${CLAUDE_ORACLE_BASELINE_DIR:-}"
PROJECT_DIR="${CLAUDE_ORACLE_PROJECT_DIR:-.claude/oracles}"

# Discover baseline if not overridden.
if [ -z "$BASELINE_DIR" ]; then
    if [ -n "${HOME:-}" ] && [ -d "$HOME/.claude/oracles" ]; then
        BASELINE_DIR="$HOME/.claude/oracles"
    else
        # Framework-dev fallback: walk up looking for claude-template/oracles.
        _probe_dir="$(pwd)"
        for _i in 1 2 3 4 5 6; do
            if [ -d "$_probe_dir/claude-template/oracles" ]; then
                BASELINE_DIR="$_probe_dir/claude-template/oracles"
                break
            fi
            _parent="$(dirname "$_probe_dir")"
            [ "$_parent" = "$_probe_dir" ] && break
            _probe_dir="$_parent"
        done
    fi
fi

# ---- Graceful absent: no baseline or no project dir -------------------------
emit_empty() {
    printf '{"drifted":false,"drifted_oracles":[],"drift_count":0,"compared_count":0,"missing_starter":[],"missing_count":0,"briefing":""}\n'
    exit 0
}
[ -z "$BASELINE_DIR" ] || [ ! -d "$BASELINE_DIR" ] && emit_empty
[ -d "$PROJECT_DIR" ] || emit_empty

# ---- CR-normalized content hash --------------------------------------------
# Strips CR so CRLF<->LF copies never raise false drift. Prefers sha256sum,
# then shasum -a 256, then cksum (weak but universally present). Missing file
# yields the sentinel "MISSING" so a project that lacks a baseline file drifts.
_hash_file() {
    [ -f "$1" ] || { printf 'MISSING'; return; }
    if command -v sha256sum >/dev/null 2>&1; then
        tr -d '\r' < "$1" | sha256sum | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        tr -d '\r' < "$1" | shasum -a 256 | awk '{print $1}'
    else
        tr -d '\r' < "$1" | cksum | awk '{print $1"-"$2}'
    fi
}

# ---- JSON string escape -----------------------------------------------------
json_escape() {
    printf '%s' "$1" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g'
}

# ---- Compare each project oracle against baseline ---------------------------
drift_entries=""     # comma-joined JSON objects
drift_count=0
compared_count=0

for subdir in "$PROJECT_DIR"/*/; do
    [ -d "$subdir" ] || continue
    name="$(basename "$subdir")"
    case "$name" in
        shared|candidates|cache) continue ;;
    esac
    # Customization opt-out: skip pinned oracles entirely.
    [ -f "$subdir/.local-customized" ] && continue
    base_oracle="$BASELINE_DIR/$name"
    # Project-only oracle (no baseline counterpart): never compared/touched.
    [ -d "$base_oracle" ] || continue

    compared_count=$((compared_count + 1))

    drifted_files=""
    df_count=0
    for f in $TRACKED; do
        # Only compare files the baseline actually ships for this oracle.
        [ -f "$base_oracle/$f" ] || continue
        bh="$(_hash_file "$base_oracle/$f")"
        ph="$(_hash_file "$subdir/$f")"
        if [ "$bh" != "$ph" ]; then
            esc="$(json_escape "$f")"
            if [ "$df_count" -eq 0 ]; then
                drifted_files="\"$esc\""
            else
                drifted_files="$drifted_files,\"$esc\""
            fi
            df_count=$((df_count + 1))
        fi
    done

    if [ "$df_count" -gt 0 ]; then
        name_esc="$(json_escape "$name")"
        entry="{\"name\":\"$name_esc\",\"files\":[$drifted_files]}"
        if [ "$drift_count" -eq 0 ]; then
            drift_entries="$entry"
        else
            drift_entries="$drift_entries,$entry"
        fi
        drift_count=$((drift_count + 1))
    fi
done

# ---- Missing starter oracles (PACK-02 additive extension) --------------------
# packs.starter entries in the baseline PACK_MANIFEST.json with no installed
# project dir. Line-parsed per the manifest's formatting contract (one quoted
# name per line) -- no JSON-parser dependency. Manifest absent (pre-PACK-01
# baseline) -> empty (graceful). .local-customized cannot apply (nothing is
# installed to pin).
missing_entries=""
missing_count=0
MANIFEST="$BASELINE_DIR/PACK_MANIFEST.json"
if [ -f "$MANIFEST" ]; then
    _starter="$(sed -n '/"starter"[[:space:]]*:[[:space:]]*\[/,/\]/p' "$MANIFEST" \
        | sed -n 's/^[[:space:]]*"\([a-z0-9][a-z0-9-]*\)",\{0,1\}[[:space:]]*$/\1/p')"
    while IFS= read -r _name; do
        [ -n "$_name" ] || continue
        # Phantom manifest entries (no baseline dir) are validate-pack-manifest's
        # finding, not a project gap.
        [ -d "$BASELINE_DIR/$_name" ] || continue
        if [ ! -d "$PROJECT_DIR/$_name" ]; then
            _esc="$(json_escape "$_name")"
            if [ "$missing_count" -eq 0 ]; then
                missing_entries="\"$_esc\""
            else
                missing_entries="$missing_entries,\"$_esc\""
            fi
            missing_count=$((missing_count + 1))
        fi
    done <<EOF
$_starter
EOF
fi

# ---- Build briefing ---------------------------------------------------------
# `drifted` keeps its frozen semantics (content drift of INSTALLED oracles);
# the briefing fires on drift OR missing-starter gaps.
if [ "$drift_count" -gt 0 ]; then
    drifted="true"
else
    drifted="false"
fi
if [ "$drift_count" -gt 0 ] && [ "$missing_count" -gt 0 ]; then
    briefing="oracle-template-drift: ${drift_count} oracle(s) drifted from framework baseline, ${missing_count} starter oracle(s) not installed — run /0-uldf-migrate-oracles"
elif [ "$drift_count" -gt 0 ]; then
    briefing="oracle-template-drift: ${drift_count} oracle(s) drifted from framework baseline — run /0-uldf-migrate-oracles to refresh"
elif [ "$missing_count" -gt 0 ]; then
    briefing="oracle-template-drift: ${missing_count} starter oracle(s) not installed — run /0-uldf-migrate-oracles to install"
else
    briefing=""
fi
briefing_esc="$(json_escape "$briefing")"

printf '{"drifted":%s,"drifted_oracles":[%s],"drift_count":%d,"compared_count":%d,"missing_starter":[%s],"missing_count":%d,"briefing":"%s"}\n' \
    "$drifted" "$drift_entries" "$drift_count" "$compared_count" "$missing_entries" "$missing_count" "$briefing_esc"
