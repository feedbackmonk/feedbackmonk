# ltads-state oracle self-test (Windows PowerShell)
# Legs: (1) live-project run emits valid JSON with the full field set (pre-ARC
# fields preserved + ARC-01/06 additive fields); (2-5) fixture verdicts:
# valid/CONTINUATION, concluded/FRESH, malformed/invalid, prose-only/legacy.
$ErrorActionPreference = "Stop"
$oracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$output = & powershell -NoProfile -File (Join-Path $oracleDir "run.ps1") 2>$null

try {
    $parsed = $output | ConvertFrom-Json
} catch {
    Write-Error "FAIL: output is not valid JSON"
    exit 1
}

$requiredFields = @("state", "has_ltads_dir", "is_tracked", "config_exists", "is_temporary", "cleanup_candidate", "session_id", "session_status", "summary", "arc_state", "arc_mode", "topmost_arc", "size_warning")
foreach ($field in $requiredFields) {
    if (-not ($parsed.PSObject.Properties.Name -contains $field)) {
        Write-Error "FAIL: missing schema field '$field'"
        exit 1
    }
}

# ---- Fixture verdict legs ---------------------------------------------------
$fix = Join-Path ([System.IO.Path]::GetTempPath()) ("ltads-state-validate-" + [System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $fix -Force | Out-Null
try {
    function Invoke-OracleIn {
        param([string]$Dir)
        Push-Location $Dir
        try {
            $out = & powershell -NoProfile -File (Join-Path $oracleDir "run.ps1") 2>$null
            return ($out | ConvertFrom-Json)
        } finally {
            Pop-Location
        }
    }

    # (2) valid + topmost ACTIVE -> valid / CONTINUATION / topmost reported
    New-Item -ItemType Directory -Path (Join-Path $fix "valid\ltads") -Force | Out-Null
    Set-Content -Path (Join-Path $fix "valid\ltads\config.json") -Value '{"temporary": false}'
    Set-Content -Path (Join-Path $fix "valid\ltads\arc-state.json") -Value '{"schemaVersion":1,"arcs":[{"id":"A007","status":"ACTIVE","started":"2026-07-22T10:00:00Z","checkpoints":[{"at":"2026-07-22T11:00:00Z","by":"sess-1"}]}]}'
    $r = Invoke-OracleIn (Join-Path $fix "valid")
    if ($r.arc_state -ne "valid") { Write-Error "FAIL: valid fixture arc_state: got '$($r.arc_state)'"; exit 1 }
    if ($r.arc_mode -ne "CONTINUATION") { Write-Error "FAIL: valid fixture arc_mode: got '$($r.arc_mode)'"; exit 1 }
    if ($r.topmost_arc.id -ne "A007") { Write-Error "FAIL: valid fixture topmost id: got '$($r.topmost_arc.id)'"; exit 1 }
    if ($r.session_id -ne "A007") { Write-Error "FAIL: valid fixture session_id from arc: got '$($r.session_id)'"; exit 1 }

    # (3) topmost CONCLUDED -> FRESH (ARC-06 fresh-next-arc)
    New-Item -ItemType Directory -Path (Join-Path $fix "fresh\ltads") -Force | Out-Null
    Set-Content -Path (Join-Path $fix "fresh\ltads\config.json") -Value '{"temporary": false}'
    Set-Content -Path (Join-Path $fix "fresh\ltads\arc-state.json") -Value '{"schemaVersion":1,"arcs":[{"id":"A008","status":"CONCLUDED","started":"2026-07-22T10:00:00Z","checkpoints":[],"concludedBy":{"sessionId":"sess-1","at":"2026-07-22T12:00:00Z","via":"arc-terminus"}}]}'
    $r = Invoke-OracleIn (Join-Path $fix "fresh")
    if ($r.arc_mode -ne "FRESH") { Write-Error "FAIL: concluded fixture arc_mode: got '$($r.arc_mode)'"; exit 1 }

    # (4) malformed -> invalid, arc_mode null (never guess)
    New-Item -ItemType Directory -Path (Join-Path $fix "bad\ltads") -Force | Out-Null
    Set-Content -Path (Join-Path $fix "bad\ltads\config.json") -Value '{"temporary": false}'
    Set-Content -Path (Join-Path $fix "bad\ltads\arc-state.json") -Value 'garbage{'
    $r = Invoke-OracleIn (Join-Path $fix "bad")
    if ($r.arc_state -ne "invalid") { Write-Error "FAIL: malformed fixture arc_state: got '$($r.arc_state)'"; exit 1 }
    if ($null -ne $r.arc_mode) { Write-Error "FAIL: malformed fixture arc_mode should be null: got '$($r.arc_mode)'"; exit 1 }

    # (5) prose-only current-session.md -> legacy (ARC-11 surfacing trigger)
    New-Item -ItemType Directory -Path (Join-Path $fix "legacy\ltads\sessions") -Force | Out-Null
    Set-Content -Path (Join-Path $fix "legacy\ltads\config.json") -Value '{"temporary": false}'
    Set-Content -Path (Join-Path $fix "legacy\ltads\sessions\current-session.md") -Value "# Session S042`n`n**ID**: S042`n**Status**: IN_PROGRESS"
    $r = Invoke-OracleIn (Join-Path $fix "legacy")
    if ($r.arc_state -ne "legacy") { Write-Error "FAIL: prose-only fixture arc_state: got '$($r.arc_state)'"; exit 1 }
} finally {
    Remove-Item -Path $fix -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Output "PASS: ltads-state oracle validates"
exit 0
