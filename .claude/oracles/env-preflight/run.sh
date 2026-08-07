#!/bin/bash
# env-preflight oracle (Unix) -- LTADS-ENV-01 (skill-corpus scrutiny 04 ADD-3 / DEC-124).
#
# Verification Oracle (kind: "verification", ORACULURGY_DESIGN.md Part 11).
# Question: does this environment have the capabilities the project's
# development requires (git, JSON parser, build tooling, optional
# device/browser probes)?
#
# READ-ONLY by contract (Part 11 sec 11.3.4): never writes. Deterministic
# capability checks replacing the hand-rolled agentic probe that used to live
# in segments/-ltads/start_preflight.md (F17): the segment is now a thin
# invoker over this script.
#
# Universal checks: git (critical), json-parser (warn), python (warn, PW-005
# Windows Store stub probe). Conditional checks (project-marker-gated so the
# common path stays cheap): node (critical when package.json exists),
# playwright (warn when a playwright.config.* exists), adb (warn when Android
# markers exist; live `adb devices` probe bounded to 2s via `timeout` when
# available, else binary presence only).
#
# Output: single-line JSON, frozen schema (README.md):
#   {ok, checks:[{name,status,critical,detail,fix}], critical_failures,
#    warnings, summary, briefing}
# Exit code always 0 -- advisory; the consuming skill decides STOP vs proceed.

set +e

esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

CHECKS_JSON=""
CRIT_FAILS=0
WARNINGS=0

# add_check <name> <status> <critical true|false> <detail> <fix>
add_check() {
    local entry
    entry="{\"name\":\"$1\",\"status\":\"$2\",\"critical\":$3,\"detail\":\"$(esc "$4")\",\"fix\":\"$(esc "$5")\"}"
    if [ -z "$CHECKS_JSON" ]; then CHECKS_JSON="$entry"; else CHECKS_JSON="$CHECKS_JSON,$entry"; fi
    if [ "$2" = "unavailable" ] && [ "$3" = "true" ]; then
        CRIT_FAILS=$((CRIT_FAILS + 1))
    elif [ "$2" = "degraded" ] || { [ "$2" = "unavailable" ] && [ "$3" = "false" ]; }; then
        WARNINGS=$((WARNINGS + 1))
    fi
}

# ---- git (critical) ---------------------------------------------------------
if command -v git >/dev/null 2>&1; then
    if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
        add_check "git" "available" "true" "git present; inside a work tree" ""
    else
        add_check "git" "degraded" "true" "git present but this directory is not a git work tree" "git init (or run from the project root)"
    fi
else
    add_check "git" "unavailable" "true" "git not found on PATH" "install git"
fi

# ---- json-parser (warn) -- jq or a WORKING python (PW-005 stub probe) -------
PY=""
if command -v python3 >/dev/null 2>&1 && python3 -c "pass" >/dev/null 2>&1; then
    PY="python3"
elif command -v python >/dev/null 2>&1 && python -c "pass" >/dev/null 2>&1; then
    PY="python"
fi
if command -v jq >/dev/null 2>&1; then
    add_check "json-parser" "available" "false" "jq present" ""
elif [ -n "$PY" ]; then
    add_check "json-parser" "available" "false" "no jq; working $PY used as JSON parser" ""
else
    add_check "json-parser" "degraded" "false" "no jq and no working python -- framework registry/oracle scripts degrade" "install jq (preferred) or a real python"
fi

# ---- python (warn; PW-005 Windows Store stub detection) ----------------------
if [ -n "$PY" ]; then
    add_check "python" "available" "false" "working python: $PY" ""
elif command -v python3 >/dev/null 2>&1 || command -v python >/dev/null 2>&1; then
    add_check "python" "degraded" "false" "python binary on PATH is non-functional (Windows Store stub, PW-005)" "install real python or disable the WindowsApps alias"
else
    add_check "python" "unavailable" "false" "no python on PATH" "install python (optional; jq covers JSON parsing)"
fi

# ---- node (critical only when package.json exists) --------------------------
if [ -f "package.json" ]; then
    if command -v node >/dev/null 2>&1; then
        add_check "node" "available" "true" "package.json present; node $(node --version 2>/dev/null | tr -d '\r') found" ""
    else
        add_check "node" "unavailable" "true" "package.json present but node not on PATH -- build/test will fail" "install Node.js"
    fi
else
    add_check "node" "not-applicable" "false" "no package.json" ""
fi

# ---- playwright (warn only when a playwright config exists) -----------------
PW_CFG=""
for c in playwright.config.ts playwright.config.js playwright.config.mjs; do
    [ -f "$c" ] && PW_CFG="$c" && break
done
if [ -n "$PW_CFG" ]; then
    if [ -d "node_modules/playwright" ] || [ -d "node_modules/@playwright" ]; then
        add_check "playwright" "available" "false" "$PW_CFG present; playwright installed in node_modules" ""
    else
        add_check "playwright" "degraded" "false" "$PW_CFG present but playwright not installed" "npm install && npx playwright install"
    fi
else
    add_check "playwright" "not-applicable" "false" "no playwright config" ""
fi

# ---- shared Android/mobile marker detection ----------------------------------
# Widened for monorepo layouts (apps/*/android, packages/*/android, Expo app.json).
# (apps/*/android, packages/*/android) + Expo app.json -- shared by the adb,
# jdk, and mobile-mcp probes so all mobile checks gate consistently (PROV-01/03).
ANDROID=""
{ [ -d "android" ] || [ -f "AndroidManifest.xml" ] || [ -f "app/build.gradle" ] || [ -f "build.gradle" ]; } && ANDROID="1"
if [ -z "$ANDROID" ]; then
    for _d in apps/*/android packages/*/android; do
        [ -d "$_d" ] && ANDROID="1" && break
    done
fi
if [ -z "$ANDROID" ] && [ -f "app.json" ] && grep -q '"expo"' app.json 2>/dev/null; then
    ANDROID="1"
fi
# RN/Expo mobile marker (mobile-mcp gate, MSG-001): Android markers OR
# app.json + react-native/expo dependency.
MOBILE="$ANDROID"
if [ -z "$MOBILE" ] && [ -f "app.json" ] && [ -f "package.json" ] && grep -qE '"(react-native|expo)"' package.json 2>/dev/null; then
    MOBILE="1"
fi

# ---- adb (warn only when Android markers exist) ------------------------------
if [ -n "$ANDROID" ]; then
    if command -v adb >/dev/null 2>&1; then
        DEVICES=""
        if command -v timeout >/dev/null 2>&1; then
            # Bounded live probe (2s) -- `adb devices` may auto-start a server.
            DEVICES=$(timeout 2 adb devices 2>/dev/null | awk 'NR>1 && $2=="device"{n++} END{print n+0}')
        fi
        if [ -n "$DEVICES" ] && [ "$DEVICES" -gt 0 ] 2>/dev/null; then
            add_check "adb" "available" "false" "Android markers present; $DEVICES device(s)/emulator(s) connected" ""
        elif [ -n "$DEVICES" ]; then
            add_check "adb" "degraded" "false" "adb present but no device/emulator connected" "start an emulator (emulator -avd <name>) or connect a device, then adb devices"
        else
            add_check "adb" "degraded" "false" "adb present; live device probe skipped (no timeout cmd or probe timed out)" "verify manually: adb devices"
        fi
    else
        add_check "adb" "degraded" "false" "Android markers present but adb not on PATH" "install Android SDK platform-tools"
    fi
else
    add_check "adb" "not-applicable" "false" "no Android markers" ""
fi

# ---- node-deps (warn only when package.json exists) -- PROV-01 wall 1 --------
# PROV-01 check-leg probes (node-deps,
# workspace-built, jdk, version-drift) + PROV-03 mobile-mcp probe (spec by
# CLAUDE-E, MSG-001). All additive to the frozen schema, all marker-gated,
# all read-only.
if [ -f "package.json" ]; then
    PM="npm"
    [ -f "pnpm-lock.yaml" ] && PM="pnpm"
    [ -f "yarn.lock" ] && PM="yarn"
    if [ -d "node_modules" ] && [ -n "$(ls -A node_modules 2>/dev/null | head -1)" ]; then
        add_check "node-deps" "available" "false" "node_modules present ($PM project)" ""
    else
        add_check "node-deps" "degraded" "false" "node_modules absent -- build/dev/test will fail until installed" "run the provisioner: provision.sh provision deps (or $PM install)"
    fi
else
    add_check "node-deps" "not-applicable" "false" "no package.json" ""
fi

# ---- workspace-built (warn only when workspace markers exist) -- wall 4 -------
WORKSPACE=""
[ -f "pnpm-workspace.yaml" ] && WORKSPACE="1"
if [ -z "$WORKSPACE" ] && [ -f "package.json" ] && grep -q '"workspaces"' package.json 2>/dev/null; then
    WORKSPACE="1"
fi
if [ -n "$WORKSPACE" ]; then
    UNBUILT=""
    UNBUILT_N=0
    for _dir in packages/* apps/* libs/*; do
        [ -d "$_dir" ] || continue
        [ -f "$_dir/package.json" ] || continue
        grep -q '"build"[[:space:]]*:' "$_dir/package.json" 2>/dev/null || continue
        if [ ! -d "$_dir/dist" ] && [ ! -d "$_dir/lib" ] && [ ! -d "$_dir/build" ]; then
            UNBUILT="$UNBUILT $_dir"
            UNBUILT_N=$((UNBUILT_N + 1))
        fi
    done
    if [ "$UNBUILT_N" -eq 0 ]; then
        add_check "workspace-built" "available" "false" "all workspace packages with build scripts have build output" ""
    else
        add_check "workspace-built" "degraded" "false" "$UNBUILT_N unbuilt workspace package(s):$UNBUILT -- dev servers cannot resolve them (wall 4)" "run the provisioner: provision.sh provision workspace"
    fi
else
    add_check "workspace-built" "not-applicable" "false" "no workspace markers" ""
fi

# ---- jdk (warn only when Android markers exist) -- PROV-08 wall 6 -------------
if [ -n "$ANDROID" ]; then
    # version from a JDK dir's release file (fixture-friendly: no exec needed)
    _jdk_rel_major() {
        local v
        [ -f "$1/release" ] || return 0
        v=$(grep '^JAVA_VERSION=' "$1/release" 2>/dev/null | head -1 | sed 's/^JAVA_VERSION="\{0,1\}\([0-9.]*\).*/\1/')
        [ -z "$v" ] && return 0
        case "$v" in
            1.*) printf '%s' "$v" | cut -d. -f2 ;;
            *)   printf '%s' "$v" | cut -d. -f1 ;;
        esac
    }
    _jdk_exec_major() {
        local out v
        out=$("$1" -version 2>&1 | head -1)
        v=$(printf '%s' "$out" | sed 's/.*version "\{0,1\}\([0-9._]*\).*/\1/')
        [ -z "$v" ] && return 0
        case "$v" in
            1.*) printf '%s' "$v" | cut -d. -f2 | cut -d_ -f1 ;;
            *)   printf '%s' "$v" | cut -d. -f1 ;;
        esac
    }
    JDK_SEL=""
    JDK_SRC=""
    JDK_PATH_V=""
    command -v java >/dev/null 2>&1 && JDK_PATH_V=$(_jdk_exec_major java)
    # candidates: PROVISION_JBR_CANDIDATES override (semicolon-separated), else
    # Android Studio JBR discovery (same list as provision.sh)
    _cands="${PROVISION_JBR_CANDIDATES:-}"
    if [ -z "$_cands" ]; then
        _la="${LOCALAPPDATA:-}"
        _cands="C:/Program Files/Android/Android Studio/jbr;/Applications/Android Studio.app/Contents/jbr/Contents/Home;/opt/android-studio/jbr"
        [ -n "$_la" ] && _cands="$(printf '%s' "$_la" | tr '\\' '/')/Programs/Android Studio/jbr;$_cands"
        [ -n "${HOME:-}" ] && _cands="$_cands;$HOME/android-studio/jbr"
    fi
    _old_ifs="$IFS"; IFS=';'
    for _c in $_cands; do
        [ -d "$_c" ] || continue
        _m=$(_jdk_rel_major "$_c")
        [ -z "$_m" ] && [ -x "$_c/bin/java" ] && _m=$(_jdk_exec_major "$_c/bin/java")
        if [ -n "$_m" ] && [ "$_m" -ge 17 ] 2>/dev/null; then
            JDK_SEL="$_m ($_c)"; JDK_SRC="jbr"; break
        fi
    done
    IFS="$_old_ifs"
    if [ -z "$JDK_SEL" ] && [ -n "${JAVA_HOME:-}" ] && [ -d "$JAVA_HOME" ]; then
        _m=$(_jdk_rel_major "$JAVA_HOME")
        [ -z "$_m" ] && [ -x "$JAVA_HOME/bin/java" ] && _m=$(_jdk_exec_major "$JAVA_HOME/bin/java")
        if [ -n "$_m" ] && [ "$_m" -ge 17 ] 2>/dev/null; then
            JDK_SEL="$_m ($JAVA_HOME)"; JDK_SRC="java_home"
        fi
    fi
    if [ -z "$JDK_SEL" ] && [ -n "$JDK_PATH_V" ] && [ "$JDK_PATH_V" -ge 17 ] 2>/dev/null; then
        JDK_SEL="$JDK_PATH_V (PATH)"; JDK_SRC="path"
    fi
    if [ -n "$JDK_SEL" ]; then
        add_check "jdk" "available" "false" "JDK $JDK_SEL via $JDK_SRC; PATH java: ${JDK_PATH_V:-none}" ""
    elif [ -n "$JDK_PATH_V" ]; then
        add_check "jdk" "degraded" "false" "only JDK $JDK_PATH_V found -- Android Gradle needs 17+ (wall 6)" "install Android Studio (bundles JBR 21) or a JDK 17+; the provisioner records the selection (provision toolchain)"
    else
        add_check "jdk" "degraded" "false" "no JDK found (JBR candidates, JAVA_HOME, PATH probed) -- Android builds will fail" "install Android Studio (bundles JBR 21) or a JDK 17+"
    fi
else
    add_check "jdk" "not-applicable" "false" "no Android markers" ""
fi

# ---- version-drift (advisory; config-driven latest-known majors) -- wall 9 ----
# Table lives at .claude/config.json provision.latestKnownMajors ({"next": 17});
# a hardcoded table would itself go stale. No table => not-applicable (NO-DATA).
if [ -f "package.json" ] && [ -f ".claude/config.json" ] && grep -q '"latestKnownMajors"' .claude/config.json 2>/dev/null; then
    TABLE=""
    if command -v jq >/dev/null 2>&1; then
        TABLE=$(jq -r '(.provision.latestKnownMajors // {}) | to_entries[] | "\(.key)\t\(.value)"' .claude/config.json 2>/dev/null)
    elif [ -n "$PY" ]; then
        TABLE=$("$PY" -c "
import json
try:
    t = json.load(open('.claude/config.json', encoding='utf-8-sig')).get('provision', {}).get('latestKnownMajors', {})
    for k, v in t.items():
        print('%s\t%s' % (k, v))
except Exception:
    pass
" 2>/dev/null)
    fi
    if [ -z "$TABLE" ] && ! command -v jq >/dev/null 2>&1 && [ -z "$PY" ]; then
        add_check "version-drift" "degraded" "false" "latestKnownMajors table present but no JSON parser -- drift state is NO-DATA, not clean" "install jq"
    elif [ -z "$TABLE" ]; then
        add_check "version-drift" "not-applicable" "false" "latestKnownMajors table empty or unparsable" ""
    else
        DRIFTS=""
        DRIFT_N=0
        while IFS="$(printf '\t')" read -r _pkg _known; do
            [ -z "$_pkg" ] && continue
            _pf="node_modules/$_pkg/package.json"
            [ -f "$_pf" ] || continue
            _inst=$(grep -m1 '"version"' "$_pf" 2>/dev/null | sed 's/.*"version"[^"]*"\([0-9]*\).*/\1/')
            [ -z "$_inst" ] && continue
            if [ "$_inst" -lt "$_known" ] 2>/dev/null; then
                DRIFTS="$DRIFTS $_pkg@$_inst<$_known"
                DRIFT_N=$((DRIFT_N + 1))
            fi
        done <<EOF_DRIFT
$TABLE
EOF_DRIFT
        if [ "$DRIFT_N" -eq 0 ]; then
            add_check "version-drift" "available" "false" "no major-version drift vs configured latest-known majors" ""
        else
            add_check "version-drift" "degraded" "false" "$DRIFT_N package(s) behind latest-known major:$DRIFTS (advisory, wall 9)" "review upgrade paths, or update provision.latestKnownMajors if the table is stale"
        fi
    fi
else
    add_check "version-drift" "not-applicable" "false" "no latestKnownMajors table configured (.claude/config.json provision.latestKnownMajors) -- drift check has NO-DATA" ""
fi

# ---- mobile-mcp (warn only when mobile markers exist) -- PROV-03, MSG-001 -----
# Spec by CLAUDE-E (messages.md MSG-001): server-agnostic detection of a mobile
# MCP registration in project-scoped .mcp.json. NO-DATA honesty: unparsable is
# never reported as unregistered.
#
# Registration is necessary but NOT sufficient (DEFER-PROV-MCP-REACH): Claude
# Code does not start a project-scoped server until the user trusts it, and
# trust lives in ~/.claude.json -> projects[<root>]. Reporting only; the oracle
# stays read-only (Part 11 sec 11.3.4) and never actuates trust.

# Normalize a host path for key comparison: backslashes -> slashes, git-bash /c/
# and WSL /mnt/c/ drive forms -> c:/, drop trailing slashes, lowercase.
mcp_norm_path() {
    printf '%s' "$1" | sed -e 's#\\#/#g' \
        -e 's#^/mnt/\([a-zA-Z]\)/#\1:/#' \
        -e 's#^/\([a-zA-Z]\)/#\1:/#' \
        -e 's#/*$##' | tr -d '\r' | tr '[:upper:]' '[:lower:]'
}

# Canonicalize before normalizing: resolves Windows 8.3 short names (CARBON~1)
# and symlinks to the long form the stored keys use. Applied to the lookup
# TARGET only -- the stored keys are already canonical, so paying one subshell
# beats paying one per key.
mcp_canon_path() {
    local _p="$1" _r
    if [ -d "$_p" ]; then
        _r=$(cd "$_p" 2>/dev/null && pwd -P 2>/dev/null)
        [ -n "$_r" ] && _p="$_r"
    fi
    mcp_norm_path "$_p"
}

# mcp_trust_state <server-name> -> trusted | untrusted | disabled | nodata
mcp_trust_state() {
    local _server="$1"
    local _cfg="$HOME/.claude.json"
    [ -f "$_cfg" ] || { printf 'nodata'; return 0; }
    [ -n "$_server" ] || { printf 'nodata'; return 0; }
    local _hasjq=""
    command -v jq >/dev/null 2>&1 && _hasjq="1"
    [ -n "$_hasjq" ] || [ -n "$PY" ] || { printf 'nodata'; return 0; }
    local _target _keys _k _match _verdict
    _target=$(mcp_canon_path "$(pwd)")
    if [ -n "$_hasjq" ]; then
        _keys=$(jq -r '(.projects // {}) | keys[]' "$_cfg" 2>/dev/null | tr -d '\r')
    else
        _keys=$("$PY" -c "
import json,sys
try:
    d = json.load(open(sys.argv[1], encoding='utf-8-sig'))
    for k in (d.get('projects') or {}):
        print(k)
except Exception:
    pass
" "$_cfg" 2>/dev/null | tr -d '\r')
    fi
    [ -z "$_keys" ] && { printf 'nodata'; return 0; }
    _match=""
    while IFS= read -r _k; do
        [ -z "$_k" ] && continue
        if [ "$(mcp_norm_path "$_k")" = "$_target" ]; then _match="$_k"; break; fi
    done <<EOF_MCP_KEYS
$_keys
EOF_MCP_KEYS
    [ -z "$_match" ] && { printf 'nodata'; return 0; }
    # NEVER pass $_match (an absolute path) into jq/python as an argument or via the
    # environment. Under Git Bash, MSYS rewrites POSIX-absolute values on their way to a
    # NATIVE Windows binary -- `--arg k /tmp/x` arrives as `C:/Users/.../Temp/x` -- and
    # BOTH argv and the environment are rewritten (measured; env is not an escape hatch).
    # The lookup then misses and every verdict falls through to the `else` branch, so the
    # trusted/disabled cases silently report "untrusted" while the untrusted case passes
    # for the wrong reason. Instead: emit key<TAB>verdict for EVERY project and select the
    # row here, where $_match already came from this same key list. $_server ("mobile-mcp")
    # is not path-shaped, so it remains safe to pass. See PW-011 / DISC-PROV-03.
    _pick_verdict() {
        while IFS="$(printf '\t')" read -r _row_k _row_v; do
            [ "$_row_k" = "$_match" ] && { printf '%s' "$_row_v"; return 0; }
        done
        printf ''
    }
    if [ -n "$_hasjq" ]; then
        _verdict=$(jq -r --arg s "$_server" '
            .projects // {} | to_entries[]
            | .key as $k | .value as $p
            | $k + "\t" + (
                if   (($p.disabledMcpjsonServers // []) | index($s)) then "disabled"
                elif ($p.enableAllProjectMcpServers == true)         then "trusted"
                elif (($p.enabledMcpjsonServers // []) | index($s))  then "trusted"
                else "untrusted" end)' "$_cfg" 2>/dev/null | tr -d '\r' | _pick_verdict)
    else
        # Same MSYS argv-rewriting hazard as the jq branch above: sys.argv[2] would arrive
        # as a Windows path and never match. Emit every key's verdict; select in bash.
        _verdict=$("$PY" -c "
import json,sys
try:
    projects = json.load(open(sys.argv[1], encoding='utf-8-sig')).get('projects', {}) or {}
    s = sys.argv[2]
    for k, p in projects.items():
        p = p or {}
        if s in (p.get('disabledMcpjsonServers') or []):
            v = 'disabled'
        elif p.get('enableAllProjectMcpServers') is True:
            v = 'trusted'
        elif s in (p.get('enabledMcpjsonServers') or []):
            v = 'trusted'
        else:
            v = 'untrusted'
        print(k + '\t' + v)
except Exception:
    pass
" "$_cfg" "$_server" 2>/dev/null | tr -d '\r' | _pick_verdict)
    fi
    case "$_verdict" in
        trusted|untrusted|disabled) printf '%s' "$_verdict" ;;
        *) printf 'nodata' ;;
    esac
}

# Emit the trust-aware check for a resolvable, registered mobile MCP server.
# <server-name> <resolvability-detail-suffix>
add_mcp_check_trusted() {
    local _srv="$1" _detail="$2" _trust
    _trust=$(mcp_trust_state "$_srv")
    case "$_trust" in
        trusted)
            add_check "mobile-mcp" "available" "false" "$_detail; trusted for this project" "" ;;
        disabled)
            add_check "mobile-mcp" "unavailable" "false" "$_detail, but '$_srv' is explicitly disabled for this project (~/.claude.json disabledMcpjsonServers) -- the server will not start" "remove '$_srv' from projects[<root>].disabledMcpjsonServers in ~/.claude.json" ;;
        untrusted)
            add_check "mobile-mcp" "degraded" "false" "registered-but-untrusted: $_detail, but the project has not trusted it (~/.claude.json enableAllProjectMcpServers not true, '$_srv' absent from enabledMcpjsonServers) -- the server will never start and its tools will not load" "accept the interactive trust prompt at session start, OR set projects[<root>].enableAllProjectMcpServers=true in ~/.claude.json, OR register the server at user scope (claude mcp add --scope user)" ;;
        *)
            add_check "mobile-mcp" "degraded" "false" "$_detail; trust state is NO-DATA (~/.claude.json unreadable, unparsable, or no matching project key) -- registration alone does not mean the server will start" "verify trust manually: claude mcp list (expect 'Connected'), or check projects[<root>] in ~/.claude.json" ;;
    esac
}
if [ -n "$MOBILE" ]; then
    MCP_LIST="mobile-mcp maestro agent-device appium-mcp"
    if [ ! -f ".mcp.json" ]; then
        add_check "mobile-mcp" "degraded" "false" "mobile project markers present but no .mcp.json -- no mobile MCP registered" "run the provisioner mobile-MCP step (PROV-03) or register a mobile MCP server project-scoped in .mcp.json"
    else
        ENTRIES=""
        if command -v jq >/dev/null 2>&1; then
            ENTRIES=$(jq -r '(.mcpServers // {}) | to_entries[] | "\(.key)\t\(.value.command // "")\t\((.value.args // []) | join(" "))"' .mcp.json 2>/dev/null | tr -d '\r')
        elif [ -n "$PY" ]; then
            ENTRIES=$("$PY" -c "
import json
try:
    d = json.load(open('.mcp.json', encoding='utf-8-sig')).get('mcpServers', {})
    for name, e in d.items():
        print('%s\t%s\t%s' % (name, e.get('command', ''), ' '.join(e.get('args', []))))
except Exception:
    pass
" 2>/dev/null | tr -d '\r')
        fi
        if ! command -v jq >/dev/null 2>&1 && [ -z "$PY" ]; then
            add_check "mobile-mcp" "degraded" "false" "cannot parse .mcp.json (no JSON parser) -- mobile-MCP registration state is NO-DATA, not absence" "install jq"
        else
            MATCH_NAME=""
            MATCH_CMD=""
            MATCH_ARGS=""
            EXTRA_N=0
            # DEFER-083 / DEC-279: `IFS=$'\t' read` COLLAPSES empty fields (tab
            # is IFS whitespace), and `command` is an empty-defaultable MIDDLE
            # field. An entry carrying `args` but no `command` therefore bound
            # the joined ARGS STRING to $_cmd and left $_args empty, so the
            # operator text named the argument string as the command
            # ("...but command not resolvable", remediating "install -y
            # @mobilenext/mobile-mcp@latest"). The `degraded` verdict itself is
            # unaffected -- an entry with no command is not resolvable either
            # way -- so this is a report-label fix, not a verdict fix.
            #
            # MEASURED CORRECTION to the brief (2026-08-05): DEFER-083 named a
            # remote/SSE server (`url`, no `command`) as the trigger. It is NOT
            # one -- such an entry has no args either, so the producer emits
            # "name<TAB><TAB>" and BOTH trailing fields collapse to empty, which
            # is indistinguishable from the correct split. The reachable trigger
            # is an entry with `args` and no `command`.
            #
            # Empty-preserving split, bash-3.2 safe (no arrays/readarray/
            # namerefs -- this file uses none). Values come from jq/python
            # interpolation and cannot contain a tab, so a literal tab is always
            # a field boundary. The `${r#*<tab>}` strip is GUARDED because that
            # expansion returns the WHOLE string when no tab is present (DEC-232,
            # same class one seam over).
            _ep_split_tsv() {
                local _r="$1" _t
                _t=$(printf '\t')
                case "$_r" in (*"$_t"*) EP_F1="${_r%%"$_t"*}"; _r="${_r#*"$_t"}" ;; (*) EP_F1="$_r"; _r="" ;; esac
                case "$_r" in (*"$_t"*) EP_F2="${_r%%"$_t"*}"; _r="${_r#*"$_t"}" ;; (*) EP_F2="$_r"; _r="" ;; esac
                EP_F3="$_r"
            }
            while IFS= read -r _ep_rec; do
                _ep_split_tsv "$_ep_rec"
                _name="$EP_F1"; _cmd="$EP_F2"; _args="$EP_F3"
                [ -z "$_name" ] && continue
                _joined=$(printf '%s %s' "$_cmd" "$_args" | tr '[:upper:]' '[:lower:]')
                _hit=""
                for _kw in $MCP_LIST; do
                    case "$_joined" in *"$_kw"*) _hit="1"; break ;; esac
                done
                if [ -n "$_hit" ]; then
                    if [ -z "$MATCH_NAME" ]; then
                        MATCH_NAME="$_name"; MATCH_CMD="$_cmd"; MATCH_ARGS="$_args"
                    else
                        EXTRA_N=$((EXTRA_N + 1))
                    fi
                fi
            done <<EOF_MCP
$ENTRIES
EOF_MCP
            EXTRA_NOTE=""
            [ "$EXTRA_N" -gt 0 ] && EXTRA_NOTE=" (+$EXTRA_N other mobile MCP entries)"
            if [ -z "$MATCH_NAME" ]; then
                add_check "mobile-mcp" "degraded" "false" ".mcp.json present; no mobile MCP server registered" "run the provisioner mobile-MCP step (PROV-03) or register a mobile MCP server project-scoped in .mcp.json"
            else
                case "$MATCH_CMD" in
                    npx|bunx|pnpm)
                        _pkg=""
                        for _a in $MATCH_ARGS; do
                            case "$_a" in -*|dlx|exec) continue ;; *) _pkg="$_a"; break ;; esac
                        done
                        if [ -n "$_pkg" ] && { [ -d "node_modules/$_pkg" ] || [ -e "node_modules/.bin/$(basename "$_pkg")" ]; }; then
                            add_mcp_check_trusted "$MATCH_NAME" "mobile MCP registered: $MATCH_NAME (package installed locally)$EXTRA_NOTE"
                        else
                            add_check "mobile-mcp" "degraded" "false" "mobile MCP registered ($MATCH_NAME) but package not installed locally -- will fetch at first use (non-deterministic offline)$EXTRA_NOTE" "pre-install project-scoped: npm i -D ${_pkg:-<pkg>}"
                        fi ;;
                    *)
                        if command -v "$MATCH_CMD" >/dev/null 2>&1; then
                            add_mcp_check_trusted "$MATCH_NAME" "mobile MCP registered: $MATCH_NAME ($MATCH_CMD resolvable)$EXTRA_NOTE"
                        else
                            add_check "mobile-mcp" "degraded" "false" "mobile MCP registered ($MATCH_NAME) but command not resolvable$EXTRA_NOTE" "install $MATCH_CMD or fix the .mcp.json command path"
                        fi ;;
                esac
            fi
        fi
    fi
else
    add_check "mobile-mcp" "not-applicable" "false" "no mobile markers" ""
fi

# ---- Compose verdict ----------------------------------------------------------
if [ "$CRIT_FAILS" -gt 0 ]; then
    OK="false"
    summary="$CRIT_FAILS critical capability failure(s), $WARNINGS warning(s)"
    briefing="env-preflight: $CRIT_FAILS critical capability failure(s) -- autonomous execution will fail mid-run; run /0-uldf-ltads-start --preflight for the fix list"
else
    OK="true"
    summary="all critical capabilities available ($WARNINGS warning(s))"
    briefing=""
fi

printf '{"ok":%s,"checks":[%s],"critical_failures":%s,"warnings":%s,"summary":"%s","briefing":"%s"}\n' \
    "$OK" "$CHECKS_JSON" "$CRIT_FAILS" "$WARNINGS" "$(esc "$summary")" "$(esc "$briefing")"
exit 0
