# module-size oracle (Windows PowerShell)
#
# Verification Oracle (kind: verification). PowerShell twin of run.sh -- see
# that file's header for the full rationale (EPP + Oraculurgy Part 11 mandate;
# scrutiny P0-7; M2 soft band). Byte-equivalent JSON: both shells parse the
# SAME `git ls-tree -r -l HEAD` output on a git tree, and the JSON is assembled
# by hand (NOT ConvertTo-Json) so key order / spacing match run.sh exactly.
#
# ADVISORY: over-band modules -> status "warn", ALWAYS exit 0 on a real run.
# NO-DATA: unreadable root / no analyzable files -> status "no-data", never a
# silent pass. Token estimate = sum(direct-file bytes)/4 (one divisor). git
# ls-tree fast path (tracked-only => respects .gitignore); filesystem-walk
# fallback for non-git trees.

param([string]$Root = "")

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()

$DefaultBand = 4000
$Cap = 50

# JSON string escape (backslash + double-quote), matching run.sh esc().
function Esc([string]$s) {
    if ($null -eq $s) { return "" }
    return $s.Replace('\', '\\').Replace('"', '\"')
}

function Emit-NoData([string]$reason) {
    $r = Esc $reason
    $out = '{"status":"no-data","modules_scanned":0,"over_band":[],"over_band_total":0,' +
           '"no_readme":[],"no_readme_total":0,"no_data":[{"path":".","reason":"' + $r + '"}],' +
           '"no_data_total":1,"band":{"softTokens":' + $DefaultBand + ',"source":"default"},' +
           '"enum_mode":"none","scan_duration_ms":0,' +
           '"briefing":"module-size: NO-DATA (' + $r + ') -- could not analyze; not a silent pass."}'
    [Console]::Out.WriteLine($out)
    exit 0
}

# ---- resolve root -----------------------------------------------------------
if ($Root -ne "") {
    $RootPath = $Root
} else {
    $RootPath = (Resolve-Path (Join-Path $PSScriptRoot "..\..\..") -ErrorAction SilentlyContinue).Path
}
if ([string]::IsNullOrEmpty($RootPath) -or -not (Test-Path -LiteralPath $RootPath -PathType Container)) {
    Emit-NoData "project root not readable"
}
$RootPath = (Resolve-Path -LiteralPath $RootPath).Path

$started = Get-Date

# ---- band from config -------------------------------------------------------
$Band = $DefaultBand
$BandSource = "default"
$cfg = Join-Path $RootPath ".claude/config.json"
if (Test-Path -LiteralPath $cfg) {
    try {
        $j = Get-Content -LiteralPath $cfg -Raw -Encoding UTF8 | ConvertFrom-Json
        $v = $j.moduleSize.softBandTokens
        if ("$v" -match '^\d+$') {
            $Band = [int]$v; $BandSource = "config"
        }
    } catch { }
}

# ---- exclusion (directory-level, matching run.sh builder) -------------------
$ExcludeRe = '(^|/)(\.git|node_modules|dist|build|coverage|__pycache__|\.next|\.expo|target|\.cargo|vendor)(/|$)'
$ExcludeRe2 = '(^|/)\.claude/(collaboration|session-state)(/|$)'
function Is-Excluded([string]$d) {
    return ($d -match $ExcludeRe) -or ($d -match $ExcludeRe2)
}

$CodeRe = '\.(sh|ps1|ts|tsx|js|jsx|mjs|cjs|py|go|rs|java|cs|cpp|cc|c|h|hpp|rb|php|swift|kt|scala|lua|pl|r)$'

# ---- enumerate: emit @{Size;Path} (repo-relative, forward-slash) ------------
$EnumMode = ""
$noData = New-Object System.Collections.Generic.List[string]
$files = New-Object System.Collections.Generic.List[object]

$inGit = $false
try {
    $null = & git -C $RootPath rev-parse --is-inside-work-tree 2>$null
    if ($LASTEXITCODE -eq 0) {
        $null = & git -C $RootPath rev-parse --verify HEAD 2>$null
        if ($LASTEXITCODE -eq 0) { $inGit = $true }
    }
} catch { $inGit = $false }

if ($inGit) {
    $EnumMode = "git"
    $lines = & git -C $RootPath ls-tree -r -l HEAD 2>$null
    foreach ($line in $lines) {
        $tab = $line.IndexOf("`t")
        if ($tab -lt 0) { continue }
        $left = $line.Substring(0, $tab)
        $path = $line.Substring($tab + 1)
        $parts = $left -split '\s+'
        if ($parts.Count -lt 4) { continue }
        $size = $parts[3]
        if ($size -notmatch '^\d+$') { continue }
        $files.Add([pscustomobject]@{ Size = [long]$size; Path = $path })
    }
} else {
    $EnumMode = "find"
    $pruneDirs = @('.git','node_modules','dist','build','coverage','__pycache__','.next','.expo','target','.cargo','vendor')
    try {
        $rootFull = $RootPath.TrimEnd('\','/')
        Get-ChildItem -LiteralPath $RootPath -Recurse -File -Force -ErrorAction SilentlyContinue | ForEach-Object {
            $full = $_.FullName
            $rel = $full.Substring($rootFull.Length).TrimStart('\','/') -replace '\\','/'
            $segs = $rel -split '/'
            $skip = $false
            foreach ($s in $segs) { if ($pruneDirs -contains $s) { $skip = $true; break } }
            if (-not $skip) { $files.Add([pscustomobject]@{ Size = [long]$_.Length; Path = $rel }) }
        }
    } catch { }
}

if ($files.Count -eq 0) { Emit-NoData "no analyzable files under root" }

# ---- aggregate per-directory ------------------------------------------------
$bytes  = @{}
$readme = @{}
$code   = @{}
foreach ($f in $files) {
    $p = $f.Path
    if ([string]::IsNullOrEmpty($p)) { continue }
    $i = $p.LastIndexOf('/')
    if ($i -lt 0) { $d = "."; $b = $p } else { $d = $p.Substring(0, $i); $b = $p.Substring($i + 1) }
    if (-not $bytes.ContainsKey($d)) { $bytes[$d] = [long]0; $readme[$d] = 0; $code[$d] = 0 }
    $bytes[$d] += $f.Size
    if ($b -eq "README.md") { $readme[$d] = 1 }
    if ($b -match $CodeRe) { $code[$d]++ }
}

# ---- classify (sorted by est desc so the cap keeps the worst) ---------------
$modules = 0
$over = New-Object System.Collections.Generic.List[object]
$nr   = New-Object System.Collections.Generic.List[object]
foreach ($d in ($bytes.Keys | Sort-Object -Property @{Expression={$bytes[$_]};Descending=$true})) {
    if (Is-Excluded $d) { continue }
    $est = [long][math]::Floor($bytes[$d] / 4)
    if ($readme[$d] -eq 1) {
        $modules++
        if ($est -gt $Band) { $over.Add([pscustomobject]@{ Path = $d; Est = $est }) }
    } elseif ($code[$d] -ge 3) {
        $nr.Add([pscustomobject]@{ Path = $d; Est = $est; Code = $code[$d] })
    }
}

$overTotal = $over.Count
$nrTotal   = $nr.Count

$overFrags = @()
for ($k = 0; $k -lt [Math]::Min($over.Count, $Cap); $k++) {
    $overFrags += '{"path":"' + (Esc $over[$k].Path) + '","est_tokens":' + $over[$k].Est + ',"band":' + $Band + '}'
}
$nrFrags = @()
for ($k = 0; $k -lt [Math]::Min($nr.Count, $Cap); $k++) {
    $nrFrags += '{"path":"' + (Esc $nr[$k].Path) + '","est_tokens":' + $nr[$k].Est + ',"code_files":' + $nr[$k].Code + '}'
}

$ndFrags = @()
$ndTotal = 0
foreach ($e in $noData) {
    if ($ndTotal -lt $Cap) { $ndFrags += '{"path":"' + (Esc $e) + '","reason":"unreadable"}' }
    $ndTotal++
}

$dur = [int]((Get-Date) - $started).TotalMilliseconds

if ($overTotal -gt 0) {
    $status = "warn"
    $noun = if ($overTotal -eq 1) { "module" } else { "modules" }
    $briefing = "module-size: $overTotal $noun over the ~${Band}-token soft band (advisory; est=bytes/4). Consider decomposition or run /0-uldf-uladp-compliance --architecture. Soft band, not a cliff (M2)."
} else {
    $status = "pass"
    $briefing = ""
}

$out = '{"status":"' + $status + '","modules_scanned":' + $modules +
       ',"over_band":[' + ($overFrags -join ',') + '],"over_band_total":' + $overTotal +
       ',"no_readme":[' + ($nrFrags -join ',') + '],"no_readme_total":' + $nrTotal +
       ',"no_data":[' + ($ndFrags -join ',') + '],"no_data_total":' + $ndTotal +
       ',"band":{"softTokens":' + $Band + ',"source":"' + $BandSource + '"}' +
       ',"enum_mode":"' + $EnumMode + '","scan_duration_ms":' + $dur +
       ',"briefing":"' + (Esc $briefing) + '"}'
[Console]::Out.WriteLine($out)
exit 0
