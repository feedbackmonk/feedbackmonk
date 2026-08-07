# doctrine-footprint oracle (Windows) -- DAUD-01 (§ DOCTRINE-AUDIT, DEC-153)
#
# Byte-parallel with run.sh. Deterministic PRESENCE check: for each ULDF doctrine,
# which prescribed instrumentation artifacts does THIS project have installed?
# PRESENCE ONLY -- verdict/not-applicable/routing are the /0-uldf-doctrine-audit
# skill's job (DAUD-02). See run.sh header for the full contract + BASELINE
# HONESTY note (DAUD-07): the expected set is PER-PROJECT-installed instrumentation,
# not global helpers. dependency-drift (MOD-01) IS shipped but as a GLOBAL script
# (~/.claude/scripts/dependency-drift.*), not a per-project artifact -- so it is
# deliberately NOT in this per-project presence set (checking it would false-flag
# every project). See run.sh header for the full rationale.
#   Framework baseline dated: 2026-07-10
#
# JSON is hand-assembled (not ConvertTo-Json) to stay byte-identical with the
# bash oracle. ASCII-only strings (PW-005 lineage: no em-dashes).

$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$oraclesDir    = ".claude/oracles"
$spec          = "docs/specs/SPECIFICATION.md"
$stateDir      = ".claude/session-state"
$finalizeSkill = ".claude/skills/0-uldf-finalize"
# Global fallback for the JIG-05 wire check (the finalize skill is a GLOBAL
# framework artifact, not a per-project install -- see run.sh header). Env
# override keeps the golden fixtures hermetic.
$globalClaude        = if ($env:DOCTRINE_FOOTPRINT_GLOBAL_CLAUDE) { $env:DOCTRINE_FOOTPRINT_GLOBAL_CLAUDE } else { Join-Path $HOME '.claude' }
$globalFinalizeSkill = Join-Path $globalClaude 'skills/0-uldf-finalize'

function Esc([string]$s) {
    if ($null -eq $s) { return "" }
    return ($s -replace '\\','\\' -replace '"','\"')
}

# Returns $true if .claude/oracles/<name>/ is installed (dir with a manifest).
function Oracle-Present([string]$name) {
    $d = Join-Path $oraclesDir $name
    if (-not (Test-Path -LiteralPath $d -PathType Container)) { return $false }
    return ((Test-Path -LiteralPath (Join-Path $d 'oracle.json')) -or (Test-Path -LiteralPath (Join-Path $d 'manifest.json')))
}

function Spec-Section-Present([string]$headingRegex) {
    if (-not (Test-Path -LiteralPath $spec)) { return $false }
    $hit = Select-String -LiteralPath $spec -Pattern ("^## " + $headingRegex) -Quiet -ErrorAction SilentlyContinue
    return [bool]$hit
}

function File-Present([string]$p) { return (Test-Path -LiteralPath $p) }

# Count module READMEs containing a marker (coverage signal; 0 is valid).
function Readme-Marker-Count([string]$markerRegex) {
    $readmes = Get-ChildItem -Path . -Filter README.md -Recurse -File -ErrorAction SilentlyContinue
    if (-not $readmes) { return 0 }
    $n = 0
    foreach ($r in $readmes) {
        if (Select-String -LiteralPath $r.FullName -Pattern $markerRegex -Quiet -ErrorAction SilentlyContinue) { $n++ }
    }
    return $n
}

$script:missingTotal = 0
$doctrineJson = New-Object System.Collections.Generic.List[string]

# Add-Doctrine: present/absent are arrays of tagged tokens.
function Add-Doctrine([string]$slug, [string[]]$present, [string[]]$absent) {
    $pCount = @($present).Count
    $aCount = @($absent).Count
    if ($aCount -eq 0)      { $hint = "integrated" }
    elseif ($pCount -eq 0)  { $hint = "missing" }
    else                    { $hint = "partial" }
    $script:missingTotal += $aCount

    $pj = ($present | ForEach-Object { '"' + (Esc $_) + '"' }) -join ','
    $aj = ($absent  | ForEach-Object { '"' + (Esc $_) + '"' }) -join ','
    $doctrineJson.Add('{"doctrine":"' + $slug + '","present":[' + $pj + '],"absent":[' + $aj + '],"verdict_hint":"' + $hint + '"}')
}

# Add-DoctrineGated: emit a doctrine the framework has AUTHORED but does not yet
# prescribe per-project instrumentation for (adoption arc pilot-gated). Contributes
# ZERO to missingTotal and carries no absent[] artifacts -- counting artifacts the
# framework does not per-project ship would false-flag every project, which DAUD-07
# BASELINE HONESTY forbids. Purpose is DISCOVERABILITY, not adoption pressure.
# Mirror of add_doctrine_gated in run.sh.
function Add-DoctrineGated([string]$slug, [string]$note) {
    $doctrineJson.Add('{"doctrine":"' + $slug + '","present":[],"absent":[],"verdict_hint":"pilot-gated","note":"' + (Esc $note) + '"}')
}

# ---- Tectonurgy -----------------------------------------------------------
$p = New-Object System.Collections.Generic.List[string]
$a = New-Object System.Collections.Generic.List[string]
if (Oracle-Present 'module-size')    { $p.Add('oracle:module-size') }    else { $a.Add('oracle:module-size') }
if (Oracle-Present 'change-coupling'){ $p.Add('oracle:change-coupling') }else { $a.Add('oracle:change-coupling') }
if (Oracle-Present 'code-graph')     { $p.Add('oracle:code-graph') }     else { $a.Add('oracle:code-graph') }
if (Spec-Section-Present 'MODULARITY'){ $p.Add('spec:MODULARITY') }      else { $a.Add('spec:MODULARITY') }
Add-Doctrine 'tectonurgy' $p.ToArray() $a.ToArray()

# ---- Development Jigs -----------------------------------------------------
$p = New-Object System.Collections.Generic.List[string]
$a = New-Object System.Collections.Generic.List[string]
if (Oracle-Present 'jig-friction') { $p.Add('oracle:jig-friction') } else { $a.Add('oracle:jig-friction') }
$catalog = Get-ChildItem -Path . -Filter JIG_CATALOG.md -Recurse -File -Depth 3 -ErrorAction SilentlyContinue | Select-Object -First 1
if ($catalog) { $p.Add('file:JIG_CATALOG.md') } else { $a.Add('file:JIG_CATALOG.md') }
if (File-Present (Join-Path $stateDir 'aria-probe-candidates.jsonl')) { $p.Add('log:aria-probe-candidates.jsonl') } else { $a.Add('log:aria-probe-candidates.jsonl') }
# JIG-05 wire: project-local skill override first, then the global framework skill.
$jigWire = $false
foreach ($fs in @($finalizeSkill, $globalFinalizeSkill)) {
    if ($jigWire) { break }
    if (Test-Path -LiteralPath $fs -PathType Container) {
        $jigWire = [bool](Get-ChildItem -Path $fs -Recurse -File -ErrorAction SilentlyContinue |
            Where-Object { Select-String -LiteralPath $_.FullName -Pattern 'jig[- ]?retrospective|retrospective jig' -Quiet -ErrorAction SilentlyContinue } |
            Select-Object -First 1)
    }
}
if ($jigWire) { $p.Add('wire:finalize-jig-retrospective') } else { $a.Add('wire:finalize-jig-retrospective(JIG-05)') }
Add-Doctrine 'jigs' $p.ToArray() $a.ToArray()

# ---- Block Verifiability Warrant ------------------------------------------
$p = New-Object System.Collections.Generic.List[string]
$a = New-Object System.Collections.Generic.List[string]
if (Oracle-Present 'block-verifiability-warrant') { $p.Add('oracle:block-verifiability-warrant') } else { $a.Add('oracle:block-verifiability-warrant') }
$warrantN = Readme-Marker-Count '^## Verifiability Warrant'
if ($warrantN -gt 0) { $p.Add("coverage:verifiability-warrant-sections(x$warrantN)") } else { $a.Add('coverage:verifiability-warrant-sections') }
Add-Doctrine 'bvw' $p.ToArray() $a.ToArray()

# ---- AOR / ARIA -----------------------------------------------------------
$p = New-Object System.Collections.Generic.List[string]
$a = New-Object System.Collections.Generic.List[string]
if (Oracle-Present 'aria-status')         { $p.Add('oracle:aria-status') }         else { $a.Add('oracle:aria-status') }
if (Oracle-Present 'ui-surface-detector') { $p.Add('oracle:ui-surface-detector') } else { $a.Add('oracle:ui-surface-detector') }
if (File-Present '.claude/aria-manifest.json') { $p.Add('manifest:aria-manifest.json') } else { $a.Add('manifest:aria-manifest.json') }
if (File-Present (Join-Path $stateDir 'aria-probe-candidates.jsonl')) { $p.Add('log:aria-probe-candidates.jsonl') } else { $a.Add('log:aria-probe-candidates.jsonl') }
Add-Doctrine 'aria_aor' $p.ToArray() $a.ToArray()

# ---- In-App Agent (TUTOR) -- pilot-gated, 0 toward missingTotal -----------
# AOR's production-shipped sibling. Doctrine authored + contracts frozen
# (2026-07-16/17); per-project instrumentation gated on the SessionHelm Arc 2
# pilot (registry-anchors-resolve is in packs.pilot-gated, never installed by
# /0-uldf-setup-project). DEFER-021. See run.sh for the full rationale.
Add-DoctrineGated 'in_app_agent' 'In-App Agent / Self-Operating App practice (TUTOR-01..11, FOUNDATIONS/IN_APP_AGENT_DOCTRINE.md): doctrine authored, contracts frozen; per-project instrumentation pilot-gated on the SessionHelm Arc 2 pilot and NOT yet prescribed. Design new UI surfaces on the assumption the agent is coming; do not install artifacts yet.'

# ---- Agent-Completeness (ACOMP) -- pilot-gated, 0 toward missingTotal ------
# The coverage guarantee over an app's capability surface (DEC-253, DEFER-089).
# Second member of the DAUD-09 pilot-gated class. See run.sh for full rationale.
Add-DoctrineGated 'agent_completeness' 'Agent-Completeness property (ACOMP-01..06, FOUNDATIONS/AGENT_COMPLETENESS_DOCTRINE.md): every user-reachable capability is agent-reachable, measured over the semantic command layer; doctrine authored, per-project instrumentation gated on the first consumer audit (Table) and NOT yet prescribed. Route new user-facing capabilities through the command layer from day one; do not install artifacts yet.'

# ---- RSPD -----------------------------------------------------------------
$p = New-Object System.Collections.Generic.List[string]
$a = New-Object System.Collections.Generic.List[string]
if (Oracle-Present 'rspd-elaboration-tree') { $p.Add('oracle:rspd-elaboration-tree') } else { $a.Add('oracle:rspd-elaboration-tree') }
Add-Doctrine 'rspd' $p.ToArray() $a.ToArray()

# ---- Oraculurgy (hand-off note) -------------------------------------------
$p = New-Object System.Collections.Generic.List[string]
$a = New-Object System.Collections.Generic.List[string]
if (Test-Path -LiteralPath $oraclesDir -PathType Container) { $p.Add('dir:.claude/oracles') } else { $a.Add('dir:.claude/oracles') }
if (File-Present (Join-Path $oraclesDir 'INDEX.md')) { $p.Add('file:oracles/INDEX.md') } else { $a.Add('file:oracles/INDEX.md') }
Add-Doctrine 'oraculurgy' $p.ToArray() $a.ToArray()

# ---- Compose --------------------------------------------------------------
$hasClaude = if (Test-Path -LiteralPath '.claude' -PathType Container) { 'true' } else { 'false' }
$noData = 'false'
if ($script:missingTotal -eq 0) {
    $briefing = "doctrine-footprint: all baseline artifacts present"
} else {
    $briefing = "doctrine-footprint: $($script:missingTotal) artifact(s) absent across doctrines (run /0-uldf-doctrine-audit for verdicts + routes)"
}

$doctrinesStr = ($doctrineJson -join ',')
Write-Output ('{"schemaVersion":"1","has_claude_dir":' + $hasClaude + ',"framework_baseline":"2026-07-10","doctrines":[' + $doctrinesStr + '],"missing_total":' + $script:missingTotal + ',"no_data":' + $noData + ',"briefing":"' + (Esc $briefing) + '"}')
exit 0
