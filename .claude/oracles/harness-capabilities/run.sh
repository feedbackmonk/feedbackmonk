#!/bin/bash
# harness-capabilities oracle (Unix) -- HAL-01 (section HAL, DEC-156)
#
# Deterministic detection: which native harness features does the CURRENT
# session's harness expose? Emits the frozen capability map defined in
# claude-template/templates/HAL_CAPABILITY_MAP.md (schema-level events happen
# THERE, not here). Consumed exclusively via segments/_hal-gate.md -- skills
# never version-sniff inline.
#
# Detection model:
#   1. Harness: env signals (CLAUDECODE=1 or CLAUDE_CODE_SESSION_ID set)
#      => "claude-code"; neither => "unknown" and an ALL-FALSE map is emitted
#      as a VALID result (exit 0, never an error -- HAL_CAPABILITY_MAP 3.2).
#   2. Version: `claude --version` (first X.Y.Z token). Undetectable => null,
#      which forces every version-keyed bit false (3.1: undetectable => false).
#   3. Bits: the version->capability floor table below (the ONLY home of that
#      knowledge -- HAL-01 version-keying rule) plus runtime kill-switch
#      refinements (workflow disable env; Agent Teams experimental opt-in).
#
# Conservative floors: rolling features without a crisp changelog first-ship
# (nativeWorktrees, bundledReviewSkills, forkSubagents) floor at the version
# they were confirmed live (2.1.205/2.1.206, evidence base
# docs/NATIVE_FEATURES_2026H1_INTEGRATION_ANALYSIS.md). A false-false costs
# only a missed fast path, never breakage. Updating a floor = edit here +
# refresh fixtures; no schema change.
#
# TEST SEAMS (validators only; never set in normal operation):
#   HARNESS_CAPS_FORCE_VERSION  raw version-string candidate; replaces the
#                               `claude --version` call (unparseable => null)
#   HARNESS_CAPS_NOW            pinned detectedAt timestamp (golden fixtures)
#
# Output: single-line JSON per HAL_CAPABILITY_MAP.md. Always exit 0.

set +e

NOW="${HARNESS_CAPS_NOW:-$(date -u +%Y-%m-%dT%H:%M:%SZ)}"

# ---- 1. Harness detection ---------------------------------------------------
harness="unknown"
if [ "$CLAUDECODE" = "1" ] || [ -n "$CLAUDE_CODE_SESSION_ID" ]; then
    harness="claude-code"
fi

# ---- 2. Version detection (only for a detected harness) --------------------
version=""           # empty = undetectable (emitted as null)
if [ "$harness" = "claude-code" ]; then
    if [ -n "${HARNESS_CAPS_FORCE_VERSION+x}" ]; then
        raw="$HARNESS_CAPS_FORCE_VERSION"
    else
        raw="$(claude --version 2>/dev/null)"
    fi
    version="$(printf '%s' "$raw" | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)"
fi

# ---- 3. Version comparison --------------------------------------------------
# ver_ge <floor> : is $version >= floor? False when version is undetectable.
ver_ge() {
    [ -n "$version" ] || return 1
    local floor="$1"
    local v1 v2 v3 f1 f2 f3
    IFS=. read -r v1 v2 v3 <<EOF
$version
EOF
    IFS=. read -r f1 f2 f3 <<EOF
$floor
EOF
    v1=${v1:-0}; v2=${v2:-0}; v3=${v3:-0}; f1=${f1:-0}; f2=${f2:-0}; f3=${f3:-0}
    if [ "$v1" -ne "$f1" ]; then [ "$v1" -gt "$f1" ]; return $?; fi
    if [ "$v2" -ne "$f2" ]; then [ "$v2" -gt "$f2" ]; return $?; fi
    [ "$v3" -ge "$f3" ]
}

# ---- 4. Capability bits (frozen spellings; floor table = HAL-01) ------------
b() { if "$@"; then printf 'true'; else printf 'false'; fi; }

workflowTool="false"
if ver_ge 2.1.154 && [ "$CLAUDE_CODE_DISABLE_WORKFLOWS" != "1" ]; then workflowTool="true"; fi
nativeWorktrees=$(b ver_ge 2.1.205)
monitor=$(b ver_ge 2.1.195)
monitorWebsocket=$(b ver_ge 2.1.195)
sendMessage=$(b ver_ge 2.1.198)
sharedTaskList=$(b ver_ge 2.1.142)
bundledReviewSkills=$(b ver_ge 2.1.205)
loopScheduleWakeup=$(b ver_ge 2.1.202)
cronLocal=$(b ver_ge 2.1.202)
agentTeams="false"
if ver_ge 2.1.178 && [ "$CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS" = "1" ]; then agentTeams="true"; fi
forkSubagents=$(b ver_ge 2.1.206)
backgroundSubagents=$(b ver_ge 2.1.198)

# ---- 5. Compose --------------------------------------------------------------
true_count=0
for v in "$workflowTool" "$nativeWorktrees" "$monitor" "$monitorWebsocket" \
         "$sendMessage" "$sharedTaskList" "$bundledReviewSkills" \
         "$loopScheduleWakeup" "$cronLocal" "$agentTeams" "$forkSubagents" \
         "$backgroundSubagents"; do
    [ "$v" = "true" ] && true_count=$((true_count + 1))
done

if [ "$harness" = "unknown" ]; then
    briefing="harness-capabilities: unknown harness - all capabilities false, ULDF paths (valid result, not an error)"
    version_json="null"
elif [ -z "$version" ]; then
    briefing="harness-capabilities: claude-code, version undetectable - all version-keyed capabilities false"
    version_json="null"
else
    briefing="harness-capabilities: claude-code $version - $true_count/12 capabilities native"
    version_json="\"$version\""
fi

printf '{"schemaVersion":"1","harness":"%s","version":%s,"detectedAt":"%s","capabilities":{"workflowTool":%s,"nativeWorktrees":%s,"monitor":%s,"monitorWebsocket":%s,"sendMessage":%s,"sharedTaskList":%s,"bundledReviewSkills":%s,"loopScheduleWakeup":%s,"cronLocal":%s,"agentTeams":%s,"forkSubagents":%s,"backgroundSubagents":%s},"briefing":"%s"}\n' \
    "$harness" "$version_json" "$NOW" \
    "$workflowTool" "$nativeWorktrees" "$monitor" "$monitorWebsocket" \
    "$sendMessage" "$sharedTaskList" "$bundledReviewSkills" \
    "$loopScheduleWakeup" "$cronLocal" "$agentTeams" "$forkSubagents" \
    "$backgroundSubagents" "$briefing"
exit 0
