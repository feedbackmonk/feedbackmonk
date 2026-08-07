#!/bin/bash
# oracle-template-drift oracle self-test (Unix)
#
# Builds synthetic baseline/project oracle trees in a tmpdir (foundations-drift
# precedent — no committed fixtures) and runs run.sh against each, asserting the
# expected drift classification. Cases:
#   1. no-drift               — identical -> drifted=false, drift=0, compared=1
#   2. one-file-drift         — project run.sh differs -> drift=1, files=[run.sh]
#   3. multi-oracle-drift     — two oracles differ -> drift=2
#   4. pinned-skip            — drifted but .local-customized -> drift=0, compared=0
#   5. project-only-oracle    — no baseline counterpart -> drift=0, compared=0
#   6. project-missing-file   — project lacks baseline run.ps1 -> drift=1, files=[run.ps1]
#   7. graceful-absent        — baseline dir missing -> drift=false, briefing=""
#   8. missing-starter        — PACK_MANIFEST names a starter oracle the project
#                               lacks -> missing_count=1, briefing "not installed"
#   9. no-manifest-graceful   — baseline without PACK_MANIFEST.json -> missing=0
#                               (pre-PACK-01 baseline; frozen-base behavior)

set -u
ORACLE_DIR="$(cd "$(dirname "$0")" && pwd)"
RUN="$ORACLE_DIR/run.sh"

PASS=0
FAIL=0
fail() { echo "FAIL: $1" >&2; FAIL=$((FAIL+1)); }
pass() { echo "PASS: $1"; PASS=$((PASS+1)); }

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# Probe for python (Windows-Store stub guard; same pattern as sibling oracles).
PYBIN=""
for _c in python3 python; do
    if command -v "$_c" >/dev/null 2>&1 && "$_c" -c "import sys; sys.exit(0)" >/dev/null 2>&1; then
        PYBIN="$_c"; break
    fi
done

get_field() {
    # $1 json, $2 field
    local json="$1" field="$2"
    if [ -n "$PYBIN" ]; then
        printf '%s' "$json" | "$PYBIN" -c "
import sys, json
d = json.load(sys.stdin)
v = d.get('$field')
if isinstance(v, bool): print('true' if v else 'false')
elif v is None: print('')
else: print(v)
"
    else
        case "$field" in
            drifted) printf '%s' "$json" | grep -oE '"drifted":(true|false)' | head -1 | sed 's/.*://' ;;
            drift_count|compared_count|missing_count) printf '%s' "$json" | grep -oE "\"$field\":[0-9]+" | head -1 | sed 's/.*://' ;;
            briefing) printf '%s' "$json" | grep -oE '"briefing":"[^"]*"' | head -1 | sed 's/^"briefing":"//;s/"$//' ;;
        esac
    fi
}

mk_oracle() {
    # $1 root dir, $2 name, $3 variant tag (controls run.sh / run.ps1 content)
    local od="$1/$2"
    mkdir -p "$od"
    printf '{"name":"%s"}\n' "$2" > "$od/oracle.json"
    printf '#run %s\n' "$3" > "$od/run.sh"
    printf '#runps %s\n' "$3" > "$od/run.ps1"
}

# Schema check — output must contain the 5 frozen-base fields + the PACK-02
# additive pair.
check_schema() {
    local label="$1" json="$2" field
    for field in drifted drifted_oracles drift_count compared_count missing_starter missing_count briefing; do
        printf '%s' "$json" | grep -q "\"$field\"" || { fail "$label: missing schema field '$field' (out=$json)"; return 1; }
    done
    return 0
}

run_oracle() { # $1 baseline $2 project -> echoes JSON
    CLAUDE_ORACLE_BASELINE_DIR="$1" CLAUDE_ORACLE_PROJECT_DIR="$2" bash "$RUN" 2>&1
}

# ---- Case 1: no-drift -------------------------------------------------------
B="$TMP/c1/b"; P="$TMP/c1/p"
mk_oracle "$B" alpha v1; mk_oracle "$P" alpha v1
OUT="$(run_oracle "$B" "$P")"
if check_schema "no-drift" "$OUT"; then
    [ "$(get_field "$OUT" drifted)" = "false" ] && [ "$(get_field "$OUT" drift_count)" = "0" ] && [ "$(get_field "$OUT" compared_count)" = "1" ] \
        && [ -z "$(get_field "$OUT" briefing)" ] \
        && pass "no-drift" || fail "no-drift: out=$OUT"
fi

# ---- Case 2: one-file-drift -------------------------------------------------
B="$TMP/c2/b"; P="$TMP/c2/p"
mk_oracle "$B" alpha v1; mk_oracle "$P" alpha v1
printf '#run v2\n' > "$P/alpha/run.sh"
OUT="$(run_oracle "$B" "$P")"
if check_schema "one-file-drift" "$OUT"; then
    if [ "$(get_field "$OUT" drifted)" = "true" ] && [ "$(get_field "$OUT" drift_count)" = "1" ] \
        && printf '%s' "$OUT" | grep -q '"run.sh"' \
        && printf '%s' "$OUT" | grep -q '/0-uldf-migrate-oracles'; then
        pass "one-file-drift"
    else
        fail "one-file-drift: out=$OUT"
    fi
fi

# ---- Case 3: multi-oracle-drift ---------------------------------------------
B="$TMP/c3/b"; P="$TMP/c3/p"
mk_oracle "$B" alpha v1; mk_oracle "$B" beta v1
mk_oracle "$P" alpha v1; mk_oracle "$P" beta v1
printf '#run v2\n'   > "$P/alpha/run.sh"
printf '#runps v2\n' > "$P/beta/run.ps1"
OUT="$(run_oracle "$B" "$P")"
[ "$(get_field "$OUT" drift_count)" = "2" ] && [ "$(get_field "$OUT" compared_count)" = "2" ] \
    && pass "multi-oracle-drift" || fail "multi-oracle-drift: out=$OUT"

# ---- Case 4: pinned-skip ----------------------------------------------------
B="$TMP/c4/b"; P="$TMP/c4/p"
mk_oracle "$B" alpha v1; mk_oracle "$P" alpha v1
printf '#run v2\n' > "$P/alpha/run.sh"
touch "$P/alpha/.local-customized"
OUT="$(run_oracle "$B" "$P")"
[ "$(get_field "$OUT" drifted)" = "false" ] && [ "$(get_field "$OUT" drift_count)" = "0" ] && [ "$(get_field "$OUT" compared_count)" = "0" ] \
    && pass "pinned-skip" || fail "pinned-skip: out=$OUT"

# ---- Case 5: project-only-oracle --------------------------------------------
B="$TMP/c5/b"; P="$TMP/c5/p"
mkdir -p "$B"                 # baseline exists but lacks 'alpha'
mk_oracle "$P" alpha v1
OUT="$(run_oracle "$B" "$P")"
[ "$(get_field "$OUT" drifted)" = "false" ] && [ "$(get_field "$OUT" compared_count)" = "0" ] \
    && pass "project-only-oracle" || fail "project-only-oracle: out=$OUT"

# ---- Case 6: project-missing-file -------------------------------------------
B="$TMP/c6/b"; P="$TMP/c6/p"
mk_oracle "$B" alpha v1
mkdir -p "$P/alpha"
printf '{"name":"alpha"}\n' > "$P/alpha/oracle.json"
printf '#run v1\n'          > "$P/alpha/run.sh"
# project deliberately lacks run.ps1
OUT="$(run_oracle "$B" "$P")"
if [ "$(get_field "$OUT" drifted)" = "true" ] && [ "$(get_field "$OUT" drift_count)" = "1" ] \
    && printf '%s' "$OUT" | grep -q '"run.ps1"'; then
    pass "project-missing-file"
else
    fail "project-missing-file: out=$OUT"
fi

# ---- Case 7: graceful-absent (baseline missing) -----------------------------
P="$TMP/c7/p"
mk_oracle "$P" alpha v1
OUT="$(run_oracle "$TMP/c7/__nonexistent_baseline__" "$P")"
[ "$(get_field "$OUT" drifted)" = "false" ] && [ -z "$(get_field "$OUT" briefing)" ] \
    && pass "graceful-absent" || fail "graceful-absent: out=$OUT"

# ---- Case 8: missing-starter (PACK-02) ----------------------------------------
B="$TMP/c8/b"; P="$TMP/c8/p"
mk_oracle "$B" alpha v1; mk_oracle "$B" beta v1
mk_oracle "$P" alpha v1     # project lacks beta
cat > "$B/PACK_MANIFEST.json" <<'EOF'
{
  "schemaVersion": 1,
  "packs": {
    "starter": [
      "alpha",
      "beta"
    ],
    "framework-internal": [
    ]
  }
}
EOF
OUT="$(run_oracle "$B" "$P")"
if check_schema "missing-starter" "$OUT"; then
    if [ "$(get_field "$OUT" drifted)" = "false" ] && [ "$(get_field "$OUT" missing_count)" = "1" ] \
        && printf '%s' "$OUT" | grep -q '"beta"' \
        && printf '%s' "$OUT" | grep -q 'not installed'; then
        pass "missing-starter"
    else
        fail "missing-starter: out=$OUT"
    fi
fi

# ---- Case 9: no-manifest-graceful (pre-PACK-01 baseline) ----------------------
B="$TMP/c9/b"; P="$TMP/c9/p"
mk_oracle "$B" alpha v1; mk_oracle "$B" beta v1
mk_oracle "$P" alpha v1
OUT="$(run_oracle "$B" "$P")"
[ "$(get_field "$OUT" missing_count)" = "0" ] && [ -z "$(get_field "$OUT" briefing)" ] \
    && pass "no-manifest-graceful" || fail "no-manifest-graceful: out=$OUT"

# ---- Summary ----------------------------------------------------------------
echo "----"
echo "Total: PASS=$PASS  FAIL=$FAIL"
[ "$FAIL" -gt 0 ] && exit 1
exit 0
