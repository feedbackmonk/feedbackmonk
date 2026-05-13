# workflow-position oracle (Windows PowerShell)
# Answers: where is this project in the LDIS/LTADS workflow, and what is the next /0-uldf-proceed step?

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

function Get-NewestInDir {
    param([string]$Dir)
    if (-not (Test-Path $Dir)) { return $null }
    $items = Get-ChildItem -Path $Dir -File -ErrorAction SilentlyContinue |
             Where-Object { $_.Name -ne "README.md" -and -not $_.Name.StartsWith(".") } |
             Sort-Object Name
    if ($items) { return $items[-1].FullName }
    return $null
}

# Artifact resolution
$LatestIntake = Get-NewestInDir "docs/planning/intakes"
if (-not $LatestIntake -and (Test-Path "docs/planning/intake-assessment.md")) {
    $LatestIntake = (Resolve-Path "docs/planning/intake-assessment.md").Path
}

$LatestPlan = Get-NewestInDir "docs/planning/plans"
if (-not $LatestPlan -and (Test-Path "docs/planning/execution-plan.md")) {
    $LatestPlan = (Resolve-Path "docs/planning/execution-plan.md").Path
}

$SpecExists = Test-Path "docs/specs/SPECIFICATION.md"

$IdeateExists = $false
if (Test-Path "docs/planning/ideations") {
    $ideationItems = Get-ChildItem -Path "docs/planning/ideations" -File -ErrorAction SilentlyContinue |
                     Where-Object { $_.Name -ne "README.md" }
    if ($ideationItems) { $IdeateExists = $true }
}

# LTADS state
$LtadsActive = $false
$LtadsStatus = ""
$currentSessionPath = "ltads/sessions/current-session.md"
if (Test-Path $currentSessionPath) {
    try {
        $sessionContent = Get-Content $currentSessionPath -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
        if ($sessionContent -match '(?m)^\s*(?:##\s+|-\s+\*\*|\*\*)Status(?:\*\*)?\s*:\s*([A-Z_]+)') {
            $LtadsStatus = $Matches[1].ToUpper()
            if ($LtadsStatus -in @("ACTIVE", "IN_PROGRESS", "STARTED")) { $LtadsActive = $true }
        }
    } catch {}
}

# Derive position
$Position = "NONE"
$NextCmd = $null
$Hint = ""

function Get-PathNormalized { param($p) if ($p) { return [string]$p } else { return "" } }

$intakeForCompare = Get-PathNormalized $LatestIntake
$planForCompare = Get-PathNormalized $LatestPlan

if ($LtadsActive) {
    $Position = "IN-EXECUTION"
    $NextCmd = "Continue current work (or /0-uldf-finalize when implementation complete)"
    $Hint = "LTADS session is active. /0-uldf-proceed (IN-EXECUTION) will route to worker finalization or /0-uldf-finalize."
} elseif ($LtadsStatus -in @("COMPLETED", "STOPPED", "FINALIZED")) {
    $Position = "POST-IMPLEMENTATION"
    $NextCmd = "/0-uldf-finalize"
    $Hint = "Prior LTADS session is finalized. /0-uldf-proceed routes to /0-uldf-finalize (if not already run) or the next phase."
} elseif ($LatestPlan -and (-not $LatestIntake -or $planForCompare -gt $intakeForCompare)) {
    $Position = "POST-PLAN"
    $NextCmd = "/0-uldf-pods-parallelize or /0-uldf-ltads-start (per plan)"
    $Hint = "Plan is newest artifact. /0-uldf-proceed will read the plan's execution strategy and route to PODS or LTADS."
} elseif ($SpecExists) {
    $newerSpecThanPlan = $true
    if ($LatestPlan) {
        $specTime = (Get-Item "docs/specs/SPECIFICATION.md").LastWriteTime
        $planTime = (Get-Item $LatestPlan).LastWriteTime
        $newerSpecThanPlan = $specTime -gt $planTime
    }
    if ($newerSpecThanPlan) {
        $Position = "POST-SPEC"
        $NextCmd = "/0-uldf-ldis-plan"
        $Hint = "Spec exists without a newer plan. /0-uldf-proceed routes to /0-uldf-ldis-plan."
    }
}

if ($Position -eq "NONE" -and $LatestIntake) {
    $Position = "POST-INTAKE"
    $NextCmd = "/0-uldf-ldis-plan or /0-uldf-ldis-spec (per intake recommendation)"
    $Hint = "Intake is newest artifact. /0-uldf-proceed will honor the intake's RECOMMENDED NEXT STEPS."
}

if ($Position -eq "NONE" -and $IdeateExists) {
    $Position = "POST-IDEATE"
    $NextCmd = "/0-uldf-ldis-intake"
    $Hint = "Ideation artifacts exist without intake. /0-uldf-proceed routes to /0-uldf-ldis-intake."
}

if ($Position -eq "NONE" -and -not $Hint) {
    $Hint = "No planning artifacts or active LTADS. Start with /0-uldf-ldis-ideate or /0-uldf-ldis-intake."
}

# Emit (use relative paths when possible)
$workDir = (Get-Location).Path
function Relativize { param($p) if ($p) { return ($p -replace [regex]::Escape($workDir + "\"), "" -replace "\\", "/") } else { return $null } }

$result = [ordered]@{
    position = $Position
    latest_intake = Relativize $LatestIntake
    latest_plan = Relativize $LatestPlan
    spec_exists = $SpecExists
    ltads_active = $LtadsActive
    ltads_session_status = if ($LtadsStatus) { $LtadsStatus } else { $null }
    suggested_next_command = $NextCmd
    proceed_hint = $Hint
}

$result | ConvertTo-Json -Compress -Depth 3
