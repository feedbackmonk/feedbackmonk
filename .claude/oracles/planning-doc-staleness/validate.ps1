# planning-doc-staleness oracle self-test (Windows PowerShell)
#
# Cases:
#   T1. shipped-via-commit
#   T2. shipped-via-spec-status
#   T3. shipped-via-both
#   T4. in-flight (fresh)
#   T5. mixed
#   T6. no-planning-dir
#   T7. malformed (zero-byte, old mtime → unknown)

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()

$OracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path

$Pass = 0
$Fail = 0
function Mark-Pass($msg) { Write-Host "PASS: $msg"; $script:Pass++ }
function Mark-Fail($msg) { Write-Host "FAIL: $msg" -ForegroundColor Red; $script:Fail++ }

function New-Sandbox {
    $sb = Join-Path ([System.IO.Path]::GetTempPath()) ([System.IO.Path]::GetRandomFileName())
    New-Item -ItemType Directory -Path $sb | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $sb ".claude/oracles/planning-doc-staleness") -Force | Out-Null
    Copy-Item (Join-Path $OracleDir "run.ps1") (Join-Path $sb ".claude/oracles/planning-doc-staleness/run.ps1")
    Copy-Item (Join-Path $OracleDir "oracle.json") (Join-Path $sb ".claude/oracles/planning-doc-staleness/oracle.json")
    New-Item -ItemType Directory -Path (Join-Path $sb "docs/planning/intakes") -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $sb "docs/planning/plans") -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $sb "docs/specs") -Force | Out-Null
    Push-Location $sb
    try {
        & git init -q 2>$null
        & git config user.email "t@t" 2>$null
        & git config user.name "t" 2>$null
    } finally { Pop-Location }
    return $sb
}

function Remove-Sandbox($sb) {
    Remove-Item -LiteralPath $sb -Recurse -Force -ErrorAction SilentlyContinue
}

function Set-Backdate($path, $days) {
    $when = [DateTime]::UtcNow.AddDays(-1 * $days)
    [System.IO.File]::SetLastWriteTimeUtc($path, $when)
}

function Run-Oracle($sb) {
    Push-Location $sb
    try {
        $out = & powershell -NoProfile -File ".claude/oracles/planning-doc-staleness/run.ps1" 2>&1
        return ($out | Out-String).Trim()
    } finally {
        Pop-Location
    }
}

function Assert-Json($json, $expr, $expect, $label) {
    try {
        $obj = $json | ConvertFrom-Json
        $actual = & ([scriptblock]::Create($expr))
        $actualStr = if ($null -eq $actual) { "" } else { $actual.ToString() }
    } catch {
        $actualStr = "<<error: $_>>"
    }
    if ($actualStr -eq $expect) {
        Mark-Pass $label
    } else {
        Mark-Fail "$label (expected '$expect', got '$actualStr')"
        Write-Host "Output: $json"
    }
}

# T1: shipped-via-commit
$sb = New-Sandbox
$file = Join-Path $sb "docs/planning/intakes/20260101T000000-feature-foo.md"
"stub" | Out-File -LiteralPath $file -Encoding utf8
Set-Backdate $file 30
Push-Location $sb
& git add -A 2>$null
& git commit -q -m "feat(foo): ship feature-foo" --allow-empty 2>$null
Pop-Location
$out = Run-Oracle $sb
Assert-Json $out '$obj.stale[0].staleness_signal' "commit-hash-found" "T1: commit-hash-found signal"
Assert-Json $out '@($obj.stale).Count' "1" "T1: exactly one stale entry"
Assert-Json $out '@($obj.fresh).Count' "0" "T1: no fresh entries"
Remove-Sandbox $sb

# T2: shipped-via-spec-status
$sb = New-Sandbox
$spec = Join-Path $sb "docs/specs/SPECIFICATION.md"
@"
# Spec
#### TEST-01: Test entry [DONE]
**Description**: shipped.
#### TEST-02: Other [DONE]
**Description**: shipped too.
"@ | Out-File -LiteralPath $spec -Encoding utf8
$file = Join-Path $sb "docs/planning/plans/20260201T000000-bar.md"
"# Plan`nReferences: TEST-01 and TEST-02.`n" | Out-File -LiteralPath $file -Encoding utf8
Set-Backdate $file 60
Push-Location $sb; & git commit -q --allow-empty -m "unrelated commit message" 2>$null; Pop-Location
$out = Run-Oracle $sb
Assert-Json $out '$obj.stale[0].staleness_signal' "all-spec-entries-done" "T2: all-spec-entries-done signal"
Assert-Json $out '@($obj.stale).Count' "1" "T2: exactly one stale entry"
Remove-Sandbox $sb

# T3: shipped-via-both
$sb = New-Sandbox
$spec = Join-Path $sb "docs/specs/SPECIFICATION.md"
"#### XYZ-01: Done [DONE]" | Out-File -LiteralPath $spec -Encoding utf8
$file = Join-Path $sb "docs/planning/intakes/20260101T000000-baz-feature.md"
"# Intake`nRefs: XYZ-01`n" | Out-File -LiteralPath $file -Encoding utf8
Set-Backdate $file 60
Push-Location $sb
& git add -A 2>$null
& git commit -q -m "ship baz-feature work" --allow-empty 2>$null
Pop-Location
$out = Run-Oracle $sb
Assert-Json $out '$obj.stale[0].staleness_signal' "both" "T3: both signals"
Remove-Sandbox $sb

# T4: in-flight
$sb = New-Sandbox
$spec = Join-Path $sb "docs/specs/SPECIFICATION.md"
"#### INF-01: Pending [PLANNED]" | Out-File -LiteralPath $spec -Encoding utf8
$file = Join-Path $sb "docs/planning/plans/20260601T000000-active-arc.md"
"in-flight work" | Out-File -LiteralPath $file -Encoding utf8
Set-Backdate $file 3
Push-Location $sb; & git commit -q --allow-empty -m "unrelated" 2>$null; Pop-Location
$out = Run-Oracle $sb
Assert-Json $out '@($obj.stale).Count' "0" "T4: no stale entries"
Assert-Json $out '@($obj.fresh).Count' "1" "T4: one fresh entry"
Assert-Json $out '$obj.briefing' "" "T4: briefing empty"
Remove-Sandbox $sb

# T5: mixed
$sb = New-Sandbox
$spec = Join-Path $sb "docs/specs/SPECIFICATION.md"
@"
#### MIX-01: Done [DONE]
#### MIX-02: Pending [PLANNED]
"@ | Out-File -LiteralPath $spec -Encoding utf8
$staleFile = Join-Path $sb "docs/planning/intakes/20260101T000000-stale-doc.md"
"Refs MIX-01." | Out-File -LiteralPath $staleFile -Encoding utf8
Set-Backdate $staleFile 60
$freshFile = Join-Path $sb "docs/planning/intakes/20260601T000000-fresh-doc.md"
"active" | Out-File -LiteralPath $freshFile -Encoding utf8
Set-Backdate $freshFile 2
$unknownFile = Join-Path $sb "docs/planning/plans/20260101T000000-unknown-doc.md"
"stale-no-signal" | Out-File -LiteralPath $unknownFile -Encoding utf8
Set-Backdate $unknownFile 60
Push-Location $sb; & git commit -q --allow-empty -m "unrelated" 2>$null; Pop-Location
$out = Run-Oracle $sb
Assert-Json $out '@($obj.stale).Count' "1" "T5: one stale"
Assert-Json $out '@($obj.fresh).Count' "1" "T5: one fresh"
Assert-Json $out '@($obj.unknown).Count' "1" "T5: one unknown"
Assert-Json $out '$obj.briefing.Contains("stale")' "True" "T5: briefing mentions stale"
Remove-Sandbox $sb

# T6: no-planning-dir
$sb = New-Sandbox
Remove-Item -LiteralPath (Join-Path $sb "docs/planning") -Recurse -Force
$out = Run-Oracle $sb
Assert-Json $out '@($obj.stale).Count' "0" "T6: no stale"
Assert-Json $out '@($obj.fresh).Count' "0" "T6: no fresh"
Assert-Json $out '@($obj.unknown).Count' "0" "T6: no unknown"
Assert-Json $out '$obj.briefing' "" "T6: briefing empty"
Remove-Sandbox $sb

# T7: malformed (zero-byte, old mtime → unknown)
$sb = New-Sandbox
$file = Join-Path $sb "docs/planning/intakes/20260101T000000-empty.md"
"" | Out-File -LiteralPath $file -Encoding utf8 -NoNewline
Set-Backdate $file 60
Push-Location $sb; & git commit -q --allow-empty -m "unrelated" 2>$null; Pop-Location
$out = Run-Oracle $sb
Assert-Json $out '@($obj.stale).Count' "0" "T7: malformed not stale"
Assert-Json $out '@($obj.unknown).Count' "1" "T7: malformed -> unknown"
Remove-Sandbox $sb

Write-Host "----"
Write-Host "Total: PASS=$Pass  FAIL=$Fail"
if ($Fail -gt 0) { exit 1 }
exit 0
