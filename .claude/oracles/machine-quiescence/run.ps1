# machine-quiescence oracle (Windows PowerShell) — QUIESCE-02/03/04
#
# "Is this machine clean enough that a number taken now means anything?" —
# answered deterministically, with the residue NAMED (which port, which PID,
# which process) rather than as a bare boolean.
#
# Usage:
#   run.ps1 [-Port <int[]>] [-Timing] [-Json]
#
#   -Port    Ports this measurement needs free. A live listener on one of these
#            is BLOCKING: an already-listening preview server makes Playwright's
#            `reuseExistingServer` skip the webServer command entirely, so the
#            build never runs and the suite grades new code against the previous
#            commit's bundle.
#   -Timing  This measurement is load-sensitive. Any residue then becomes
#            BLOCKING — four idle-but-alive agent sessions moved an incumbent
#            renderer's own figures by ~20-25% in the motivating incident.
#
# Exit codes (1:1 with `verdict`):
#   0 quiet | 1 noisy | 2 unknown (FAILS CLOSED) | 3 blocking
#
# GROUND TRUTH IS THE OS. Detection is listening sockets and live processes; the
# registries only ATTRIBUTE what was found. NEVER ACTUATES — warm worker
# sessions stay warm by design.
#
# TWIN-01: this file and run.sh must agree over one fixture (ULDF_QUIESCE_FIXTURE).
# Parity is asserted by scripts/smoke-tests/machine-quiescence-smoke.sh.
# JSON is hand-built rather than ConvertTo-Json so both twins emit byte-identical
# output (ConvertTo-Json escapes apostrophes to ' and reorders nothing
# predictably across engines).
#
# Spec: docs/specs/SPECIFICATION.md § QUIESCE. Decision: DEC-208. Brief: DEFER-043.

[CmdletBinding()]
param(
    [int[]]$Port = @(),
    [switch]$Timing,
    [switch]$Json
)

$ErrorActionPreference = "Continue"
try {
    [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
    $OutputEncoding = [System.Text.UTF8Encoding]::new()
} catch {}

$degraded = New-Object System.Collections.ArrayList
$blocking = New-Object System.Collections.ArrayList
$residue  = New-Object System.Collections.ArrayList   # @{ key; json }

function ConvertTo-JEsc {
    param([string]$s)
    if ($null -eq $s) { return "" }
    $s = $s -replace '\\', '\\'
    $s = $s -replace '"', '\"'
    $s = $s -replace "`t", ' '
    return $s
}
function ConvertTo-JStrArray {
    param([object[]]$items)
    if (-not $items -or $items.Count -eq 0) { return "[]" }
    return "[" + (($items | ForEach-Object { '"' + (ConvertTo-JEsc $_) + '"' }) -join ",") + "]"
}

# ---- Step 1: raw OS facts -------------------------------------------------
$facts = @()
if ($env:ULDF_QUIESCE_FIXTURE) {
    if (Test-Path -LiteralPath $env:ULDF_QUIESCE_FIXTURE -PathType Leaf) {
        $facts = Get-Content -LiteralPath $env:ULDF_QUIESCE_FIXTURE -Encoding UTF8
    } else {
        [void]$degraded.Add("fixture not found: $($env:ULDF_QUIESCE_FIXTURE)")
    }
} else {
    $probe = Join-Path $PSScriptRoot "probe.ps1"
    if (Test-Path -LiteralPath $probe -PathType Leaf) {
        $raw = & $probe
        if ($raw) { $facts = ($raw -split "`n") | ForEach-Object { $_.TrimEnd("`r") } }
        if (-not $facts -or $facts.Count -eq 0) { [void]$degraded.Add("Windows probe returned no facts") }
    } else {
        [void]$degraded.Add("probe.ps1 missing - cannot read machine state on Windows")
    }
}

foreach ($line in $facts) {
    if ($line -like "degraded`t*") { [void]$degraded.Add(($line -split "`t", 2)[1]) }
}

# ---- Step 2: attribution sets (naming only, never detection) --------------
$projByPort = @{}
$bands = @()
$defaults = @{}
$dprLib = Join-Path $PSScriptRoot "../../scripts/lib/dev-port-registry.ps1"
if (Test-Path -LiteralPath $dprLib -PathType Leaf) {
    . $dprLib
    foreach ($r in (Get-DprRows)) { if (-not $projByPort.ContainsKey($r.port)) { $projByPort[$r.port] = $r.project } }
    $bands = @(Get-DprBands)
    foreach ($p in (Get-DprDefaultPorts)) { $defaults[$p] = $true }
} else {
    [void]$degraded.Add("dev-port-registry lib unavailable - reserved bands and project attribution unknown")
}

$required = @{}
foreach ($p in $Port) { $required[[int]$p] = $true }

# ---- Step 3: classify -----------------------------------------------------
# Build/watch tokens are matched as COMMAND WORDS, never as bare substrings.
# See run.sh for the full rationale (a substring scan reported the Playwright
# MCP server and its Chrome helpers as 70 "build processes").
$pre      = '(^|[^A-Za-z0-9._@-])'
$suf      = '(\.(js|mjs|cjs|exe|cmd))?([ \t"]|$)'
# SACT-04 (DEC-240): heavy compile/container toolchains included — see run.sh.
$words    = '(vite|esbuild|rollup|webpack|nodemon|tsup|metro|vitest|jest|playwright|storybook|watchman|tsc|msbuild|gradlew|gradle|cmake|make|ninja)'
$cmdRe    = $pre + $words + $suf
$phraseRe = '(tauri dev|next dev|expo start|cargo watch|turbo run dev|npm run dev|pnpm dev|yarn dev|tsc --watch|tsc -w|docker build|docker buildx|cargo build|cargo test|go build|dotnet build)'
$plumbing = @{}
foreach ($n in @('bash','sh','dash','zsh','grep','awk','sed','powershell','pwsh','conhost','windowsterminal','git','less','more')) { $plumbing[$n] = $true }

$selfPid = 0
if ($env:CLAUDE_PID) { [void][int]::TryParse($env:CLAUDE_PID, [ref]$selfPid) }

$nListeners = 0; $nAgents = 0; $nBuilds = 0
$seenListener = @{}

foreach ($line in $facts) {
    if (-not $line) { continue }
    $f = $line -split "`t", 4
    switch ($f[0]) {
        'listener' {
            if ($f.Count -lt 3) { break }
            $lp = 0; $lpid = 0
            [void][int]::TryParse($f[1], [ref]$lp)
            [void][int]::TryParse($f[2], [ref]$lpid)
            if ($lp -le 0) { break }
            $pname = if ($f.Count -ge 4 -and $f[3]) { $f[3] } else { "?" }

            $via = ""
            if ($required.ContainsKey($lp))         { $via = "required" }
            elseif ($projByPort.ContainsKey($lp))   { $via = "registry" }
            else {
                foreach ($b in $bands) { if ($lp -ge $b.lo -and $lp -le $b.hi) { $via = "band"; break } }
                if (-not $via -and $defaults.ContainsKey($lp)) { $via = "default" }
            }
            if (-not $via) { break }

            $key = "$lp/$lpid"
            if ($seenListener.ContainsKey($key)) { break }
            $seenListener[$key] = $true

            $attr = if ($projByPort.ContainsKey($lp)) { '"' + (ConvertTo-JEsc $projByPort[$lp]) + '"' } else { "null" }
            $nListeners++
            [void]$residue.Add(@{
                key  = ("1-{0:d6}-{1:d6}" -f $lp, $lpid)
                json = '{"kind":"dev-port-listener","port":' + $lp + ',"pid":' + $lpid + ',"process":"' + (ConvertTo-JEsc $pname) + '","attributedTo":' + $attr + ',"via":"' + $via + '"}'
            })
            if ($via -eq "required") {
                [void]$blocking.Add("port $lp is required free by this measurement but is held by PID $lpid ($pname)")
            }
        }
        'process' {
            if ($f.Count -lt 3) { break }
            $ppid = 0
            [void][int]::TryParse($f[1], [ref]$ppid)
            if ($ppid -le 0) { break }
            if ($selfPid -gt 0 -and $ppid -eq $selfPid) { break }
            $name = if ($f[2]) { $f[2] } else { "?" }
            $cmd  = if ($f.Count -ge 4) { $f[3] } else { "" }
            $lname = $name.ToLowerInvariant() -replace '\.exe$', ''
            $lcmd = $cmd.ToLowerInvariant()

            if ($lname -eq 'claude') {
                $nAgents++
                [void]$residue.Add(@{
                    key  = ("2-{0:d6}-000000" -f $ppid)
                    json = '{"kind":"agent-session","pid":' + $ppid + ',"process":"' + (ConvertTo-JEsc $name) + '"}'
                })
                break
            }
            if ($plumbing.ContainsKey($lname)) { break }

            $tok = ""
            $m = [regex]::Match($lcmd, $phraseRe)
            if ($m.Success) {
                $tok = $m.Value
            } else {
                $m = [regex]::Match($lcmd, $cmdRe)
                if ($m.Success) {
                    $tok = $m.Value -replace '^[^A-Za-z0-9]+', '' -replace '[^A-Za-z0-9]+$', ''
                }
            }
            if ($tok) {
                $nBuilds++
                [void]$residue.Add(@{
                    key  = ("3-{0:d6}-000000" -f $ppid)
                    json = '{"kind":"build-process","pid":' + $ppid + ',"process":"' + (ConvertTo-JEsc $name) + '","matched":"' + (ConvertTo-JEsc $tok) + '"}'
                })
            }
        }
    }
}

$totalResidue = $nListeners + $nAgents + $nBuilds
if ($Timing -and $totalResidue -gt 0) {
    [void]$blocking.Add("timing-sensitive measurement requested while $totalResidue residue item(s) are live ($nListeners listener(s), $nAgents agent session(s), $nBuilds build/watch process(es)) - machine load moves timing figures independently of any code change")
}

# ---- Step 4: verdict (fail closed) ----------------------------------------
if ($degraded.Count -gt 0) {
    $verdict = "unknown"; $code = 2
    $summary = "UNKNOWN - the machine could not be read ($($degraded[0])). Treat as a refusal: a probe that cannot see residue must not report quiet."
} elseif ($blocking.Count -gt 0) {
    $verdict = "blocking"; $code = 3
    $summary = "BLOCKING - $($blocking[0])"
} elseif ($totalResidue -gt 0) {
    $verdict = "noisy"; $code = 1
    $summary = "NOISY - $nListeners dev-port listener(s), $nAgents live agent session(s), $nBuilds build/watch process(es). Survivable for a pass/fail run; not for a timing measurement."
} else {
    $verdict = "quiet"; $code = 0
    $summary = "QUIET - no dev-port listeners, foreign agent sessions, or build/watch processes detected."
}

# ---- Step 5: emit ---------------------------------------------------------
# Ordinal sort on the composite key — deterministic order is a parity requirement.
$sorted = @($residue | Sort-Object -Property @{ Expression = { $_.key } })
$residueJson = "[" + (($sorted | ForEach-Object { $_.json }) -join ",") + "]"
$reqJson = "[" + (($Port | ForEach-Object { [int]$_ }) -join ",") + "]"

$out = '{"schemaVersion":1,"verdict":"' + $verdict + '"' +
       ',"checkedAt":"' + (Get-Date).ToUniversalTime().ToString("yyyy-MM-dd'T'HH:mm:ss'Z'", [System.Globalization.CultureInfo]::InvariantCulture) + '"' +
       ',"requiredPorts":' + $reqJson +
       ',"timingSensitive":' + $(if ($Timing) { "true" } else { "false" }) +
       ',"counts":{"devPortListeners":' + $nListeners + ',"agentSessions":' + $nAgents + ',"buildProcesses":' + $nBuilds + '}' +
       ',"residue":' + $residueJson +
       ',"blockingReasons":' + (ConvertTo-JStrArray $blocking.ToArray()) +
       ',"degraded":' + (ConvertTo-JStrArray $degraded.ToArray()) +
       ',"summary":"' + (ConvertTo-JEsc $summary) + '"}'

Write-Output $out
exit $code
