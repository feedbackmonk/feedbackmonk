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

# PLANKIND-01 (DEC-327) -- twin of the sh leg. docs/planning/plans/ holds two
# artifact classes (ldis-plan program plans and ltads-start analyses) under one
# "newest wins" rule; a start-analysis stub shadowed a program plan, and
# finalize phase 0.6 resolves its active plan from `latest_plan`, so the
# testability gate armed against a stub with no findings table and silently did
# not fire. `latest_plan` now names the PLAN OF RECORD; POSITION still keys on
# the newest artifact of either class so nothing regresses to POST-SPEC.
function Test-StartAnalysis {
    param([string]$Path)
    if ([System.IO.Path]::GetFileName($Path) -like "*-start-analysis.md") { return $true }
    $head = Get-Content -LiteralPath $Path -TotalCount 10 -ErrorAction SilentlyContinue
    foreach ($line in @($head)) {
        if ($line -match '^kind:\s*start-analysis\s*$') { return $true }
    }
    return $false
}

function Get-PlansSortedDesc {
    param([string]$Dir)
    if (-not (Test-Path $Dir)) { return @() }
    return @(Get-ChildItem -Path $Dir -File -ErrorAction SilentlyContinue |
             Where-Object { $_.Name -ne "README.md" -and -not $_.Name.StartsWith(".") } |
             Sort-Object Name -Descending)
}

$LatestPlan = $null
$LatestStartAnalysis = $null
$NewestPlanningArtifact = Get-NewestInDir "docs/planning/plans"
foreach ($item in (Get-PlansSortedDesc "docs/planning/plans")) {
    $isAnalysis = Test-StartAnalysis $item.FullName
    if ($isAnalysis) {
        if (-not $LatestStartAnalysis) { $LatestStartAnalysis = $item.FullName }
    } else {
        if (-not $LatestPlan) { $LatestPlan = $item.FullName }
    }
}
if (-not $LatestPlan -and (Test-Path "docs/planning/execution-plan.md")) {
    $LatestPlan = (Resolve-Path "docs/planning/execution-plan.md").Path
}
if (-not $NewestPlanningArtifact) { $NewestPlanningArtifact = $LatestPlan }

$SpecExists = Test-Path "docs/specs/SPECIFICATION.md"

$IdeateExists = $false
if (Test-Path "docs/planning/ideations") {
    $ideationItems = Get-ChildItem -Path "docs/planning/ideations" -File -ErrorAction SilentlyContinue |
                     Where-Object { $_.Name -ne "README.md" }
    if ($ideationItems) { $IdeateExists = $true }
}

# LTADS state (ARC-03 / DEC-199: topmost arc of ltads/arc-state.json; native
# JSON read -- graceful empty on absent/malformed. Legacy prose-only projects
# report no active LTADS here; ltads-state's `legacy` verdict covers them.)
$LtadsActive = $false
$LtadsStatus = ""
$arcStatePath = "ltads/arc-state.json"
if (Test-Path $arcStatePath) {
    try {
        $arcDoc = Get-Content $arcStatePath -Raw -Encoding UTF8 -ErrorAction SilentlyContinue | ConvertFrom-Json
        $arcs = @($arcDoc.arcs)
        if ($arcs.Count -gt 0 -and $arcs[0].status) {
            $LtadsStatus = [string]$arcs[0].status
            if ($LtadsStatus -eq "ACTIVE") { $LtadsActive = $true }   # PAUSED = parked, not active (prose-era parity)
        }
    } catch {}
}

# Derive position
$Position = "NONE"
$NextCmd = $null
$Hint = ""

function Get-PathNormalized { param($p) if ($p) { return [string]$p } else { return "" } }

$intakeForCompare = Get-PathNormalized $LatestIntake
$planForCompare = Get-PathNormalized $NewestPlanningArtifact

if ($LtadsActive) {
    $Position = "IN-EXECUTION"
    $NextCmd = "Continue current work (or /0-uldf-finalize when implementation complete)"
    $Hint = "LTADS session is active. /0-uldf-proceed (IN-EXECUTION) will route to worker finalization or /0-uldf-finalize."
} elseif ($LtadsStatus -in @("COMPLETED", "STOPPED", "FINALIZED")) {
    $Position = "POST-IMPLEMENTATION"
    $NextCmd = "/0-uldf-finalize"
    $Hint = "Prior LTADS session is finalized. /0-uldf-proceed routes to /0-uldf-finalize (if not already run) or the next phase."
} elseif ($NewestPlanningArtifact -and (-not $LatestIntake -or $planForCompare -gt $intakeForCompare)) {
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
    latest_start_analysis = Relativize $LatestStartAnalysis
    spec_exists = $SpecExists
    ltads_active = $LtadsActive
    ltads_session_status = if ($LtadsStatus) { $LtadsStatus } else { $null }
    suggested_next_command = $NextCmd
    proceed_hint = $Hint
}

$result | ConvertTo-Json -Compress -Depth 3
