# rspd-elaboration-tree oracle self-test (Windows PowerShell) -- RSPD-06 / RSPD-10.
# Mirror of validate.sh. This validate IS the oracle's smoke. ASCII-only literals.
# Covers: populated tree (charter/elaborated/missing/done-via-recursion),
# graceful absence (no delegated nodes), no-spec, schema fields, real-repo absence.
#
# Usage: powershell -NoProfile -ExecutionPolicy Bypass -File validate.ps1 [-KeepSandbox]
# Exit: 0 all pass; 1 at least one assertion failed.

param([switch]$KeepSandbox)

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$oracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$run = Join-Path $oracleDir "run.ps1"
if (-not (Test-Path $run)) { Write-Error "FATAL: missing $run"; exit 1 }

$script:Pass = 0
$script:Fail = 0
function Ok() { $script:Pass++ }
function Bad($m) { $script:Fail++; Write-Host "  FAIL: $m" }
function Assert-Has($desc, $json, $needle) {
    # literal substring (avoid -like: JSON contains '[' which is a wildcard char)
    if ($json.Contains($needle)) { Ok } else { Bad "$desc (missing '$needle')" }
}
function Assert-Not($desc, $json, $needle) {
    if ($json.Contains($needle)) { Bad "$desc (unexpected '$needle')" } else { Ok }
}
function Assert-Node($desc, $json, $id, $elab) {
    if ($json -match "`"id`":`"$id`"[^}]*`"elaboration`":`"$elab`"") { Ok }
    else { Bad "$desc (node $id not classified $elab)" }
}

# Invoke run.ps1 with a given working directory; return stdout string.
function Invoke-Run($workDir) {
    Push-Location $workDir
    try { return (& powershell -NoProfile -ExecutionPolicy Bypass -File $run 2>&1 | Out-String) }
    finally { Pop-Location }
}

$sb = Join-Path ([System.IO.Path]::GetTempPath()) ("rspd-oracle-" + [System.Guid]::NewGuid().ToString("N").Substring(0,8))
New-Item -ItemType Directory -Force -Path $sb | Out-Null

try {
    # --- A. populated tree fixture -------------------------------------------
    New-Item -ItemType Directory -Force -Path "$sb/A/docs/specs/payments" | Out-Null
    New-Item -ItemType Directory -Force -Path "$sb/A/docs/specs/auth/session" | Out-Null
    New-Item -ItemType Directory -Force -Path "$sb/A/docs/specs/billing" | Out-Null
    $rootSpec = @(
        "# Root spec",
        "",
        "#### PAY-DELEG: Payments sub-system [CHARTER]",
        "**Priority**: Must",
        "**Spec-Owner**: child (delegated) - ``payments/SPECIFICATION.md``",
        "**Contract**: ``payments/CONTRACT.md``",
        "",
        "#### AUTH-DELEG: Auth sub-system [ELABORATED]",
        "**Priority**: Must",
        "**Spec-Owner**: child (delegated) - ``auth/SPECIFICATION.md``",
        "",
        "#### BILL-DELEG: Billing sub-system [IN_PROGRESS]",
        "**Priority**: Must",
        "**Spec-Owner**: child (delegated) - ``billing/SPECIFICATION.md``",
        "",
        "#### RPT-DELEG: Reporting sub-system [CHARTER]",
        "**Priority**: Should",
        "**Spec-Owner**: child (delegated) - ``reporting/SPECIFICATION.md``",
        "",
        "#### CORE-01: Core thing [DONE]",
        "**Implementation**: src/core.ts"
    ) -join "`n"
    Set-Content -Path "$sb/A/docs/specs/SPECIFICATION.md" -Value $rootSpec -Encoding UTF8
    Set-Content -Path "$sb/A/docs/specs/payments/SPECIFICATION.md" -Value "# payments" -Encoding UTF8
    Set-Content -Path "$sb/A/docs/specs/billing/SPECIFICATION.md" -Value "# billing" -Encoding UTF8
    $authSpec = @(
        "# auth spec",
        "#### SESS-DELEG: Session sub-system [DONE]",
        "**Spec-Owner**: child (delegated) - ``session/SPECIFICATION.md``"
    ) -join "`n"
    Set-Content -Path "$sb/A/docs/specs/auth/SPECIFICATION.md" -Value $authSpec -Encoding UTF8
    Set-Content -Path "$sb/A/docs/specs/auth/session/SPECIFICATION.md" -Value "# session" -Encoding UTF8
    # reporting store intentionally absent

    $outA = Invoke-Run "$sb/A"
    Assert-Has  "A1 JSON object"                    $outA '{"status":"ok"'
    Assert-Has  "A2 applicable true"                $outA '"applicable":true'
    Assert-Node "A3 PAY-DELEG charter (present)"    $outA "PAY-DELEG"  "charter"
    Assert-Node "A4 AUTH-DELEG elaborated"          $outA "AUTH-DELEG" "elaborated"
    Assert-Node "A5 RPT-DELEG missing (absent)"     $outA "RPT-DELEG"  "missing"
    Assert-Node "A6 SESS-DELEG done (recursion)"    $outA "SESS-DELEG" "done"
    Assert-Node "A6b BILL-DELEG in_progress"        $outA "BILL-DELEG" "in_progress"
    Assert-Has  "A7 summary totals"                 $outA '"summary":{"total":5,"charter":1,"elaborated":1,"in_progress":1,"done":1,"missing":1}'
    Assert-Has  "A8 missing in briefing"            $outA 'MISSING their sub-spec store'
    Assert-Not  "A9 non-delegated CORE-01 excluded" $outA '"id":"CORE-01"'
    Assert-Has  "A10 missing node shape"            $outA '"id":"RPT-DELEG","domain":"reporting","status":"[CHARTER]"'
    Assert-Has  "A11 nested domain derived"         $outA '"domain":"auth/session"'

    # --- B. graceful absence: no delegated nodes -----------------------------
    New-Item -ItemType Directory -Force -Path "$sb/B/docs/specs" | Out-Null
    $bSpec = @(
        "# Root spec",
        "#### CORE-01: Core thing [DONE]",
        "**Implementation**: src/core.ts",
        "#### CORE-02: Other thing [PLANNED]"
    ) -join "`n"
    Set-Content -Path "$sb/B/docs/specs/SPECIFICATION.md" -Value $bSpec -Encoding UTF8
    $outB = Invoke-Run "$sb/B"
    Assert-Has "B1 applicable false" $outB '"applicable":false'
    Assert-Has "B2 empty tree"       $outB '"tree":[]'
    Assert-Has "B3 zero totals"      $outB '"total":0'
    Assert-Has "B4 empty briefing"   $outB '"briefing":""'

    # --- C. no spec at all ---------------------------------------------------
    New-Item -ItemType Directory -Force -Path "$sb/C" | Out-Null
    $outC = Invoke-Run "$sb/C"
    Assert-Has "C1 applicable false (no spec)" $outC '"applicable":false'
    Assert-Has "C2 spec_root null"             $outC '"spec_root":null'

    # --- D. schema fields ----------------------------------------------------
    foreach ($f in @('"status":', '"applicable":', '"spec_root":', '"tree":', '"summary":', '"briefing":')) {
        Assert-Has "D schema field $f" $outA $f
    }
    foreach ($f in @('"total":', '"charter":', '"elaborated":', '"in_progress":', '"done":', '"missing":')) {
        Assert-Has "D summary field $f" $outA $f
    }

    # --- E. real repo: gracefully absent -------------------------------------
    if (Test-Path "docs/specs/SPECIFICATION.md") {
        $outE = (& powershell -NoProfile -ExecutionPolicy Bypass -File $run 2>&1 | Out-String)
        Assert-Has "E1 real repo applicable false" $outE '"applicable":false'
    }
}
finally {
    if (-not $KeepSandbox) { Remove-Item -Recurse -Force $sb -ErrorAction SilentlyContinue }
}

Write-Host ""
Write-Host "================================================================"
Write-Host "RSPD-ELABORATION-TREE ORACLE VALIDATE (PowerShell): $script:Pass passed, $script:Fail failed"
Write-Host "Sandbox: $sb"
Write-Host "================================================================"
if ($script:Fail -gt 0) { exit 1 }
Write-Host "PASS: rspd-elaboration-tree oracle validates"
exit 0
