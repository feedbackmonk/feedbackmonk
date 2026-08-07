# change-coupling oracle (Windows) — scrutiny ADD proposal A2 (KEEP), Arc 1.
# PowerShell parity of run.sh: git co-change mining across module boundaries.
# Language-agnostic coupling evidence (needs only `git log`). ADVISORY,
# correlation-not-dependency — never blocks, never exits nonzero on findings.
# On-demand (kind: project-state) — NOT in the every-session briefing fan-out.
# JSON is assembled by hand (mirroring run.sh) for byte-parity and to avoid the
# ConvertTo-Json single-element-array unwrap pitfall.

$ErrorActionPreference = "SilentlyContinue"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

# ---- Defaults (overridable via .claude/config.json -> changeCoupling.*) ------
$DEF_SINCE_DAYS = 180
$DEF_MAX_COMMITS = 1000
$DEF_BULK_MAX = 20
$DEF_MIN_CO = 5
$CAP = 50

$NoCache = $false
foreach ($a in $args) { if ($a -eq "--no-cache") { $NoCache = $true } }

function Esc([string]$s) {
    if ($null -eq $s) { return "" }
    $s = $s -replace '\\', '\\'
    $s = $s -replace '"', '\"'
    return $s
}

# state used by Emit-NoData
$script:SINCE_DAYS = $DEF_SINCE_DAYS
$script:MAX_COMMITS = $DEF_MAX_COMMITS
$script:BULK_MAX = $DEF_BULK_MAX
$script:MIN_CO = $DEF_MIN_CO
$script:SHALLOW = $false
$script:COMMITS_ANALYZED = 0
$script:EXCLUDED_BULK = 0

function Emit-NoData([string]$reason) {
    $r = Esc $reason
    $out = '{"status":"no-data","reason":"' + $r + '","shallow":' + ($script:SHALLOW.ToString().ToLower()) +
        ',"window":{"sinceDays":' + $script:SINCE_DAYS + ',"maxCommits":' + $script:MAX_COMMITS +
        ',"commitsAnalyzed":' + $script:COMMITS_ANALYZED + ',"qualifyingCommits":0},"filters":{"bulkCommitMax":' +
        $script:BULK_MAX + ',"minCoChanges":' + $script:MIN_CO + ',"excludedCommits":' + $script:EXCLUDED_BULK +
        '},"filePairs":[],"modulePairs":[],"crossBoundaryTop":[],"truncated":false,"cached":false,"briefing":"change-coupling: NO-DATA (' +
        $r + ')"}'
    Write-Output $out
    exit 0
}

# ---- git presence + repo root ------------------------------------------------
$null = & git --version 2>$null
if ($LASTEXITCODE -ne 0) { Emit-NoData "git not available" }

$RepoRoot = (& git rev-parse --show-toplevel 2>$null)
if (-not $RepoRoot) { Emit-NoData "not a git repository" }
$RepoRoot = "$RepoRoot".Trim()

# ---- Config load (optional) --------------------------------------------------
$Config = Join-Path $RepoRoot ".claude/config.json"
if (Test-Path $Config) {
    try {
        $cfg = (Get-Content -Raw -Encoding UTF8 $Config | ConvertFrom-Json).changeCoupling
        if ($cfg) {
            if ($cfg.sinceDays -match '^\d+$' -or $cfg.sinceDays -is [int]) { $script:SINCE_DAYS = [int]$cfg.sinceDays }
            if ($cfg.maxCommits -match '^\d+$' -or $cfg.maxCommits -is [int]) { $script:MAX_COMMITS = [int]$cfg.maxCommits }
            if ($cfg.bulkCommitMax -match '^\d+$' -or $cfg.bulkCommitMax -is [int]) { $script:BULK_MAX = [int]$cfg.bulkCommitMax }
            if ($cfg.minCoChanges -match '^\d+$' -or $cfg.minCoChanges -is [int]) { $script:MIN_CO = [int]$cfg.minCoChanges }
        }
    } catch { }
}
$SINCE_DAYS = $script:SINCE_DAYS; $MAX_COMMITS = $script:MAX_COMMITS
$BULK_MAX = $script:BULK_MAX; $MIN_CO = $script:MIN_CO

# ---- HEAD + shallow ----------------------------------------------------------
$HeadSha = (& git -C $RepoRoot rev-parse HEAD 2>$null)
if (-not $HeadSha) { Emit-NoData "no commit history" }
$HeadSha = "$HeadSha".Trim()
if ((& git -C $RepoRoot rev-parse --is-shallow-repository 2>$null) -eq "true") { $script:SHALLOW = $true }
$SHALLOW = $script:SHALLOW

# ---- Cache -------------------------------------------------------------------
$CacheDir = Join-Path $RepoRoot ".claude/session-state"
$CacheFile = Join-Path $CacheDir "change-coupling-cache.json"
$CacheKey = "$HeadSha|sd=$SINCE_DAYS;mc=$MAX_COMMITS;bm=$BULK_MAX;co=$MIN_CO"
if (-not $NoCache -and (Test-Path $CacheFile)) {
    try {
        $cached = Get-Content -Raw -Encoding UTF8 $CacheFile | ConvertFrom-Json
        if ($cached.key -eq $CacheKey -and $cached.result) {
            # result stored as a raw JSON string (unified with run.sh) — flip
            # cached:false -> true so shells share one interchangeable cache.
            $rj = [string]$cached.result
            $rj = $rj -replace '"cached":false', '"cached":true'
            Write-Output $rj
            exit 0
        }
    } catch { }
}

# ---- Mine history ------------------------------------------------------------
$raw = & git -C $RepoRoot -c core.quotepath=off log --name-only --no-merges `
    --since="$SINCE_DAYS days ago" --max-count=$MAX_COMMITS --pretty=format:'@@C@@%H' 2>$null
if (-not $raw) { $script:COMMITS_ANALYZED = 0; Emit-NoData "no commits in the $SINCE_DAYS-day window" }
$lines = @($raw -split "`n")

# Group into commits; collect qualifying (2..BULK_MAX) file lists.
$analyzed = 0; $excluded = 0; $qualifying = 0
$qualCommits = New-Object System.Collections.Generic.List[object]
$curFiles = New-Object System.Collections.Generic.List[string]
$markerSeen = $false

function Flush-Commit {
    if (-not $script:markerSeen) { return }
    $script:analyzed++
    $n = $script:curFiles.Count
    if ($n -gt $BULK_MAX) { $script:excluded++ }
    elseif ($n -ge 2) {
        $script:qualifying++
        $script:qualCommits.Add(@($script:curFiles.ToArray()))
    }
    $script:curFiles = New-Object System.Collections.Generic.List[string]
}
$script:analyzed = 0; $script:excluded = 0; $script:qualifying = 0
$script:markerSeen = $false
$script:curFiles = $curFiles
$script:qualCommits = $qualCommits

foreach ($ln in $lines) {
    $l = ($ln -replace "`r$", "")
    if ($l.StartsWith("@@C@@")) { Flush-Commit; $script:markerSeen = $true; continue }
    if ($l -eq "") { continue }
    $script:curFiles.Add($l)
}
Flush-Commit

$analyzed = $script:analyzed; $excluded = $script:excluded; $qualifying = $script:qualifying
$script:COMMITS_ANALYZED = $analyzed
$script:EXCLUDED_BULK = $excluded

if ($analyzed -lt 2) { Emit-NoData "insufficient history ($analyzed commit(s) in window)" }
if ($qualifying -eq 0) { Emit-NoData "no co-change-analyzable commits (all single-file or bulk-filtered)" }

# ---- Module map (nearest-ancestor README; memoised per directory) ------------
# ORDINAL (case-sensitive) dictionaries below: PowerShell's default @{} hashtable
# is case-INSENSITIVE, which would merge case-distinct git paths (e.g. LTADS/
# design docs vs ltads/ runtime state) — git is case-sensitive, so those are
# different modules. bash awk arrays are case-sensitive; ordinal keeps parity.
$modMemo = New-Object 'System.Collections.Generic.Dictionary[string,string]' ([System.StringComparer]::Ordinal)
function Module-For([string]$f) {
    if ($f -like "*/*") { $dir = $f.Substring(0, $f.LastIndexOf('/')) } else { $dir = "." }
    if ($modMemo.ContainsKey($dir)) { return $modMemo[$dir] }
    $mod = ""
    $d = $dir
    while ($d -and $d -ne "." -and $d -ne "/") {
        $readme = Join-Path $RepoRoot (Join-Path $d "README.md")
        if (Test-Path -LiteralPath $readme) { $mod = $d; break }
        if ($d -like "*/*") { $d = $d.Substring(0, $d.LastIndexOf('/')) } else { $d = "." }
    }
    if (-not $mod) {
        if ($dir -eq ".") { $mod = "(root)" }
        else { $mod = $dir.Split('/')[0] }
        if (-not $mod) { $mod = "(root)" }
    }
    $modMemo[$dir] = $mod
    return $mod
}

# ---- Pair counting (file + module) -------------------------------------------
$fpc = New-Object 'System.Collections.Generic.Dictionary[string,int]' ([System.StringComparer]::Ordinal)  # "a`tb" -> count
$mpc = New-Object 'System.Collections.Generic.Dictionary[string,int]' ([System.StringComparer]::Ordinal)  # "modA`tmodB" -> count
foreach ($files in $qualCommits) {
    $arr = @($files)
    $m = $arr.Count
    # precompute modules for this commit's files
    $mods = New-Object 'string[]' $m
    for ($i = 0; $i -lt $m; $i++) { $mods[$i] = Module-For $arr[$i] }
    for ($i = 0; $i -lt $m - 1; $i++) {
        for ($j = $i + 1; $j -lt $m; $j++) {
            $a = $arr[$i]; $b = $arr[$j]
            if ($a -eq $b) { continue }
            if ([string]::CompareOrdinal($a, $b) -le 0) { $fk = "$a`t$b" } else { $fk = "$b`t$a" }
            if ($fpc.ContainsKey($fk)) { $fpc[$fk]++ } else { $fpc[$fk] = 1 }
            $ma = $mods[$i]; $mb = $mods[$j]
            if (-not $ma) { $ma = "(unknown)" }
            if (-not $mb) { $mb = "(unknown)" }
            if ([string]::CompareOrdinal($ma, $mb) -le 0) { $mk = "$ma`t$mb" } else { $mk = "$mb`t$ma" }
            if ($mpc.ContainsKey($mk)) { $mpc[$mk]++ } else { $mpc[$mk] = 1 }
        }
    }
}

# ---- Filter (>= MIN_CO), sort desc, cap ----------------------------------------
$fpList = foreach ($k in $fpc.Keys) {
    if ($fpc[$k] -ge $MIN_CO) { $p = $k -split "`t", 2; [PSCustomObject]@{ a = $p[0]; b = $p[1]; n = $fpc[$k] } }
}
$fpList = @($fpList | Sort-Object -Property n -Descending)

$mpList = foreach ($k in $mpc.Keys) {
    if ($mpc[$k] -ge $MIN_CO) {
        $p = $k -split "`t", 2
        $cross = ($p[0] -ne $p[1])
        [PSCustomObject]@{ moduleA = $p[0]; moduleB = $p[1]; n = $mpc[$k]; cross = $cross }
    }
}
$mpList = @($mpList | Sort-Object -Property n -Descending)
$cbList = @($mpList | Where-Object { $_.cross })

$fpTotal = $fpList.Count; $mpTotal = $mpList.Count; $cbTotal = $cbList.Count
$truncated = ($fpTotal -gt $CAP) -or ($mpTotal -gt $CAP) -or ($cbTotal -gt $CAP)

$fpTop = @($fpList | Select-Object -First $CAP)
$mpTop = @($mpList | Select-Object -First $CAP)
$cbTop = @($cbList | Select-Object -First $CAP)

# ---- Assemble JSON arrays by hand (parity with run.sh) -----------------------
function Bool([bool]$b) { if ($b) { "true" } else { "false" } }

$fpJson = "["
$first = $true
foreach ($e in $fpTop) {
    if (-not $first) { $fpJson += "," } else { $first = $false }
    $fpJson += '{"a":"' + (Esc $e.a) + '","b":"' + (Esc $e.b) + '","coChanges":' + $e.n + '}'
}
$fpJson += "]"

$mpJson = "["
$first = $true
foreach ($e in $mpTop) {
    if (-not $first) { $mpJson += "," } else { $first = $false }
    $mpJson += '{"moduleA":"' + (Esc $e.moduleA) + '","moduleB":"' + (Esc $e.moduleB) + '","coChanges":' + $e.n + ',"crossBoundary":' + (Bool $e.cross) + '}'
}
$mpJson += "]"

$cbJson = "["
$first = $true
foreach ($e in $cbTop) {
    if (-not $first) { $cbJson += "," } else { $first = $false }
    $cbJson += '{"moduleA":"' + (Esc $e.moduleA) + '","moduleB":"' + (Esc $e.moduleB) + '","coChanges":' + $e.n + ',"crossBoundary":true}'
}
$cbJson += "]"

# ---- Briefing ----------------------------------------------------------------
if ($cbTotal -gt 0) {
    $top = $cbTop[0]
    $briefing = Esc ("change-coupling: $cbTotal cross-boundary pair(s); top " + $top.moduleA + " <-> " + $top.moduleB + " (" + $top.n + "x). Advisory co-change evidence, not proven dependency.")
} else {
    $briefing = Esc ("change-coupling: no cross-boundary pair reached $MIN_CO co-changes over $analyzed analyzed commits (checked, clean).")
}

$result = '{"status":"ok","shallow":' + (Bool $SHALLOW) + ',"window":{"sinceDays":' + $SINCE_DAYS +
    ',"maxCommits":' + $MAX_COMMITS + ',"commitsAnalyzed":' + $analyzed + ',"qualifyingCommits":' + $qualifying +
    '},"filters":{"bulkCommitMax":' + $BULK_MAX + ',"minCoChanges":' + $MIN_CO + ',"excludedCommits":' + $excluded +
    '},"filePairs":' + $fpJson + ',"modulePairs":' + $mpJson + ',"crossBoundaryTop":' + $cbJson +
    ',"truncated":' + (Bool $truncated) + ',"cached":false,"briefing":"' + $briefing + '"}'

# ---- Write cache (best-effort) -----------------------------------------------
try {
    if (-not (Test-Path $CacheDir)) { New-Item -ItemType Directory -Force -Path $CacheDir | Out-Null }
    $cacheObj = [ordered]@{ key = $CacheKey; result = $result }
    ($cacheObj | ConvertTo-Json -Depth 4 -Compress) | Set-Content -Encoding UTF8 -NoNewline $CacheFile
} catch { }

Write-Output $result
exit 0
