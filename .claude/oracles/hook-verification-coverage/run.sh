#!/bin/bash
# hook-verification-coverage oracle (Unix)
# Verification Oracle (kind: "verification", EPP-03): does every hook script
# registered in settings.json carry its own verification surface?
#
# EPP-03 clause (FOUNDATIONS/ENFORCEMENT_PLACEMENT_PRINCIPLE.md SS 2.4):
# "No hook ships without its own verification surface" -- a buggy Tier-1
# mechanism fails confidently and invisibly (DEC-71 / DISC-CSI-12; the
# 2026-06-11 sync stall). This oracle is the inventory check.
#
# Coverage = any of:
#   1. <hooks_dir>/tests/<base>.test.{sh,ps1} exists
#   2. a smoke under scripts/csi-tests/ OR scripts/smoke-tests/ references
#      "<base>.sh" or "<base>.ps1"
# Hooks predating EPP-03 are grandfathered in BASELINE (reported as warn);
# only NEW uncovered hooks produce status fail.
#
# Read-only, idempotent, <2s. Graceful absence: no settings file -> pass,
# applicable=false. HVC_ROOT env var overrides the scan root (self-tests).
set -u

ROOT="${HVC_ROOT:-.}"

# Hooks shipped before EPP-03 (2026-06-11) without a verification surface.
# Remove an entry when its test/smoke lands. Mirrored in run.ps1 + README.
BASELINE="session-detect command-usage-tracker pre-compact"

# Locate the settings file + sibling dirs (framework repo first, then project).
SETTINGS=""; HOOKS_DIR=""; SMOKE_DIRS=""
# Both harness roots are searched. csi-tests/ was the original home; smoke-tests/
# is where every harness authored since ~2026-06 actually lives. Searching only
# csi-tests/ made this oracle report EPP-03 VIOLATIONS for three hooks that DO
# ship smokes (post-tool-use-write-journal, pre-tool-use-ctd-spawn-gate,
# turn-state-stamp) -- a detector that cannot see the surface reports absence,
# the same failure class as DISC-ORA-02/03 that this very oracle was nearly
# retired by. Fixed 2026-07-13 (DEC-166 arc).
if [ -f "$ROOT/claude-template/settings.json" ]; then
    SETTINGS="$ROOT/claude-template/settings.json"
    HOOKS_DIR="$ROOT/claude-template/hooks"
    SMOKE_DIRS="$ROOT/claude-template/scripts/csi-tests $ROOT/claude-template/scripts/smoke-tests"
elif [ -f "$ROOT/.claude/settings.json" ]; then
    SETTINGS="$ROOT/.claude/settings.json"
    HOOKS_DIR="$ROOT/.claude/hooks"
    SMOKE_DIRS="$ROOT/.claude/scripts/csi-tests $ROOT/.claude/scripts/smoke-tests"
fi

if [ -z "$SETTINGS" ]; then
    printf '{"status":"pass","details":{"applicable":false,"checked":0,"covered":[],"uncovered_new":[],"uncovered_legacy":[]},"briefing":""}\n'
    exit 0
fi

in_baseline() {
    local b probe="$1"
    for b in $BASELINE; do [ "$b" = "$probe" ] && return 0; done
    return 1
}

is_covered() {
    local base="$1"
    [ -f "$HOOKS_DIR/tests/$base.test.sh" ] && return 0
    [ -f "$HOOKS_DIR/tests/$base.test.ps1" ] && return 0
    for _sd in $SMOKE_DIRS; do
        [ -d "$_sd" ] || continue
        grep -rlEq "${base}\.(sh|ps1)" "$_sd" 2>/dev/null && return 0
    done
    return 1
}

covered=""; unc_new=""; unc_legacy=""; checked=0
while IFS= read -r base; do
    [ -z "$base" ] && continue
    checked=$((checked+1))
    if is_covered "$base"; then
        covered="${covered:+$covered,}\"$base\""
    elif in_baseline "$base"; then
        unc_legacy="${unc_legacy:+$unc_legacy,}\"$base\""
    else
        unc_new="${unc_new:+$unc_new,}\"$base\""
    fi
done < <(grep -oE 'hooks/[A-Za-z0-9_-]+\.(ps1|sh)' "$SETTINGS" | sed -E 's|^hooks/||; s|\.(ps1\|sh)$||' | sort -u)

status="pass"; briefing=""
if [ -n "$unc_new" ]; then
    status="fail"
    briefing="EPP-03 VIOLATION: new hook(s) registered without a verification surface: [$unc_new]. Ship a hooks/tests/<name>.test.{sh,ps1} or a scripts/{csi-tests,smoke-tests}/ smoke referencing the hook script, in the same change (ENFORCEMENT_PLACEMENT_PRINCIPLE.md SS 2.4)."
elif [ -n "$unc_legacy" ]; then
    status="warn"
    briefing="EPP-03: legacy hook(s) still lack a verification surface (grandfathered): [$unc_legacy]."
fi

printf '{"status":"%s","details":{"applicable":true,"checked":%d,"covered":[%s],"uncovered_new":[%s],"uncovered_legacy":[%s]},"briefing":"%s"}\n' \
    "$status" "$checked" "$covered" "$unc_new" "$unc_legacy" "$(printf '%s' "$briefing" | sed 's/"/\\"/g')"
[ "$status" = "fail" ] && exit 1
exit 0
