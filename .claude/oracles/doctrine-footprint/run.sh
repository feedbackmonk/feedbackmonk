#!/bin/bash
# doctrine-footprint oracle (Unix) -- DAUD-01 (§ DOCTRINE-AUDIT, DEC-153)
#
# Deterministic PRESENCE check: for each ULDF doctrine, which prescribed
# instrumentation artifacts does THIS project have installed? Answers the
# recurring "is Tectonurgy / are Development Jigs integrated here?" question that
# otherwise forces a manual multi-doc sweep.
#
# This oracle answers PRESENCE ONLY (deterministic). Verdict rendering,
# not-applicable judgment, and remediation routing are the /0-uldf-doctrine-audit
# skill's job (DAUD-02). The `verdict_hint` here is a naive present/absent
# rollup the skill may override (e.g. -> not-applicable for RSPD on a flat spec,
# or ARIA on a no-surface project).
#
# BASELINE HONESTY (DAUD-07): the EXPECTED-artifact set below is the framework's
# PER-PROJECT-installed doctrine instrumentation -- NOT a doctrine's aspirational
# prescription, and NOT global framework helpers. The Tectonurgy detection surface
# `dependency-drift` (MOD-01 M5/M6) IS shipped, but as a GLOBAL script
# (`~/.claude/scripts/dependency-drift.*`, available to every project via sync),
# not a per-project artifact -- so it is deliberately absent from this per-project
# presence set. Checking for it under a project's .claude/ would false-flag EVERY
# project (it is never installed locally). Whether the global helper set is
# complete is `/0-uldf-uldf-admin health`'s job, not this per-project audit. When
# the framework adds a per-project artifact, add it to the baseline block below.
#   Framework baseline dated: 2026-07-10
#
# Output: single-line JSON. kind: project-state, on-demand (NOT a briefing oracle).
#
# Gracefully absent: no .claude/oracles/ -> every artifact absent, exit 0 (a
# greenfield project legitimately has nothing installed; that is a finding, not
# an error). NO-DATA honesty: a scan target present-but-unreadable -> no_data:true
# on that doctrine, never a silent "clean".

set +e

ORACLES_DIR=".claude/oracles"
SPEC="docs/specs/SPECIFICATION.md"
STATE_DIR=".claude/session-state"
FINALIZE_SKILL=".claude/skills/0-uldf-finalize"
# The finalize skill is a GLOBAL framework artifact (~/.claude/skills/), not a
# per-project install -- so the JIG-05 wire check falls back to the global skill
# dir when no project-local override exists (DAUD-07 honesty: checking only
# project-local would false-flag every project forever). Env override keeps the
# golden fixtures hermetic (validate.{sh,ps1} pins it inside the fixture).
GLOBAL_FINALIZE_SKILL="${DOCTRINE_FOOTPRINT_GLOBAL_CLAUDE:-$HOME/.claude}/skills/0-uldf-finalize"

esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }
brief_safe() { printf '%s' "$1" | tr -d '"' | tr '\r\n\t' '   ' | sed 's/  */ /g'; }

# oracle_present <name> : is .claude/oracles/<name>/ installed? (dir with a manifest)
oracle_present() {
    [ -d "$ORACLES_DIR/$1" ] || return 1
    [ -f "$ORACLES_DIR/$1/oracle.json" ] || [ -f "$ORACLES_DIR/$1/manifest.json" ]
}

# spec_section_present <heading-regex> : is a `## <heading>` section in the spec?
spec_section_present() {
    [ -f "$SPEC" ] || return 1
    grep -qE "^## $1" "$SPEC" 2>/dev/null
}

# file_present <path>
file_present() { [ -e "$1" ]; }

# readme_marker_count <marker-regex> : count module READMEs containing the marker.
# Coverage signal only (0 is a valid "no coverage yet", not an error).
readme_marker_count() {
    grep -rlE "$1" --include=README.md . 2>/dev/null | wc -l | tr -d ' '
}

# ---- Per-doctrine accumulation --------------------------------------------
doctrines_json=""
d_first=1
missing_total=0

# add_doctrine <slug> <present_csv> <absent_csv>
# present_csv/absent_csv are comma-joined artifact tokens (already collected).
add_doctrine() {
    local slug="$1" present="$2" absent="$3"
    local p_count a_count hint
    # count non-empty CSV members
    if [ -z "$present" ]; then p_count=0; else p_count=$(printf '%s' "$present" | tr ',' '\n' | grep -c .); fi
    if [ -z "$absent" ];  then a_count=0; else a_count=$(printf '%s' "$absent"  | tr ',' '\n' | grep -c .); fi

    if [ "$a_count" -eq 0 ]; then
        hint="integrated"
    elif [ "$p_count" -eq 0 ]; then
        hint="missing"
    else
        hint="partial"
    fi
    missing_total=$((missing_total + a_count))

    # Build JSON arrays from CSV.
    local pj="" aj="" tok f2
    f2=1
    if [ -n "$present" ]; then
        IFS=','; for tok in $present; do
            [ -n "$tok" ] || continue
            if [ "$f2" -eq 1 ]; then f2=0; else pj="$pj,"; fi
            pj="$pj\"$(esc "$tok")\""
        done; unset IFS
    fi
    f2=1
    if [ -n "$absent" ]; then
        IFS=','; for tok in $absent; do
            [ -n "$tok" ] || continue
            if [ "$f2" -eq 1 ]; then f2=0; else aj="$aj,"; fi
            aj="$aj\"$(esc "$tok")\""
        done; unset IFS
    fi

    if [ "$d_first" -eq 1 ]; then d_first=0; else doctrines_json="$doctrines_json,"; fi
    doctrines_json="$doctrines_json{\"doctrine\":\"$slug\",\"present\":[$pj],\"absent\":[$aj],\"verdict_hint\":\"$hint\"}"
}

# add_doctrine_gated <slug> <note> : emit a doctrine the framework has AUTHORED but
# does not yet prescribe per-project instrumentation for (adoption arc pilot-gated).
# Deliberately contributes ZERO to missing_total and carries no absent[] artifacts:
# counting artifacts the framework does not per-project ship would false-flag every
# project, which DAUD-07 BASELINE HONESTY forbids. The purpose is DISCOVERABILITY --
# the doctrine appears in every audit so projects know it exists and can design for
# it -- not adoption pressure. Flips to a real presence leg when the pilot lands.
add_doctrine_gated() {
    local slug="$1" note="$2"
    if [ "$d_first" -eq 1 ]; then d_first=0; else doctrines_json="$doctrines_json,"; fi
    doctrines_json="$doctrines_json{\"doctrine\":\"$slug\",\"present\":[],\"absent\":[],\"verdict_hint\":\"pilot-gated\",\"note\":\"$(esc "$note")\"}"
}

# collect <token> : append token to $P if present-condition true, else to $A.
# Uses the calling function's P/A via nameless append through globals _P/_A.
_P=""; _A=""
reset_pa() { _P=""; _A=""; }
mark() {   # mark <token> <0|1 present>
    if [ "$2" -eq 0 ]; then
        if [ -z "$_P" ]; then _P="$1"; else _P="$_P,$1"; fi
    else
        if [ -z "$_A" ]; then _A="$1"; else _A="$_A,$1"; fi
    fi
}

# ---- Tectonurgy (Principle 2.16 / § MODULARITY) ---------------------------
reset_pa
oracle_present module-size    ; mark "oracle:module-size"    $?
oracle_present change-coupling; mark "oracle:change-coupling" $?
oracle_present code-graph     ; mark "oracle:code-graph"      $?
spec_section_present "MODULARITY"; mark "spec:MODULARITY" $?
add_doctrine "tectonurgy" "$_P" "$_A"

# ---- Development Jigs (§ JIG) ----------------------------------------------
reset_pa
oracle_present jig-friction; mark "oracle:jig-friction" $?
# JIG_CATALOG.md anywhere in the tree (doctrine home is co-located, JIG-03)
if ls JIG_CATALOG.md docs/JIG_CATALOG.md FOUNDATIONS/JIG_CATALOG.md >/dev/null 2>&1 \
   || [ -n "$(find . -maxdepth 3 -name JIG_CATALOG.md 2>/dev/null | head -1)" ]; then
    mark "file:JIG_CATALOG.md" 0
else
    mark "file:JIG_CATALOG.md" 1
fi
# demand log (JIG-06) -- shared with ARIA probe candidates
file_present "$STATE_DIR/aria-probe-candidates.jsonl"; mark "log:aria-probe-candidates.jsonl" $?
# finalize jig-retrospective wired? (JIG-05 -- SHIPPED as Phase 11 step 4c, DEC-154;
# absent now means a stale/pre-JIG-05 framework install, not an unbuilt roadmap node.)
# Project-local skill override first, then the global framework skill (see header).
jig_wire=1
for _fs in "$FINALIZE_SKILL" "$GLOBAL_FINALIZE_SKILL"; do
    if [ -d "$_fs" ] && grep -rqiE "jig[- ]?retrospective|retrospective jig" "$_fs" 2>/dev/null; then
        jig_wire=0; break
    fi
done
if [ "$jig_wire" -eq 0 ]; then
    mark "wire:finalize-jig-retrospective" 0
else
    mark "wire:finalize-jig-retrospective(JIG-05)" 1
fi
add_doctrine "jigs" "$_P" "$_A"

# ---- Block Verifiability Warrant (§ BVW) ----------------------------------
reset_pa
oracle_present block-verifiability-warrant; mark "oracle:block-verifiability-warrant" $?
warrant_readmes=$(readme_marker_count "^## Verifiability Warrant")
# Coverage signal: presence of >=1 Warrant README section (0 = no coverage yet,
# reported as absent coverage-marker; the oracle presence is the primary check).
if [ "$warrant_readmes" -gt 0 ]; then
    mark "coverage:verifiability-warrant-sections(x$warrant_readmes)" 0
else
    mark "coverage:verifiability-warrant-sections" 1
fi
add_doctrine "bvw" "$_P" "$_A"

# ---- AOR / ARIA (§ ARIA, § FOOTPRINT) -------------------------------------
reset_pa
oracle_present aria-status;        mark "oracle:aria-status"        $?
oracle_present ui-surface-detector; mark "oracle:ui-surface-detector" $?
file_present ".claude/aria-manifest.json"; mark "manifest:aria-manifest.json" $?
file_present "$STATE_DIR/aria-probe-candidates.jsonl"; mark "log:aria-probe-candidates.jsonl" $?
add_doctrine "aria_aor" "$_P" "$_A"

# ---- In-App Agent (§ TUTOR) -- pilot-gated, 0 toward missing_total ---------
# AOR's production-shipped sibling: every app with a UI ships an end-user-facing
# agent that operates it on instruction. Doctrine authored + contracts frozen
# (2026-07-16/17), but per-project instrumentation is gated on the SessionHelm
# Arc 2 pilot -- registry-anchors-resolve is in packs.pilot-gated and is NEVER
# installed by /0-uldf-setup-project. See add_doctrine_gated for why this reports
# no absent[] artifacts. DEFER-021.
add_doctrine_gated "in_app_agent" "In-App Agent / Self-Operating App practice (TUTOR-01..11, FOUNDATIONS/IN_APP_AGENT_DOCTRINE.md): doctrine authored, contracts frozen; per-project instrumentation pilot-gated on the SessionHelm Arc 2 pilot and NOT yet prescribed. Design new UI surfaces on the assumption the agent is coming; do not install artifacts yet."

# ---- Agent-Completeness (§ ACOMP) -- pilot-gated, 0 toward missing_total ---
# The coverage guarantee over an app's capability surface: every user-reachable
# capability is agent-reachable, measured over the semantic command layer.
# Doctrine authored (DEC-253, DEFER-089); per-project instrumentation (capability
# manifest + command-surface-parity check) gated on the first consumer audit
# (Table) and NOT yet prescribed. Second member of the DAUD-09 pilot-gated class.
add_doctrine_gated "agent_completeness" "Agent-Completeness property (ACOMP-01..06, FOUNDATIONS/AGENT_COMPLETENESS_DOCTRINE.md): every user-reachable capability is agent-reachable, measured over the semantic command layer; doctrine authored, per-project instrumentation gated on the first consumer audit (Table) and NOT yet prescribed. Route new user-facing capabilities through the command layer from day one; do not install artifacts yet."

# ---- RSPD (Principle 2.15) -- proportionate; skill may -> not-applicable ---
reset_pa
oracle_present rspd-elaboration-tree; mark "oracle:rspd-elaboration-tree" $?
add_doctrine "rspd" "$_P" "$_A"

# ---- Oraculurgy (hand-off note: /0-uldf-oracle owns the depth audit) -------
reset_pa
if [ -d "$ORACLES_DIR" ]; then mark "dir:.claude/oracles" 0; else mark "dir:.claude/oracles" 1; fi
file_present "$ORACLES_DIR/INDEX.md"; mark "file:oracles/INDEX.md" $?
add_doctrine "oraculurgy" "$_P" "$_A"

# ---- Compose ---------------------------------------------------------------
has_claude="true"; [ -d ".claude" ] || has_claude="false"
no_data="false"

briefing="doctrine-footprint: $missing_total artifact(s) absent across doctrines (run /0-uldf-doctrine-audit for verdicts + routes)"
[ "$missing_total" -eq 0 ] && briefing="doctrine-footprint: all baseline artifacts present"

printf '{"schemaVersion":"1","has_claude_dir":%s,"framework_baseline":"2026-07-10","doctrines":[%s],"missing_total":%d,"no_data":%s,"briefing":"%s"}\n' \
    "$has_claude" "$doctrines_json" "$missing_total" "$no_data" "$(esc "$briefing")"
exit 0
