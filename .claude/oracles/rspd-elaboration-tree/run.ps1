# rspd-elaboration-tree oracle (Windows PowerShell) -- RSPD-06.
# Mirror of run.sh. Derives the decomposition tree's elaboration state from
# RSPD-05 spec node statuses across the root spec + nested delegated sub-spec
# stores. The load-bearing distinction is MISSING: a delegated row whose
# <domain>/SPECIFICATION.md store does NOT exist (deferred-by-design vs lost).
# kind: project-state. Gracefully absent: applicable:false (never an error).
# Output: single-line JSON (see README.md for the FROZEN schema).

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$specsDir = "docs/specs"
$rootSpec = "$specsDir/SPECIFICATION.md"

function Json-Esc($s) {
    if ($null -eq $s) { return "" }
    return ($s -replace '\\', '\\' -replace '"', '\"')
}

function Emit-Absent($root) {
    $r = if ($null -eq $root) { "null" } else { '"' + (Json-Esc $root) + '"' }
    Write-Output ('{"status":"ok","applicable":false,"spec_root":' + $r + ',"tree":[],"summary":{"total":0,"charter":0,"elaborated":0,"in_progress":0,"done":0,"missing":0},"briefing":""}')
}

if (-not (Test-Path $rootSpec)) { Emit-Absent $null; exit 0 }

function Map-Status($s) {
    switch ($s) {
        "CHARTER"     { return "charter" }
        "ELABORATED"  { return "elaborated" }
        "IN_PROGRESS" { return "in_progress" }
        "DONE"        { return "done" }
        default       { return $s.ToLower() }
    }
}

# For each delegated requirement block in $specFile, return rows {id,name,status,ptr}.
function Extract-Delegated($specFile) {
    $rows = @()
    if (-not (Test-Path $specFile)) { return $rows }
    $lines = Get-Content $specFile -Encoding UTF8 -ErrorAction SilentlyContinue
    $curId = ""; $curName = ""; $curStatus = ""; $curHave = $false
    foreach ($ln in $lines) {
        if ($ln -match '^####\s+(.+)$') {
            $h = $Matches[1]
            $h = $h -replace '\*\([^)]*\)\*\s*$', ''
            $curStatus = ""
            if ($h -match '\[([A-Za-z_]+)\]\s*$') {
                $curStatus = $Matches[1]
                $h = $h -replace '\[[A-Za-z_]+\]\s*$', ''
            }
            $h = $h.Trim()
            $ci = $h.IndexOf(":")
            if ($ci -ge 0) { $curId = $h.Substring(0, $ci).Trim(); $curName = $h.Substring($ci + 1).Trim() }
            else { $curId = $h.Trim(); $curName = "" }
            $curHave = $false
            continue
        }
        if ($ln -match '^(##|###)\s') { $curId = ""; continue }
        if (($curId -ne "") -and (-not $curHave) -and ($ln -match 'child\s*\(delegated\)') -and ($ln -match '[A-Za-z0-9_./-]+SPECIFICATION\.md')) {
            $ptr = $Matches[0]
            $rows += [pscustomobject]@{ id = $curId; name = $curName; status = $curStatus; ptr = $ptr }
            $curHave = $true
        }
    }
    return $rows
}

# Worklist recursion over stores.
$queue = New-Object System.Collections.Generic.Queue[object]
$queue.Enqueue([pscustomobject]@{ file = $rootSpec })
$seen = @{}
$nodeJsons = @()
$tot = 0; $ch = 0; $el = 0; $ip = 0; $dn = 0; $ms = 0

while ($queue.Count -gt 0) {
    $item = $queue.Dequeue()
    $spec = $item.file
    if ($seen.ContainsKey($spec)) { continue }
    $seen[$spec] = $true
    if (-not (Test-Path $spec)) { continue }
    $specDir = Split-Path $spec -Parent
    if ([string]::IsNullOrEmpty($specDir)) { $specDir = "." }
    foreach ($r in (Extract-Delegated $spec)) {
        $store = "$specDir/$($r.ptr)" -replace '\\', '/' -replace '/+', '/' -replace '/\./', '/'
        $domain = $store
        if ($domain.StartsWith("$specsDir/")) { $domain = $domain.Substring($specsDir.Length + 1) }
        $domain = $domain -replace '/SPECIFICATION\.md$', ''
        $exists = Test-Path $store
        if ($exists) { $elab = Map-Status $r.status } else { $elab = "missing" }
        $tot++
        switch ($elab) {
            "charter"     { $ch++ }
            "elaborated"  { $el++ }
            "in_progress" { $ip++ }
            "done"        { $dn++ }
            "missing"     { $ms++ }
        }
        $existsLit = if ($exists) { "true" } else { "false" }
        $nodeJsons += ('{"id":"' + (Json-Esc $r.id) + '","domain":"' + (Json-Esc $domain) + '","status":"[' + (Json-Esc $r.status) + ']","store":"' + (Json-Esc $store) + '","store_exists":' + $existsLit + ',"elaboration":"' + (Json-Esc $elab) + '"}')
        if ($exists) { $queue.Enqueue([pscustomobject]@{ file = $store }) }
    }
}

if ($tot -eq 0) { Emit-Absent $rootSpec; exit 0 }

$briefing = ""
if ($ms -gt 0) {
    $briefing = "$ms delegated node(s) MISSING their sub-spec store (broke); $ch charter, $el elaborated, $ip in_progress, $dn done."
}

$treeJson = "[" + ($nodeJsons -join ",") + "]"
$summaryJson = '{"total":' + $tot + ',"charter":' + $ch + ',"elaborated":' + $el + ',"in_progress":' + $ip + ',"done":' + $dn + ',"missing":' + $ms + '}'
$json = '{"status":"ok","applicable":true,"spec_root":"' + (Json-Esc $rootSpec) + '","tree":' + $treeJson + ',"summary":' + $summaryJson + ',"briefing":"' + (Json-Esc $briefing) + '"}'
Write-Output $json
exit 0
