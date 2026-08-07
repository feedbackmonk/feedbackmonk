# harness-capabilities oracle (Windows) -- HAL-01 (section HAL, DEC-156)
#
# Byte-parallel with run.sh (same detection model, same floor table, byte-identical
# JSON for identical inputs). See run.sh header for the full contract: env-signal
# harness detection, `claude --version` version detection, version->capability
# floor table (the ONLY home of that knowledge -- consuming skills branch only via
# segments/_hal-gate.md), conservative floors for rolling features, and the
# undetectable=>false / unknown-harness=>all-false-VALID semantics
# (claude-template/templates/HAL_CAPABILITY_MAP.md sections 2-3).
#
# TEST SEAMS (validators only; never set in normal operation):
#   HARNESS_CAPS_FORCE_VERSION  raw version-string candidate; replaces the
#                               `claude --version` call (unparseable => null)
#   HARNESS_CAPS_NOW            pinned detectedAt timestamp (golden fixtures)
#
# JSON is hand-assembled (not ConvertTo-Json) to stay byte-identical with the
# bash oracle. ASCII-only strings (PW-005 lineage: no em-dashes).

$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$now = if ($env:HARNESS_CAPS_NOW) { $env:HARNESS_CAPS_NOW } else { (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ') }

# ---- 1. Harness detection ---------------------------------------------------
$harness = 'unknown'
if ($env:CLAUDECODE -eq '1' -or -not [string]::IsNullOrEmpty($env:CLAUDE_CODE_SESSION_ID)) {
    $harness = 'claude-code'
}

# ---- 2. Version detection (only for a detected harness) --------------------
$version = ''    # empty = undetectable (emitted as null)
if ($harness -eq 'claude-code') {
    if ($null -ne $env:HARNESS_CAPS_FORCE_VERSION) {
        $raw = $env:HARNESS_CAPS_FORCE_VERSION
    } else {
        $raw = (& claude --version 2>$null) -join ' '
    }
    $m = [regex]::Match([string]$raw, '\d+\.\d+\.\d+')
    if ($m.Success) { $version = $m.Value }
}

# ---- 3. Version comparison --------------------------------------------------
function Ver-Ge([string]$floor) {
    if ([string]::IsNullOrEmpty($script:version)) { return $false }
    $v = $script:version.Split('.'); $f = $floor.Split('.')
    for ($i = 0; $i -lt 3; $i++) {
        $vi = [int]($v[$i]); $fi = [int]($f[$i])
        if ($vi -ne $fi) { return ($vi -gt $fi) }
    }
    return $true
}
function B([bool]$x) { if ($x) { 'true' } else { 'false' } }

# ---- 4. Capability bits (frozen spellings; floor table = HAL-01) ------------
$workflowTool        = B ((Ver-Ge '2.1.154') -and ($env:CLAUDE_CODE_DISABLE_WORKFLOWS -ne '1'))
$nativeWorktrees     = B (Ver-Ge '2.1.205')
$monitor             = B (Ver-Ge '2.1.195')
$monitorWebsocket    = B (Ver-Ge '2.1.195')
$sendMessage         = B (Ver-Ge '2.1.198')
$sharedTaskList      = B (Ver-Ge '2.1.142')
$bundledReviewSkills = B (Ver-Ge '2.1.205')
$loopScheduleWakeup  = B (Ver-Ge '2.1.202')
$cronLocal           = B (Ver-Ge '2.1.202')
$agentTeams          = B ((Ver-Ge '2.1.178') -and ($env:CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS -eq '1'))
$forkSubagents       = B (Ver-Ge '2.1.206')
$backgroundSubagents = B (Ver-Ge '2.1.198')

# ---- 5. Compose ---------------------------------------------------------------
$bits = @($workflowTool, $nativeWorktrees, $monitor, $monitorWebsocket,
          $sendMessage, $sharedTaskList, $bundledReviewSkills,
          $loopScheduleWakeup, $cronLocal, $agentTeams, $forkSubagents,
          $backgroundSubagents)
$trueCount = ($bits | Where-Object { $_ -eq 'true' }).Count

if ($harness -eq 'unknown') {
    $briefing = 'harness-capabilities: unknown harness - all capabilities false, ULDF paths (valid result, not an error)'
    $versionJson = 'null'
} elseif ([string]::IsNullOrEmpty($version)) {
    $briefing = 'harness-capabilities: claude-code, version undetectable - all version-keyed capabilities false'
    $versionJson = 'null'
} else {
    $briefing = "harness-capabilities: claude-code $version - $trueCount/12 capabilities native"
    $versionJson = '"' + $version + '"'
}

$json = '{"schemaVersion":"1","harness":"' + $harness + '","version":' + $versionJson +
        ',"detectedAt":"' + $now + '","capabilities":{' +
        '"workflowTool":' + $workflowTool +
        ',"nativeWorktrees":' + $nativeWorktrees +
        ',"monitor":' + $monitor +
        ',"monitorWebsocket":' + $monitorWebsocket +
        ',"sendMessage":' + $sendMessage +
        ',"sharedTaskList":' + $sharedTaskList +
        ',"bundledReviewSkills":' + $bundledReviewSkills +
        ',"loopScheduleWakeup":' + $loopScheduleWakeup +
        ',"cronLocal":' + $cronLocal +
        ',"agentTeams":' + $agentTeams +
        ',"forkSubagents":' + $forkSubagents +
        ',"backgroundSubagents":' + $backgroundSubagents +
        '},"briefing":"' + $briefing + '"}'
Write-Output $json
exit 0
