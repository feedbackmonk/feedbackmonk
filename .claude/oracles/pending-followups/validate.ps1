# pending-followups oracle self-test (Windows PowerShell) -- TWIN of validate.sh
#
# Two layers, and the second is the point.
#
# LAYER 1 (pre-existing): run the oracle against the live repo and assert the
# schema fields exist. A SHAPE check. It passed every day from 2026-04 to
# 2026-08-19 while `overdue` was a constant, which is precisely why FOLLOWUP-01
# survived: the validator never asserted a VERDICT.
#
# LAYER 2 (FOLLOWUP-01/02, DEC-385): sandboxed cells that write entries in the
# /0-uldf-schedule documented output format with a past date and assert the
# oracle reports them overdue -- plus CONTROLS, which are half the design. A fix
# that simply made the oracle credulous passes every must-STILL-fire cell and
# fails V5/V6/V7.
#
# Engine fidelity: run.ps1 is invoked with the SAME engine running this file, so
# a powershell.exe-only or pwsh-only divergence cannot hide (TWIN-02).
$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()
$oracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$runPs = Join-Path $oracleDir "run.ps1"

$script:pass = 0
$script:fail = 0
function Ok  ($m) { $script:pass++; Write-Output ("  ok   " + $m) }
function Bad ($m) { $script:fail++; Write-Output ("  FAIL " + $m) }

$engine = (Get-Process -Id $PID).Path
if (-not $engine) { $engine = "powershell" }

# ------------------------------------------------------------------ LAYER 1 --
$output = & $engine -NoProfile -ExecutionPolicy Bypass -File $runPs
try {
    $parsed = $output | ConvertFrom-Json
} catch {
    Write-Error "FAIL: output is not valid JSON"
    exit 1
}

$requiredFields = @("briefing", "has_followups_section", "total", "overdue", "overdue_evaluable", "unevaluable", "items")
foreach ($field in $requiredFields) {
    if (-not ($parsed.PSObject.Properties.Name -contains $field)) {
        Write-Error ("FAIL: missing schema field '" + $field + "'")
        exit 1
    }
}

# ------------------------------------------------------------------ LAYER 2 --
$sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("pf-validate-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $sandbox -Force | Out-Null
# A machine-global source would contaminate every project-scoped assertion.
$fakeHome = Join-Path $sandbox "home"
New-Item -ItemType Directory -Path (Join-Path $fakeHome ".claude") -Force | Out-Null
Set-Content -Path (Join-Path $fakeHome ".claude\MACHINE_CONFIG.md") -Value "" -Encoding UTF8

$origProfile = $env:USERPROFILE
$origCwd = (Get-Location).Path

function New-Proj {
    param([string]$Name, [string]$Body)
    $dir = Join-Path $sandbox $Name
    if (Test-Path $dir) { Remove-Item $dir -Recurse -Force }
    New-Item -ItemType Directory -Path $dir -Force | Out-Null
    $text = "## Pending Follow-Ups`n`n" + $Body + "`n`n## Next`n"
    [System.IO.File]::WriteAllText((Join-Path $dir "CLAUDE.md"), $text, (New-Object System.Text.UTF8Encoding($false)))
    return $dir
}

function Invoke-Oracle {
    param([string]$Name)
    $dir = Join-Path $sandbox $Name
    $env:USERPROFILE = $fakeHome
    Set-Location $dir
    try {
        $raw = & $engine -NoProfile -ExecutionPolicy Bypass -File $runPs 2>$null
        return ($raw | ConvertFrom-Json)
    } finally {
        Set-Location $origCwd
        $env:USERPROFILE = $origProfile
    }
}

$past   = (Get-Date).Date.AddDays(-30).ToString("yyyy-MM-dd", [System.Globalization.CultureInfo]::InvariantCulture)
$future = (Get-Date).Date.AddDays(3650).ToString("yyyy-MM-dd", [System.Globalization.CultureInfo]::InvariantCulture)

# --- V1: the producer --after form. THE cell that would have caught it. ------
New-Proj -Name "v1" -Body ("- **After " + $past + " (Standing telemetry review)**: do the thing.`n`n  Remove this entry once acted on.") | Out-Null
$r = Invoke-Oracle "v1"
if ($r.total -eq 1 -and $r.overdue -eq 1 -and $r.items[0].overdue -eq $true) {
    Ok "V1 schedule --after form reports OVERDUE (total=1, overdue=1)"
} else {
    Bad ("V1 producer --after form not overdue: total=" + $r.total + " overdue=" + $r.overdue + " item=" + $r.items[0].overdue + " -- the FOLLOWUP-01 defect is back")
}
if ($r.items[0].due -eq $past) { Ok "V1 due carries the date, not the whole bold prefix" }
else { Bad ("V1 expected due=" + $past + ", got " + $r.items[0].due) }

# --- V2: the producer --on form (absent from the pre-fix alternation) --------
New-Proj -Name "v2" -Body ("- **On " + $past + " (A dated review)**: do the thing.") | Out-Null
$r = Invoke-Oracle "v2"
if ($r.overdue -eq 1) { Ok "V2 schedule --on form reports OVERDUE" }
else { Bad ("V2 producer --on form not overdue (overdue=" + $r.overdue + ")") }

# --- V3: a future date is a real measurement, not an unknown -----------------
New-Proj -Name "v3" -Body ("- **After " + $future + " (Not yet due)**: later.") | Out-Null
$r = Invoke-Oracle "v3"
if ($r.overdue -eq 0 -and $r.unevaluable -eq 0 -and $r.items[0].overdue -eq $false) {
    Ok "V3 future producer entry: overdue=false as a MEASUREMENT (not null, not overdue)"
} else { Bad ("V3 expected overdue=0/unevaluable=0/item=False, got " + $r.overdue + "/" + $r.unevaluable + "/" + $r.items[0].overdue) }

# --- V4 CONTROL: a trigger entry has no date and is NOT unevaluable ----------
New-Proj -Name "v4" -Body "- **Critic verdict telemetry (trigger: when the first PODS session converges)**: capture it." | Out-Null
$r = Invoke-Oracle "v4"
if ($r.total -eq 1 -and $r.unevaluable -eq 0 -and $r.items[0].overdue -eq $false) {
    Ok "V4 CONTROL trigger-based entry stays a clean label (not an unknown)"
} else { Bad ("V4 CONTROL expected total=1/unevaluable=0/overdue=False, got " + $r.total + "/" + $r.unevaluable + "/" + $r.items[0].overdue + " -- DEC-379: an unknown may only be emitted where a real question existed") }

# --- V5 CONTROL: a date MENTIONED mid-prefix is a label, not a trigger -------
# 59 of the 61 dated-looking prefixes measured machine-wide are this shape.
New-Proj -Name "v5" -Body "- **In-App Agent (TUTOR) -- Phase 3 SHIPPED 2020-01-01 (58da696)**: shipped, kept for provenance.`n- **[ACTIVE, added 2020-02-02]**: an ongoing note." | Out-Null
$r = Invoke-Oracle "v5"
if ($r.total -eq 2 -and $r.overdue -eq 0 -and $r.unevaluable -eq 0) {
    Ok "V5 CONTROL a date mentioned mid-prefix is neither overdue nor unknown"
} else { Bad ("V5 CONTROL expected total=2/overdue=0/unevaluable=0, got " + $r.total + "/" + $r.overdue + "/" + $r.unevaluable + " -- contains-a-date would misjudge 59 live entries") }

# --- V6: DECLARED but unreadable => null, never false -----------------------
New-Proj -Name "v6" -Body "- **After 2026-02-30 (An impossible date)**: this date cannot exist." | Out-Null
$r = Invoke-Oracle "v6"
if ($r.unevaluable -eq 1 -and $r.overdue_evaluable -eq $false -and $null -eq $r.items[0].overdue -and $null -eq $r.items[0].days_overdue) {
    Ok "V6 declared-but-unreadable date => overdue:null + overdue_evaluable:false (CSI-36/DEC-342)"
} else { Bad ("V6 expected unevaluable=1/evaluable=False/item null, got " + $r.unevaluable + "/" + $r.overdue_evaluable + "/[" + $r.items[0].overdue + "]/[" + $r.items[0].days_overdue + "]") }
if ($r.briefing -like "*FLOOR*") { Ok "V6 the briefing SAYS the count is a floor (silence is half the defect -- OVALID-09)" }
else { Bad ("V6 briefing does not disclose the unevaluable entry: " + $r.briefing) }

# --- V7 CONTROL: the legacy bare form must not have narrowed ----------------
New-Proj -Name "v7" -Body "- **After 2020-01-01**: the pre-fix fixture shape.`n- **2020-02-02**: the bare pre-fix fixture shape." | Out-Null
$r = Invoke-Oracle "v7"
if ($r.overdue -eq 2) { Ok "V7 CONTROL legacy bare forms still parse (the fix widened, never narrowed)" }
else { Bad ("V7 CONTROL expected overdue=2, got " + $r.overdue + " -- the fix NARROWED") }

# --- V8: the briefing must survive a quote-naive extractor ------------------
New-Proj -Name "v8" -Body ("- **After " + $past + " (An entry whose title carries a " + [char]34 + "quoted phrase" + [char]34 + " and a comma)**: body.") | Out-Null
$r = Invoke-Oracle "v8"
if ($r.briefing -notmatch [char]34) { Ok "V8 briefing carries no double quote even when an entry title does" }
else { Bad ("V8 briefing contains a double quote -- the consumer extractor would truncate it: " + $r.briefing) }
if ($r.briefing.Length -le 300) { Ok ("V8 briefing is " + $r.briefing.Length + " chars (session-start caps the whole briefing at 3000)") }
else { Bad ("V8 briefing is " + $r.briefing.Length + " chars -- too long for the shared cap") }

# --- V9 CONTROL: no section at all => empty briefing (line suppressed) ------
$dir = Join-Path $sandbox "v9"
New-Item -ItemType Directory -Path $dir -Force | Out-Null
[System.IO.File]::WriteAllText((Join-Path $dir "CLAUDE.md"), "# P`n`nnothing here`n", (New-Object System.Text.UTF8Encoding($false)))
$r = Invoke-Oracle "v9"
if ($r.has_followups_section -eq $false -and [string]::IsNullOrEmpty($r.briefing)) {
    Ok "V9 CONTROL no section => empty briefing (graceful absence)"
} else { Bad ("V9 CONTROL expected has_section=False + empty briefing, got " + $r.has_followups_section + "/[" + $r.briefing + "]") }

# --- V10 CONTROL: an entry due TODAY is not yet overdue ---------------------
# TWIN-01 boundary. run.sh compares midnight epochs with `>`; if run.ps1 keeps
# the wall clock, this same entry reads overdue here and not there.
$todayStr = (Get-Date).Date.ToString("yyyy-MM-dd", [System.Globalization.CultureInfo]::InvariantCulture)
New-Proj -Name "v10" -Body ("- **After " + $todayStr + " (Due today, not yet past)**: today is not overdue.") | Out-Null
$r = Invoke-Oracle "v10"
if ($r.overdue -eq 0 -and $r.unevaluable -eq 0) { Ok "V10 CONTROL an entry dated TODAY is not overdue (midnight boundary)" }
else { Bad ("V10 CONTROL expected overdue=0/unevaluable=0, got " + $r.overdue + "/" + $r.unevaluable) }

# --- V11: days_overdue is exact, and floors ---------------------------------
New-Proj -Name "v11" -Body ("- **After " + $past + " (Thirty days past)**: exactly thirty.") | Out-Null
$r = Invoke-Oracle "v11"
if ($r.items[0].days_overdue -eq 30) { Ok "V11 days_overdue is exactly 30 (floor, not PowerShell bankers rounding)" }
else { Bad ("V11 expected days_overdue=30, got " + $r.items[0].days_overdue + " -- twins disagree on rounding") }

Remove-Item $sandbox -Recurse -Force -ErrorAction SilentlyContinue

Write-Output ""
Write-Output ("PASS=" + $script:pass + " FAIL=" + $script:fail)
if ($script:fail -gt 0) { Write-Output "FAIL: pending-followups oracle self-test"; exit 1 }
Write-Output "PASS: pending-followups oracle validates"
exit 0
