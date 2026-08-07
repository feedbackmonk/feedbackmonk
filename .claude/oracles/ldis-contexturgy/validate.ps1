# ldis-contexturgy oracle self-test (Windows) -- focused PowerShell mirror of
# validate.sh (whose PS-parity leg drives run.ps1 from bash; this file proves
# run.ps1 standalone on Windows). Cases: NO-DATA, gap -> warn (+ never-fail),
# crystallized -> pass, other-project ignored.
# NOTE: ASCII-only string literals (template convention).

$ErrorActionPreference = 'SilentlyContinue'

$thisDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$runPs = Join-Path $thisDir 'run.ps1'

$sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("ldis-ctxy-validate-" + [System.Guid]::NewGuid().ToString('N').Substring(0, 8))
$proj = Join-Path $sandbox 'projx'
$ledger = Join-Path $sandbox 'ledger'
New-Item -ItemType Directory -Path (Join-Path $proj 'docs/planning/intakes') -Force | Out-Null
New-Item -ItemType Directory -Path $ledger -Force | Out-Null

$script:pass = 0; $script:fail = 0
function Ok([string]$label)  { Write-Output "PASS: $label"; $script:pass++ }
function Bad([string]$label) { Write-Output "FAIL: $label"; $script:fail++ }

$atRecent = (Get-Date).AddHours(-1).ToString('yyyy-MM-ddTHH:mm:ss')

function Write-Ledger([string]$At, [string]$Project, [string]$Cmd) {
    Set-Content -Path (Join-Path $ledger 'test.jsonl') -Value ('{"at":"' + $At + '","cmd":"' + $Cmd + '","project":"' + $Project + '","type":"skill","machine":"T","session":"s"}') -Encoding Ascii
}

function Invoke-Oracle([string]$LedgerDir) {
    $env:CLAUDE_USAGE_LEDGER_DIR = $LedgerDir
    Push-Location $proj
    try { return ((& powershell -NoProfile -ExecutionPolicy Bypass -File $runPs) -join "`n") }
    finally { Pop-Location; Remove-Item Env:\CLAUDE_USAGE_LEDGER_DIR -ErrorAction SilentlyContinue }
}

try {
    # 1. NO-DATA (empty ledger dir)
    $out = Invoke-Oracle (Join-Path $sandbox '__none__')
    if ($out -match '"status":"pass"' -and $out -match 'NO-DATA') { Ok 'no ledger -> pass + NO-DATA' } else { Bad "no-ledger (got: $out)" }

    # 2. gap -> warn (only a stale artifact present)
    $old = Join-Path $proj 'docs/planning/intakes/old.md'
    Set-Content -Path $old -Value 'x' -Encoding Ascii
    (Get-Item $old).LastWriteTime = (Get-Date).AddDays(-2)
    Write-Ledger $atRecent 'projx' '0-uldf-ldis-intake'
    $out = Invoke-Oracle $ledger
    if ($out -match '"status":"warn"' -and $out -match '"skill":"0-uldf-ldis-intake"' -and $out -match 'Ephemeral') { Ok 'gap -> warn + briefing' } else { Bad "gap (got: $out)" }
    if ($out -notmatch '"status":"fail"') { Ok 'never-fail invariant' } else { Bad 'NEVER-FAIL invariant broken' }

    # 3. crystallized -> pass
    Set-Content -Path (Join-Path $proj 'docs/planning/intakes/fresh.md') -Value 'x' -Encoding Ascii
    $out = Invoke-Oracle $ledger
    if ($out -match '"status":"pass"' -and $out -match '"checked":1') { Ok 'crystallized -> pass' } else { Bad "crystallized (got: $out)" }

    # 4. other-project record ignored
    Write-Ledger $atRecent 'someother' '0-uldf-ldis-intake'
    $out = Invoke-Oracle $ledger
    if ($out -match '"checked":0') { Ok 'other-project ignored' } else { Bad "other-project (got: $out)" }
} finally {
    Remove-Item -Recurse -Force $sandbox -ErrorAction SilentlyContinue
}

Write-Output ''
Write-Output ("ldis-contexturgy validate (ps): " + $script:pass + " pass / " + $script:fail + " fail")
if ($script:fail -eq 0) { exit 0 } else { exit 1 }
