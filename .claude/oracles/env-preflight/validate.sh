#!/bin/bash
# env-preflight oracle self-test (Unix) -- LTADS-ENV-01.
# Confirms run.sh emits valid JSON carrying the frozen schema fields, that the
# universal checks are always present, and that the run is read-only.
set -e
ORACLE_DIR="$(cd "$(dirname "$0")" && pwd)"
OUTPUT="$(bash "$ORACLE_DIR/run.sh" 2>&1)" || { echo "FAIL: run.sh exited non-zero" >&2; exit 1; }

# Probe-verify python (Microsoft Store stub on Windows exits non-zero silently).
PY=""
if command -v python3 >/dev/null 2>&1 && python3 -c "pass" >/dev/null 2>&1; then
    PY="python3"
elif command -v python >/dev/null 2>&1 && python -c "pass" >/dev/null 2>&1; then
    PY="python"
fi

if [ -n "$PY" ]; then
    if ! echo "$OUTPUT" | "$PY" -c "import sys, json; json.load(sys.stdin)" 2>/dev/null; then
        echo "FAIL: output is not valid JSON" >&2
        exit 1
    fi
fi

for field in ok checks critical_failures warnings summary briefing; do
    if ! echo "$OUTPUT" | grep -q "\"$field\""; then
        echo "FAIL: missing schema field '$field'" >&2
        exit 1
    fi
done

# Universal checks must always be present (conditional ones may be not-applicable).
# PROV-01/03/08 probes covered below (every check
# emits an entry -- conditional ones report not-applicable when ungated).
for check in git json-parser python node playwright adb node-deps workspace-built jdk version-drift mobile-mcp; do
    if ! echo "$OUTPUT" | grep -q "\"name\":\"$check\""; then
        echo "FAIL: missing check entry '$check'" >&2
        exit 1
    fi
done

# In this repo (a git work tree with git installed) the git check must pass.
if ! echo "$OUTPUT" | grep -q '"name":"git","status":"available"'; then
    echo "FAIL: git check not 'available' in a git work tree" >&2
    exit 1
fi

# --- mobile-mcp trust reporting (DEFER-PROV-MCP-REACH) ----------------------
# Registration in .mcp.json is necessary but NOT sufficient -- Claude Code does
# not start a project-scoped server the project has not trusted. Assert the
# probe never reports "available" on registration alone.
TDIR="$(mktemp -d "${TMPDIR:-/tmp}/env-preflight-mcp-XXXXXX")"
trap 'rm -rf "$TDIR"' EXIT
mkdir -p "$TDIR/proj/android" "$TDIR/home"
echo '{"name":"x"}' > "$TDIR/proj/package.json"
echo '{"mcpServers":{"maestro":{"command":"bash","args":["-c","mobile-mcp"]}}}' > "$TDIR/proj/.mcp.json"
PROJ_KEY="$(cd "$TDIR/proj" && pwd -P | sed -e 's#^/\([a-zA-Z]\)/#\1:/#')"

mcp_status() { # <trust-json|-> -> the mobile-mcp status token
    # MULTI-KEY on purpose: `jq -r keys[]` emits CRLF on Windows, and a
    # single-key fixture hides it (command substitution strips the lone
    # trailing \r\n). Several keys leave a trailing \r on all but the last.
    if [ "$1" = "-" ]; then rm -f "$TDIR/home/.claude.json"
    else printf '{"projects":{"a:/decoy/before":{},"%s":%s,"z:/decoy/after":{}}}\n' \
        "$PROJ_KEY" "$1" > "$TDIR/home/.claude.json"; fi
    ( cd "$TDIR/proj" && HOME="$TDIR/home" bash "$ORACLE_DIR/run.sh" 2>/dev/null ) \
        | tr ',' '\n' | grep -A1 '"name":"mobile-mcp"' | grep '"status"' \
        | sed 's/.*"status":"\([a-z-]*\)".*/\1/'
}
expect_mcp() { # <expected> <trust-json|-> <label>
    _got="$(mcp_status "$2")"
    if [ "$_got" != "$1" ]; then
        echo "FAIL: mobile-mcp $3: expected '$1', got '$_got'" >&2
        exit 1
    fi
}

expect_mcp degraded    '{"enabledMcpjsonServers":[],"disabledMcpjsonServers":[]}' "registered-but-untrusted must not be 'available'"
expect_mcp available   '{"enableAllProjectMcpServers":true}'                      "enableAllProjectMcpServers=true is trusted"
expect_mcp available   '{"enabledMcpjsonServers":["maestro"]}'                    "server named in enabledMcpjsonServers is trusted"
expect_mcp unavailable '{"disabledMcpjsonServers":["maestro"]}'                   "explicitly-disabled server"
expect_mcp degraded    '{}'                                                        "empty project entry is untrusted"
expect_mcp degraded    '-'                                                         "absent ~/.claude.json is NO-DATA, not a pass"

echo "PASS: env-preflight oracle validates"
exit 0
