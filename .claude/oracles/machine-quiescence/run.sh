#!/bin/bash
# machine-quiescence oracle (Unix + Git Bash on Windows)
#
# QUIESCE-02/03/04: "Is this machine clean enough that a number taken now means
# anything?" — answered deterministically, with the residue NAMED (which port,
# which PID, which process) rather than as a bare boolean.
#
# Usage:
#   run.sh [--port N]... [--timing] [--json]
#
#   --port N   A port this measurement needs free (repeatable). A live listener
#              on one of these is BLOCKING: it is the deterministic case where a
#              number would silently mean something else — an already-listening
#              preview server makes Playwright's `reuseExistingServer` skip the
#              webServer command entirely, so the build never runs and the suite
#              grades new code against the previous commit's bundle.
#   --timing   This measurement is load-sensitive (a benchmark, a render timing).
#              Any residue at all then becomes BLOCKING — four idle-but-alive
#              agent sessions moved an incumbent renderer's own figures by
#              ~20-25% in the incident that motivated this oracle.
#
# Exit codes (1:1 with the `verdict` field):
#   0  quiet     no residue in the detection set
#   1  noisy     residue present, none of it definitively invalidates THIS run
#   2  unknown   probe degraded — FAILS CLOSED, treat as refusal, never as quiet
#   3  blocking  residue that definitively invalidates THIS run
#
# GROUND TRUTH IS THE OS. Detection is listening sockets and live processes.
# `MACHINE_CONFIG.md` and the session registries are consulted only to ATTRIBUTE
# what was found to a project. That inversion is deliberate: stale
# `active-sessions.json` PIDs reporting false liveness is one of the three
# incidents this oracle exists to catch, so a registry can name residue but must
# never be the thing that detects it. Residue with no attribution is reported
# anonymously — never dropped.
#
# DETECTION SET IS THE RESERVED BANDS, NOT THE CLAIMED PORTS. The incident port
# (a Playwright preview on 14239) was never registered. A gate scoped to claimed
# ports would have been blind to exactly the residue that motivated it.
#
# NEVER ACTUATES. This oracle reports; it does not close, kill, or unbind
# anything. Warm worker sessions stay warm by design — two real defects in the
# originating incident were fixed by dispatching them back to their original
# authors, who still held full context. The defect is the missing SIGNAL, not
# the liveness.
#
# Test seam: ULDF_QUIESCE_FIXTURE=<file> supplies raw facts instead of probing
# the OS, so the classification+verdict logic is exercisable — and falsifiable —
# without needing a real noisy machine. Both twins honour it (TWIN-01).
#
# Spec: docs/specs/SPECIFICATION.md § QUIESCE. Decision: DEC-208. Brief: DEFER-043.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
DPR_LIB="$SCRIPT_DIR/../../scripts/lib/dev-port-registry.sh"

required_ports=()
timing_sensitive="false"

while [ $# -gt 0 ]; do
    case "$1" in
        --port)   shift; [ $# -gt 0 ] && required_ports+=("$1") ;;
        --port=*) required_ports+=("${1#*=}") ;;
        --timing) timing_sensitive="true" ;;
        --json)   : ;;  # JSON is the only output format; accepted for symmetry.
        -h|--help)
            sed -n '2,50p' "${BASH_SOURCE[0]:-$0}" | sed 's/^# \{0,1\}//'
            exit 0 ;;
        *) : ;;
    esac
    shift
done

degraded=()

jesc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g; s/\t/ /g'; }
json_str_array() {
    if [ "$#" -eq 0 ]; then printf '[]'; return; fi
    local out="[" first=1 i
    for i in "$@"; do
        if [ "$first" -eq 1 ]; then first=0; else out+=","; fi
        out+="\"$(jesc "$i")\""
    done
    printf '%s]' "$out"
}

# ---- Step 1: raw OS facts -------------------------------------------------
# TSV, one fact per line:
#   listener<TAB>port<TAB>pid<TAB>procName
#   process<TAB>pid<TAB>name<TAB>commandLine
#   degraded<TAB>reason
facts=""
if [ -n "${ULDF_QUIESCE_FIXTURE:-}" ]; then
    if [ -f "$ULDF_QUIESCE_FIXTURE" ]; then
        facts="$(cat "$ULDF_QUIESCE_FIXTURE")"
    else
        degraded+=("fixture not found: $ULDF_QUIESCE_FIXTURE")
    fi
else
    case "$(uname -s 2>/dev/null)" in
        MINGW*|MSYS*|CYGWIN*)
            # ONE shell-out to the shared probe — never a per-port loop.
            if [ -f "$SCRIPT_DIR/probe.ps1" ]; then
                facts="$(powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$SCRIPT_DIR/probe.ps1" 2>/dev/null)"
                [ -n "$facts" ] || degraded+=("Windows probe returned no facts")
            else
                degraded+=("probe.ps1 missing - cannot read machine state on Windows")
            fi
            ;;
        *)
            got_listeners=0
            if command -v lsof >/dev/null 2>&1; then
                facts="$(lsof -nP -iTCP -sTCP:LISTEN 2>/dev/null | awk 'NR>1 {
                    n = split($9, a, ":"); port = a[n]
                    if (port ~ /^[0-9]+$/) print "listener\t" port "\t" $2 "\t" $1
                }' | sort -u)"
                got_listeners=1
            elif command -v ss >/dev/null 2>&1; then
                facts="$(ss -ltnp 2>/dev/null | awk 'NR>1 {
                    n = split($4, a, ":"); port = a[n]
                    pid = "0"; pname = "?"
                    if (match($0, /pid=[0-9]+/)) pid = substr($0, RSTART+4, RLENGTH-4)
                    if (match($0, /\(\("[^"]+/)) pname = substr($0, RSTART+3, RLENGTH-3)
                    if (port ~ /^[0-9]+$/) print "listener\t" port "\t" pid "\t" pname
                }' | sort -u)"
                got_listeners=1
            elif command -v netstat >/dev/null 2>&1; then
                facts="$(netstat -ltn 2>/dev/null | awk '/LISTEN/ {
                    n = split($4, a, ":"); port = a[n]
                    if (port ~ /^[0-9]+$/) print "listener\t" port "\t0\t?"
                }' | sort -u)"
                got_listeners=1
                degraded+=("netstat fallback: listening sockets found but PIDs unavailable")
            fi
            [ "$got_listeners" -eq 1 ] || degraded+=("no listening-socket probe available (lsof/ss/netstat all absent)")

            if command -v ps >/dev/null 2>&1; then
                facts+=$'\n'"$(ps -eo pid=,comm=,args= 2>/dev/null | awk '{
                    cmd = ""
                    for (i = 3; i <= NF; i++) cmd = cmd (i > 3 ? " " : "") $i
                    print "process\t" $1 "\t" $2 "\t" cmd
                }')"
            else
                degraded+=("no process enumeration available (ps absent)")
            fi
            ;;
    esac
fi

# Adopt any degradation the probe itself declared.
while IFS=$'\t' read -r _k _rest; do
    [ "$_k" = "degraded" ] && degraded+=("$_rest")
done <<< "$facts"

# ---- Step 2: attribution sets (naming only, never detection) --------------
registry_ports=""
bands=""
default_ports=""
if [ -f "$DPR_LIB" ]; then
    # shellcheck source=/dev/null
    . "$DPR_LIB"
    registry_ports="$(dpr_rows 2>/dev/null | awk -F'\t' '{print $2"="$1}' | sort -u)"
    bands="$(dpr_bands 2>/dev/null | awk -F'\t' '{print $1" "$2}' | sort -u)"
    default_ports="$(dpr_default_ports)"
else
    degraded+=("dev-port-registry lib unavailable - reserved bands and project attribution unknown")
fi

req_list=""
if [ "${#required_ports[@]}" -gt 0 ]; then req_list="${required_ports[*]}"; fi

# ---- Step 3: classify (single awk pass — 700+ facts, no per-line subshells) -
# Emits:
#   R<TAB>sortkey<TAB><residue json>
#   B<TAB><blocking reason>
#   C<TAB>listeners<TAB>agents<TAB>builds
classified="$(printf '%s\n' "$facts" | awk -F'\t' \
    -v REG="$registry_ports" -v BANDS="$bands" -v DEF="$default_ports" \
    -v REQ="$req_list" -v SELF="${CLAUDE_PID:-}" '
    function jesc(s) { gsub(/\\/, "\\\\", s); gsub(/"/, "\\\"", s); return s }
    BEGIN {
        n = split(REG, a, "\n")
        for (i = 1; i <= n; i++) {
            if (a[i] == "") continue
            k = index(a[i], "=")
            if (k < 2) continue
            proj[substr(a[i], 1, k - 1) + 0] = substr(a[i], k + 1)
        }
        n = split(BANDS, b, "\n")
        for (i = 1; i <= n; i++) {
            if (b[i] == "") continue
            split(b[i], c, " ")
            nb++; blo[nb] = c[1] + 0; bhi[nb] = c[2] + 0
        }
        n = split(DEF, d, "\n")
        for (i = 1; i <= n; i++) if (d[i] != "") isdef[d[i] + 0] = 1
        n = split(REQ, r, " ")
        for (i = 1; i <= n; i++) if (r[i] != "") isreq[r[i] + 0] = 1

        # ---- Build/watch matching -------------------------------------------
        # Tokens are matched as COMMAND WORDS, never as bare substrings. A bare
        # substring scan produced 75 "build processes" on the authoring machine,
        # of which ~70 were the Playwright MCP server, its Chrome helpers, and
        # (self-referentially) the grep that was inspecting them -- residue by
        # path-spelling, not by activity. The success criterion is residue NAMED
        # usefully, so the anchors matter:
        #   pre  = start, or a char that cannot be part of a program name
        #          (excludes `@playwright/mcp` and `ms-playwright-mcp`)
        #   suf  = optional script/exe extension, then whitespace/quote/end
        #          (excludes `node_modules/vite/dist/...` — a path, not a call)
        pre = "(^|[^A-Za-z0-9._@-])"
        suf = "(\\.(js|mjs|cjs|exe|cmd))?([ \t\"]|$)"
        # SACT-04 (DEC-240): the list also matches heavy compile/container
        # toolchains, not just dev servers/watchers — a 41-minute docker build
        # was invisible to this oracle because the list stopped at dev tooling.
        words = "(vite|esbuild|rollup|webpack|nodemon|tsup|metro|vitest|jest|playwright|storybook|watchman|tsc|msbuild|gradlew|gradle|cmake|make|ninja)"
        cmdre = pre words suf
        # Multi-word invocations, where the first word alone is too common to
        # anchor on ("next", "cargo", "npm", "docker", "go").
        phrasere = "(tauri dev|next dev|expo start|cargo watch|turbo run dev|npm run dev|pnpm dev|yarn dev|tsc --watch|tsc -w|docker build|docker buildx|cargo build|cargo test|go build|dotnet build)"
        # Harness plumbing is never a dev server; excluding it keeps the oracle
        # from reporting the shell that is running the oracle.
        split("bash|sh|dash|zsh|grep|awk|sed|powershell|pwsh|conhost|windowsterminal|git|less|more", pl, "|")
        for (i in pl) plumbing[pl[i]] = 1

        nl = 0; na = 0; nbp = 0
    }
    $1 == "listener" {
        port = $2 + 0; pid = $3 + 0; pname = ($4 == "" ? "?" : $4)
        if (port <= 0) next
        via = ""
        if (port in isreq)      via = "required"
        else if (port in proj)  via = "registry"
        else {
            for (i = 1; i <= nb; i++) if (port >= blo[i] && port <= bhi[i]) { via = "band"; break }
            if (via == "" && (port in isdef)) via = "default"
        }
        if (via == "") next
        key = port "/" pid
        if (key in seenl) next
        seenl[key] = 1
        attr = (port in proj) ? "\"" jesc(proj[port]) "\"" : "null"
        nl++
        printf "R\t1-%06d-%06d\t{\"kind\":\"dev-port-listener\",\"port\":%d,\"pid\":%d,\"process\":\"%s\",\"attributedTo\":%s,\"via\":\"%s\"}\n", port, pid, port, pid, jesc(pname), attr, via
        if (via == "required")
            printf "B\tport %d is required free by this measurement but is held by PID %d (%s)\n", port, pid, jesc(pname)
        next
    }
    $1 == "process" {
        pid = $2 + 0; name = ($3 == "" ? "?" : $3); cmd = $4
        if (pid <= 0) next
        if (SELF != "" && pid == SELF + 0) next
        lname = tolower(name); lcmd = tolower(cmd)
        sub(/\.exe$/, "", lname)
        if (lname == "claude") {
            na++
            printf "R\t2-%06d-000000\t{\"kind\":\"agent-session\",\"pid\":%d,\"process\":\"%s\"}\n", pid, pid, jesc(name)
            next
        }
        if (lname in plumbing) next
        tok = ""
        if (match(lcmd, phrasere)) {
            tok = substr(lcmd, RSTART, RLENGTH)
        } else if (match(lcmd, cmdre)) {
            tok = substr(lcmd, RSTART, RLENGTH)
            gsub(/^[^A-Za-z0-9]+|[^A-Za-z0-9]+$/, "", tok)
        }
        if (tok != "") {
            nbp++
            printf "R\t3-%06d-000000\t{\"kind\":\"build-process\",\"pid\":%d,\"process\":\"%s\",\"matched\":\"%s\"}\n", pid, pid, jesc(name), jesc(tok)
        }
        next
    }
    END { printf "C\t%d\t%d\t%d\n", nl, na, nbp }
')"

n_listeners=0; n_agents=0; n_builds=0
blocking_reasons=()
while IFS=$'\t' read -r tag v1 v2 v3; do
    case "$tag" in
        C) n_listeners="$v1"; n_agents="$v2"; n_builds="$v3" ;;
        B) blocking_reasons+=("$v1") ;;
    esac
done <<< "$classified"

total_residue=$((n_listeners + n_agents + n_builds))

if [ "$timing_sensitive" = "true" ] && [ "$total_residue" -gt 0 ]; then
    blocking_reasons+=("timing-sensitive measurement requested while $total_residue residue item(s) are live ($n_listeners listener(s), $n_agents agent session(s), $n_builds build/watch process(es)) - machine load moves timing figures independently of any code change")
fi

# ---- Step 4: verdict (fail closed) ----------------------------------------
if [ "${#degraded[@]}" -gt 0 ]; then
    verdict="unknown"; code=2
    summary="UNKNOWN - the machine could not be read (${degraded[0]}). Treat as a refusal: a probe that cannot see residue must not report quiet."
elif [ "${#blocking_reasons[@]}" -gt 0 ]; then
    verdict="blocking"; code=3
    summary="BLOCKING - ${blocking_reasons[0]}"
elif [ "$total_residue" -gt 0 ]; then
    verdict="noisy"; code=1
    summary="NOISY - $n_listeners dev-port listener(s), $n_agents live agent session(s), $n_builds build/watch process(es). Survivable for a pass/fail run; not for a timing measurement."
else
    verdict="quiet"; code=0
    summary="QUIET - no dev-port listeners, foreign agent sessions, or build/watch processes detected."
fi

# ---- Step 5: emit ---------------------------------------------------------
residue_json="$(printf '%s\n' "$classified" | awk -F'\t' '
    $1 == "R" { keys[NR] = $2 "\x01" $3 }
    END {
        n = 0
        for (i in keys) { arr[++n] = keys[i] }
        # Insertion sort on the composite key — deterministic order is a parity
        # requirement (both twins must emit residue in the same sequence).
        for (i = 2; i <= n; i++) {
            v = arr[i]; j = i - 1
            while (j > 0 && arr[j] > v) { arr[j+1] = arr[j]; j-- }
            arr[j+1] = v
        }
        out = "["
        for (i = 1; i <= n; i++) {
            split(arr[i], p, "\x01")
            out = out (i > 1 ? "," : "") p[2]
        }
        print out "]"
    }
')"
[ -n "$residue_json" ] || residue_json="[]"

req_json="["
if [ "${#required_ports[@]}" -gt 0 ]; then
    first=1
    for p in "${required_ports[@]}"; do
        if [ "$first" -eq 1 ]; then first=0; else req_json+=","; fi
        req_json+="$((p))"
    done
fi
req_json+="]"

printf '{"schemaVersion":1,"verdict":"%s","checkedAt":"%s","requiredPorts":%s,"timingSensitive":%s,"counts":{"devPortListeners":%d,"agentSessions":%d,"buildProcesses":%d},"residue":%s,"blockingReasons":%s,"degraded":%s,"summary":"%s"}\n' \
    "$verdict" \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    "$req_json" \
    "$timing_sensitive" \
    "$n_listeners" "$n_agents" "$n_builds" \
    "$residue_json" \
    "$(json_str_array ${blocking_reasons[@]+"${blocking_reasons[@]}"})" \
    "$(json_str_array ${degraded[@]+"${degraded[@]}"})" \
    "$(jesc "$summary")"

exit "$code"
