# stranded-dirty-files oracle (Windows)
#
# CSI-15 (Phase 1.7): emit a [stranded-dirty-files] briefing line when this
# project's working tree contains dirty files older than the most recent
# finalize commit AND no live peer (per dispatchable-sessions registry) owns
# them.
#
# Visibility-only — never mutates state. Cleanup is user-driven via
# /0-uldf-finalize --include-stranded (FINALIZE-04, same Arc 2).
#
# Output: single-line JSON matching the FROZEN output schema (oracle.json).
# Briefing field is empty string when count == 0 so the session-start hook
# gracefully suppresses the line (parallel to stale-ltads-state's silence pattern).

$ErrorActionPreference = 'SilentlyContinue'

$registry         = ".claude/collaboration/active-sessions.json"
$scopeGuardMax    = 2000
$sampleCap        = 10
$largeThreshold   = 50

# ---- SWEEP-10 / DEFER-095: identity-aware liveness via lib/pid-liveness.ps1 --
# A recycled peer pid inflated live_peer_count and its dirtyFiles kept masking
# genuinely stranded files. Anchor-only, no name glob (DEC-257); absent anchor
# or lib unavailable -> byte-identical existence-only verdict. Path REPORTABLE
# (QUIESCE-08 W4). Dot-sourced at SCRIPT scope.
$sdfPidIdentity = 'fallback'
foreach ($sdfPlCand in @(
    (Join-Path $PSScriptRoot "../../scripts/lib/pid-liveness.ps1"),
    (Join-Path $PSScriptRoot "../../../claude-template/scripts/lib/pid-liveness.ps1"),
    (Join-Path $env:USERPROFILE ".claude/scripts/lib/pid-liveness.ps1")
)) {
    if (Test-Path -LiteralPath $sdfPlCand) {
        try {
            if (-not (Test-Path variable:global:_UldfPidLivenessLoaded)) { $global:_UldfPidLivenessLoaded = $false }
            . $sdfPlCand
            if (Get-Command Test-UldfPidAliveAs -ErrorAction SilentlyContinue) { $sdfPidIdentity = 'lib' }
        } catch { }
        break
    }
}
if ($env:ULDF_SDF_REPORT_PID_IDENTITY -eq '1') {
    Write-Output $sdfPidIdentity
    exit 0
}

function Emit-Json($obj) {
    # Compact one-line JSON, dictionaries preserve insertion order via [ordered]
    $json = $obj | ConvertTo-Json -Compress -Depth 6
    Write-Output $json
    exit 0
}

function Emit-Empty([string]$lastFinalize, [int]$livePeerCount, [string]$attribution) {
    $obj = [ordered]@{
        has_stranded     = $false
        count            = 0
        oldest_mtime     = $null
        sample           = @()
        live_peer_count  = $livePeerCount
        provably_unowned = 0
        attribution      = $attribution
        last_finalize_at = $lastFinalize
        briefing         = ""
    }
    Emit-Json $obj
}

# TWIN-01 / DEC-263: ConvertFrom-Json coerces ISO-8601 strings to [DateTime] on
# pwsh 6+ and leaves them as strings on powershell.exe 5.1, so a registry
# timestamp read here is EITHER type depending on the engine. Normalize at this
# parse boundary (the Convert-*NormalizeDates pattern), never downstream.
# Returns $null when the value is absent or unparseable -- and $null is what
# disables the STRAND-02 proof for the whole run (fail closed).
function ConvertTo-SdfUtcOrNull($v) {
    if ($null -eq $v) { return $null }
    if ($v -is [datetime]) { return ([datetime]$v).ToUniversalTime() }
    $s = [string]$v
    if ([string]::IsNullOrWhiteSpace($s)) { return $null }
    try { return ([DateTimeOffset]::Parse($s, [System.Globalization.CultureInfo]::InvariantCulture)).UtcDateTime } catch { return $null }
}

function Iso-Utc([datetime]$dt) {
    $u = $dt.ToUniversalTime()
    return $u.ToString("yyyy-MM-ddTHH:mm:ssZ", [System.Globalization.CultureInfo]::InvariantCulture)
}

# ---- Graceful absence: not in a git repo -----------------------------------
& git rev-parse --git-dir 2>$null | Out-Null
if ($LASTEXITCODE -ne 0) {
    Emit-Empty $null 0 'no-peers'
}

# ---- last_finalize_at = HEAD's commit timestamp ----------------------------
$lastFinalizeIsoRaw = & git log -1 --format=%aI HEAD 2>$null
$lastFinalizeIso    = $null
$lastFinalizeUtc    = $null
if ($null -ne $lastFinalizeIsoRaw) {
    $lastFinalizeIsoRaw = ($lastFinalizeIsoRaw | Out-String).Trim()
}
if (-not [string]::IsNullOrEmpty($lastFinalizeIsoRaw)) {
    try {
        $dto = [DateTimeOffset]::Parse($lastFinalizeIsoRaw)
        $lastFinalizeUtc = $dto.UtcDateTime
        # Re-emit normalized to UTC Z form for the briefing schema.
        $lastFinalizeIso = Iso-Utc $lastFinalizeUtc
    } catch {
        $lastFinalizeIso = $lastFinalizeIsoRaw  # Best-effort echo of original
    }
}

# ---- Dirty file set --------------------------------------------------------
$dirtyRaw = & git status --porcelain 2>$null
$dirtyFiles = New-Object System.Collections.Generic.List[string]
if ($null -ne $dirtyRaw) {
    foreach ($line in @($dirtyRaw)) {
        if ([string]::IsNullOrEmpty($line)) { continue }
        if ($line.Length -lt 3) { continue }
        $xy = $line.Substring(0, 2)
        $rest = $line.Substring(3)
        # Working-tree deletion -> no mtime; skip
        if ($xy -match '^[ AMRC]D$' -or $xy -eq 'D ' -or $xy -eq ' D' -or $xy -eq 'DD') { continue }
        # Rename: "old -> new" - keep new
        if ($xy -match '^R' -or $xy -match '^.R$') {
            if ($rest -match ' -> ') {
                $rest = ($rest -split ' -> ')[-1]
            }
        }
        $dirtyFiles.Add($rest) | Out-Null
    }
}

$dirtyCount = $dirtyFiles.Count

# ---- Scope guard: too many dirty files -> detection skipped ----------------
if ($dirtyCount -gt $scopeGuardMax) {
    $briefing = "stranded-dirty-files: detection skipped - too many dirty files (>${scopeGuardMax}); run /0-uldf-finalize --include-stranded for full audit"
    $obj = [ordered]@{
        has_stranded     = $false
        count            = -1
        oldest_mtime     = $null
        sample           = @()
        live_peer_count  = 0
        provably_unowned = 0
        attribution      = 'not-measured'
        last_finalize_at = $lastFinalizeIso
        briefing         = $briefing
    }
    Emit-Json $obj
}

# ---- Empty dirty set -> graceful empty -------------------------------------
if ($dirtyCount -eq 0) {
    Emit-Empty $lastFinalizeIso 0 'no-peers'
}

# ---- Build live-peer ownership set -----------------------------------------
$projRoot = (Get-Location).Path
$projRootNorm = ($projRoot -replace '\\', '/').TrimEnd('/')

$livePeerCount = 0
$attributingPeerCount = 0
$journalPeerCount = 0
$ownedSet = New-Object System.Collections.Generic.HashSet[string]

# STRAND-02 (DEC-372): the mtime-ordering proof. A live peer that had written
# file F would have set F's mtime at or after its OWN start, so a file older
# than every live peer's `spawnedAt` cannot be any visible live peer's in-flight
# work. Full rationale (why spawnedAt is a sound lower bound, and why an absent
# one disables the proof for the whole run rather than dropping that peer from
# the min) is in the sh twin -- it is the same three checked facts.
$earliestPeerStartUtc = $null
$peerStartUnknown = $false

if (Test-Path $registry) {
    $reg = $null
    try {
        $bytes = [System.IO.File]::ReadAllBytes($registry)
        if ($bytes.Length -gt 0) {
            $rawText = $null
            if ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) {
                $rawText = [System.Text.Encoding]::UTF8.GetString($bytes, 3, $bytes.Length - 3)
            } else {
                $rawText = [System.Text.Encoding]::UTF8.GetString($bytes)
            }
            if (-not [string]::IsNullOrWhiteSpace($rawText)) {
                $reg = $rawText | ConvertFrom-Json -ErrorAction Stop
            }
        }
    } catch {
        $reg = $null
    }

    if ($null -ne $reg -and $reg.PSObject.Properties.Name -contains 'sessions' -and $reg.sessions) {
        foreach ($s in @($reg.sessions)) {
            if ($null -eq $s) { continue }
            if ($s.status -ne 'active') { continue }
            $sPid = $null
            if ($s.PSObject.Properties.Name -contains 'claudeShellPid' -and $null -ne $s.claudeShellPid) {
                try { $sPid = [int]$s.claudeShellPid } catch { $sPid = $null }
            }
            if ($null -eq $sPid -or $sPid -le 0) { continue }
            $sWd = ""
            if ($s.PSObject.Properties.Name -contains 'workDir' -and $null -ne $s.workDir) {
                $sWd = ([string]$s.workDir -replace '\\', '/').TrimEnd('/')
            }
            if ($sWd -ne $projRootNorm) { continue }
            # Liveness probe (DEFER-095: identity-aware -- a recycled pid,
            # started after the entry's own claudeShellPidWrittenAt anchor,
            # reads dead; absent anchor/lib -> existence-only).
            $sAnchor = ''
            if ($s.PSObject.Properties.Name -contains 'claudeShellPidWrittenAt' -and $null -ne $s.claudeShellPidWrittenAt) {
                $sAnchor = [string]$s.claudeShellPidWrittenAt
            }
            $alive = $false
            try {
                if ($sdfPidIdentity -eq 'lib') {
                    $alive = [bool](Test-UldfPidAliveAs $sPid $sAnchor)
                } else {
                    $proc = Get-Process -Id $sPid -ErrorAction SilentlyContinue
                    if ($null -ne $proc) { $alive = $true }
                }
            } catch { $alive = $false }
            if (-not $alive) { continue }

            $livePeerCount++
            $sPublished = $false      # STRONG: a peer-published dirtyFiles array
            $sJournalled = $false     # POSITIVE-ONLY: the peer's write journal

            # STRAND-02: fold this peer into the earliest-start floor. An absent
            # or unparseable spawnedAt disables the proof for the WHOLE run --
            # dropping this peer from the min() would RAISE the floor and prove
            # more, which is the unsafe direction.
            $sStartUtc = $null
            if ($s.PSObject.Properties.Name -contains 'spawnedAt') {
                $sStartUtc = ConvertTo-SdfUtcOrNull $s.spawnedAt
            }
            if ($null -eq $sStartUtc) {
                $peerStartUnknown = $true
            } elseif ($null -eq $earliestPeerStartUtc -or $sStartUtc -lt $earliestPeerStartUtc) {
                $earliestPeerStartUtc = $sStartUtc
            }

            # Source 1 (original): the peer's published dirtyFiles array.
            # DEFER-039/DEFER-190: measured (GitCellar 2026-08-12, ULDF 2026-08-16),
            # NO session writes this field, so on its own the owned-set was empty
            # on every run and "no live owner" was asserted over zero data. Kept
            # because it is authoritative when present, and costs nothing when
            # absent.
            if ($s.PSObject.Properties.Name -contains 'dirtyFiles' -and $null -ne $s.dirtyFiles) {
                $sPublished = $true
                foreach ($df in @($s.dirtyFiles)) {
                    if ($null -eq $df) { continue }
                    [void]$ownedSet.Add([string]$df)
                }
            }

            # Source 2 (DEFER-039 option 2): the peer's WRITE JOURNAL, which is
            # actually written -- the post-tool-use hook appends {ts, path, op}
            # per Edit/Write to .claude/session-state/write-journal/<sessionId>.jsonl.
            # This is the same input lib/file-owner.sh resolves ownership from, so
            # the oracle now asks the question the rest of the shared-tree
            # machinery already answers.
            #
            # THE JOURNAL IS POSITIVE-ONLY EVIDENCE, and this is load-bearing.
            # It records TOOL CALLS, so a write made any other way -- a script
            # the agent ran, an external editor -- leaves no record (same bound
            # DEC-359 measured on the append-only docs: 65% of spec-doc commits
            # had no journal record at all). So a journal HIT rescues a file
            # from the strand list, but a journal MISS proves nothing. Treating
            # journal coverage as completeness is how this oracle would go
            # straight back to recommending a sweep over a live peer's work --
            # with a confident 'measured' next to it.
            $sId = ""
            if ($s.PSObject.Properties.Name -contains 'sessionId' -and $null -ne $s.sessionId) {
                $sId = [string]$s.sessionId
            }
            if (-not [string]::IsNullOrWhiteSpace($sId)) {
                $jPath = Join-Path ".claude/session-state/write-journal" ($sId + ".jsonl")
                if (Test-Path -LiteralPath $jPath) {
                    $sJournalled = $true
                    try {
                        foreach ($jLine in [System.IO.File]::ReadLines((Resolve-Path -LiteralPath $jPath))) {
                            if ([string]::IsNullOrWhiteSpace($jLine)) { continue }
                            $rec = $null
                            try { $rec = $jLine | ConvertFrom-Json -ErrorAction Stop } catch { continue }
                            if ($null -eq $rec -or -not ($rec.PSObject.Properties.Name -contains 'path')) { continue }
                            $jp = ([string]$rec.path -replace '\\', '/')
                            if ([string]::IsNullOrWhiteSpace($jp)) { continue }
                            # Journal paths are absolute; keep only this project's,
                            # and store them repo-relative to match git status output.
                            if ($jp.StartsWith(($projRootNorm + "/"), [StringComparison]::OrdinalIgnoreCase)) {
                                [void]$ownedSet.Add($jp.Substring($projRootNorm.Length + 1))
                            }
                        }
                    } catch { }
                }
            }

            if ($sPublished)  { $attributingPeerCount++ }
            if ($sJournalled) { $journalPeerCount++ }
        }
    }
}

# ---- Attribution verdict (DEFER-039 / DEFER-190) ----------------------------
# 'measured'        -- at least one live peer published a dirtyFiles array
#                      (complete by construction), so an absent path is evidence
#                      of non-ownership.
# 'journal-partial' -- attribution came only from write journals, which are
#                      positive-only: hits filter, a miss proves nothing, so the
#                      sweep recommendation is withheld.
# 'unavailable'     -- live peers exist but NONE supplied attribution by either
#                      route. Silence is not evidence: nothing here can be
#                      called un-owned, so the sweep recommendation is withheld.
# 'no-peers'        -- no live peer in this project, so there is no owner to miss.
# 'predates-peers'  -- STRAND-02: decided AFTER the walk (a property of the
#                      reported set), below. 'measured' is RESERVED: no producer
#                      exists anywhere in the framework (DISC-ORA-12).
if ($livePeerCount -eq 0) {
    $attribution = 'no-peers'
} elseif ($attributingPeerCount -gt 0) {
    $attribution = 'measured'
} elseif ($journalPeerCount -gt 0) {
    $attribution = 'journal-partial'
} else {
    $attribution = 'unavailable'
}

# ---- Walk dirty files, classify, build sample ------------------------------
$nowUtc = [DateTime]::UtcNow
$strandCount = 0
$oldestUtc = $null
$sample = New-Object System.Collections.Generic.List[object]
$provedCount = 0     # STRAND-02: reported strands proved un-owned by mtime ordering
$unprovedCount = 0   # STRAND-02: reported strands the proof does NOT cover

foreach ($f in $dirtyFiles) {
    if ([string]::IsNullOrEmpty($f)) { continue }
    if (-not (Test-Path -LiteralPath $f)) { continue }
    if ($ownedSet.Contains($f)) { continue }
    if ($null -eq $lastFinalizeUtc) { continue }   # No finalize boundary -> no strands

    try {
        $info = Get-Item -LiteralPath $f -Force -ErrorAction Stop
    } catch { continue }
    if ($info -is [System.IO.DirectoryInfo]) { continue }

    $mtimeUtc = $info.LastWriteTimeUtc
    if ($mtimeUtc -ge $lastFinalizeUtc) { continue }

    $strandCount++
    # STRAND-02: proved un-owned by every live peer? With zero live peers the
    # proposition is vacuously true (there is no owner to miss).
    if ($livePeerCount -eq 0) {
        $provedCount++
    } elseif ((-not $peerStartUnknown) -and $null -ne $earliestPeerStartUtc -and $mtimeUtc -lt $earliestPeerStartUtc) {
        $provedCount++
    } else {
        $unprovedCount++
    }
    if ($null -eq $oldestUtc -or $mtimeUtc -lt $oldestUtc) { $oldestUtc = $mtimeUtc }

    if ($strandCount -le $sampleCap) {
        $ageDays = [int][Math]::Floor(($nowUtc - $mtimeUtc).TotalDays)
        $pNorm = ($f -replace '\\', '/')
        $sample.Add([ordered]@{
            path     = $pNorm
            mtime    = Iso-Utc $mtimeUtc
            age_days = $ageDays
        }) | Out-Null
    }
}

# ---- Emit results ----------------------------------------------------------
if ($strandCount -eq 0) {
    Emit-Empty $lastFinalizeIso $livePeerCount $attribution
}

$oldestIso  = Iso-Utc $oldestUtc
$oldestAge  = [int][Math]::Floor(($nowUtc - $oldestUtc).TotalDays)

# ---- Attribution verdict, part 2: the STRAND-02 mtime proof ------------------
# All-or-nothing over the reported set, because the sweep it gates is
# all-or-nothing. Sits BELOW 'measured' and ABOVE the withholding grades.
$proofUsable = ((-not $peerStartUnknown) -and $null -ne $earliestPeerStartUtc)
if ($livePeerCount -gt 0 -and $attributingPeerCount -eq 0 -and $proofUsable -and $unprovedCount -eq 0) {
    $attribution = 'predates-peers'
}

# A withheld run states how much of the list IS proved, so the refusal is
# actionable rather than opaque (DEC-360 applied to the report).
$provedClause = ""
if ($livePeerCount -gt 0 -and $proofUsable) {
    $provedClause = " $provedCount of $strandCount predate every live peer's session start; $unprovedCount do not."
}

if ($attribution -eq 'journal-partial') {
    # Journal hits removed what they could from the list above; what remains is
    # unproven, not un-owned. Same withholding as 'unavailable', different reason.
    $briefing = "stranded-dirty-files: $strandCount pre-finalize dirty file(s) (oldest $oldestAge days) - OWNERSHIP UNPROVEN: attribution came only from $journalPeerCount write journal(s), which record tool calls and cannot see script or external writes, so a miss is not evidence of non-ownership.$provedClause Do NOT sweep; inspect with /0-uldf-oracle stranded-dirty-files"
} elseif ($attribution -eq 'unavailable') {
    # DEFER-039: OVALID -- an oracle that cannot see its subject says so instead
    # of asserting over it. With no live peer publishing attribution there is NO
    # evidence any of these files is un-owned, so the sweep recommendation is
    # withheld: acting on it would mean finalizing a live peer's in-flight work,
    # which is exactly what DEC-239 exists to prevent.
    $briefing = "stranded-dirty-files: $strandCount pre-finalize dirty file(s) (oldest $oldestAge days) - OWNERSHIP UNKNOWN: $livePeerCount live peer(s) and none publishes attribution, so none of these is shown to be un-owned.$provedClause Do NOT sweep; inspect with /0-uldf-oracle stranded-dirty-files"
} elseif ($attribution -eq 'predates-peers' -and $strandCount -lt $largeThreshold) {
    $briefing = "stranded-dirty-files: $strandCount files (oldest $oldestAge days; no live owner - every one predates all $livePeerCount live peer(s)' session start) - run /0-uldf-finalize --include-stranded for cleanup"
} elseif ($strandCount -lt $largeThreshold) {
    if ($attribution -eq 'measured') {
        $briefing = "stranded-dirty-files: $strandCount files (oldest $oldestAge days; no live owner per $attributingPeerCount attributing peer(s)) - run /0-uldf-finalize --include-stranded for cleanup"
    } else {
        # no-peers: trivially true "no live owner" -- there is no owner to miss.
        $briefing = "stranded-dirty-files: $strandCount files (oldest $oldestAge days; no live owner - no live peers in this project) - run /0-uldf-finalize --include-stranded for cleanup"
    }
} else {
    $briefing = "stranded-dirty-files: $strandCount files (oldest $oldestAge days) - significant accumulation; see /0-uldf-oracle stranded-dirty-files for full sample"
}

$obj = [ordered]@{
    has_stranded     = $true
    count            = $strandCount
    oldest_mtime     = $oldestIso
    sample           = $sample
    live_peer_count  = $livePeerCount
    provably_unowned = $provedCount
    attribution      = $attribution
    last_finalize_at = $lastFinalizeIso
    briefing         = $briefing
}
Emit-Json $obj
