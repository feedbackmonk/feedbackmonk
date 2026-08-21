# jig-demand oracle (Windows) -- JIG-09 (the conversion side of the Jig doctrine)
#
# Twin of run.sh. See that file's header for the mechanism; the clustering
# algorithm itself lives in cluster.py and is invoked by BOTH twins, so this
# file owns only argument/config resolution, interpreter discovery and the
# not-measured fallback.

[CmdletBinding()]
param(
    [int]    $MinCandidates = 0,
    [int]    $MinOccasions  = 0,
    [double] $Similarity    = 0,
    [int]    $TopN          = 0,
    [string] $Log           = ".claude/session-state/aria-probe-candidates.jsonl",
    [string] $Dispositions  = ".claude/session-state/jig-demand-dispositions.jsonl",
    [switch] $SelfTest
)

$ErrorActionPreference = 'Continue'

$thisDir = Split-Path -Parent $MyInvocation.MyCommand.Path

if ($SelfTest) {
    & (Join-Path $thisDir 'validate.ps1')
    exit $LASTEXITCODE
}

function Write-NotMeasured([string]$why) {
    # status:ok here means "not measured", never "nothing found". `clustered`
    # is the field that distinguishes them and the briefing says so out loud.
    $o = [ordered]@{
        status  = 'ok'
        clusters = @()
        totals  = [ordered]@{ candidates = 0; dispositioned = 0; undispositioned = 0 }
        clustered = $false
        briefing = "jig-demand: NOT MEASURED -- $why (clustered:false)"
    }
    $o | ConvertTo-Json -Depth 6 -Compress
}

# Locate a working python (the WindowsApps `python3` shim is a broken stub --
# probe it, do not trust its presence on PATH).
$py = $null
foreach ($c in @('python', 'python3', 'py')) {
    $cmd = Get-Command $c -ErrorAction SilentlyContinue
    if ($null -eq $cmd) { continue }
    try {
        & $c -c 'import json,sys' 2>$null | Out-Null
        if ($LASTEXITCODE -eq 0) { $py = $c; break }
    } catch { }
}
if (-not $py) { Write-NotMeasured 'no python interpreter available, so no recurrence clustering ran'; exit 0 }

# Config precedence: parameter > .claude/config.json jigDemand.* > default (cluster.py).
$cfgPath = '.claude/config.json'
if (Test-Path -LiteralPath $cfgPath) {
    try {
        $cfg = Get-Content -LiteralPath $cfgPath -Raw -Encoding UTF8 | ConvertFrom-Json
        $j = $cfg.jigDemand
        if ($null -ne $j) {
            if ($MinCandidates -le 0 -and $null -ne $j.minCandidates) { $MinCandidates = [int]$j.minCandidates }
            if ($MinOccasions  -le 0 -and $null -ne $j.minOccasions)  { $MinOccasions  = [int]$j.minOccasions }
            if ($Similarity    -le 0 -and $null -ne $j.similarity)    { $Similarity    = [double]$j.similarity }
            if ($TopN          -le 0 -and $null -ne $j.topN)          { $TopN          = [int]$j.topN }
        }
    } catch { }
}

# Empty string => cluster.py uses its own default. Never pass a 0 through.
$env:JIGD_MIN_CANDIDATES = if ($MinCandidates -gt 0) { "$MinCandidates" } else { '' }
$env:JIGD_MIN_OCCASIONS  = if ($MinOccasions  -gt 0) { "$MinOccasions" }  else { '' }
$env:JIGD_SIMILARITY     = if ($Similarity    -gt 0) { "$([string]::Format([System.Globalization.CultureInfo]::InvariantCulture,'{0}',$Similarity))" } else { '' }
$env:JIGD_TOP_N          = if ($TopN          -gt 0) { "$TopN" }          else { '' }

$script = Join-Path $thisDir 'cluster.py'
try {
    $out = & $py $script $Log $Dispositions 2>$null
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($out)) {
        Write-NotMeasured 'clustering pass failed'
        exit 0
    }
    Write-Output $out
} catch {
    Write-NotMeasured 'clustering pass failed'
}
exit 0
