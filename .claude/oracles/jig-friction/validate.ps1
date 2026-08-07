# jig-friction oracle self-test (Windows) -- JIG-04, DEC-142. Parity with validate.sh.
# Self-contained: synthesizes a tiny in-temp corpus and asserts BOTH branches --
# the `signal` schema and the quiet-path invariant. PW-005: UTF-8; ASCII-only.
$ErrorActionPreference = 'Stop'
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch { }

$OracleDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$Run = Join-Path $OracleDir 'run.ps1'

function Fail([string]$msg, [string]$raw) {
    [Console]::Error.WriteLine("FAIL: $msg")
    if ($raw) { [Console]::Error.WriteLine($raw) }
    exit 1
}

$Frozen = @('fabricator','state-fabricator','scenario-replayer','log-distiller',
            'diff-summarizer','environment-resetter','corpus-harvester')

# --- signal case: 5 identical named invocations in one session --------------
$SigDir = Join-Path ([System.IO.Path]::GetTempPath()) ("jf-sig-" + [Guid]::NewGuid().ToString('N'))
$QuietDir = Join-Path ([System.IO.Path]::GetTempPath()) ("jf-quiet-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $SigDir -Force | Out-Null
New-Item -ItemType Directory -Path $QuietDir -Force | Out-Null
try {
    $lines = @()
    foreach ($t in '00','01','02','03','04') {
        $lines += ('{"at":"2026-07-05T09:00:' + $t + '","cmd":"mcp:playwright/browser_take_screenshot","type":"mcp","session":"sess-v","project":"P","machine":"M"}')
    }
    Set-Content -Path (Join-Path $SigDir 'x.jsonl') -Value $lines -Encoding UTF8

    $signalRaw = (& pwsh -NoProfile -File $Run --dir $SigDir --session sess-v 2>$null | Out-String).Trim()
    if (-not $signalRaw) { $signalRaw = (& powershell -NoProfile -File $Run --dir $SigDir --session sess-v 2>$null | Out-String).Trim() }
    $quietRaw = (& pwsh -NoProfile -File $Run --dir $QuietDir 2>$null | Out-String).Trim()
    if (-not $quietRaw) { $quietRaw = (& powershell -NoProfile -File $Run --dir $QuietDir 2>$null | Out-String).Trim() }

    try { $sig = $signalRaw | ConvertFrom-Json } catch { Fail 'signal output is not valid JSON' $signalRaw }
    try { $quiet = $quietRaw | ConvertFrom-Json } catch { Fail 'quiet output is not valid JSON' $quietRaw }

    foreach ($pair in @(@($sig,'signal'), @($quiet,'quiet'))) {
        $o = $pair[0]; $label = $pair[1]
        foreach ($k in 'status','signals','briefing') {
            if ($null -eq $o.PSObject.Properties[$k]) { Fail "missing schema key '$k' on $label output" $signalRaw }
        }
    }

    # signal branch
    if ($sig.status -ne 'signal') { Fail 'expected status=signal on the 5x fixture' $signalRaw }
    $sigs = @($sig.signals)
    if ($sigs.Count -lt 1) { Fail 'expected >=1 signal on the 5x fixture' $signalRaw }
    $s0 = $sigs[0]
    foreach ($k in 'pattern','count','evidence','suggestedArchetype') {
        if ($null -eq $s0.PSObject.Properties[$k]) { Fail "signal object missing key '$k'" $signalRaw }
    }
    if ([int]$s0.count -ne 5) { Fail ("expected count=5, got " + $s0.count) $signalRaw }
    if (@($s0.evidence).Count -ne 5) { Fail ("expected 5 verbatim evidence entries, got " + @($s0.evidence).Count) $signalRaw }
    if ($Frozen -notcontains $s0.suggestedArchetype) { Fail ("suggestedArchetype '" + $s0.suggestedArchetype + "' is not a frozen catalog slug") $signalRaw }
    if (-not $sig.briefing) { Fail 'status=signal must carry a non-empty briefing' $signalRaw }

    # quiet branch + quiet-path invariant
    if ($quiet.status -ne 'ok') { Fail 'expected status=ok on empty data' $quietRaw }
    if (@($quiet.signals).Count -ne 0) { Fail 'expected empty signals on empty data' $quietRaw }
    if ($quiet.briefing -ne '') { Fail 'QUIET-PATH INVARIANT violated: status=ok but briefing is non-empty' $quietRaw }

    [Console]::Out.WriteLine('PASS: jig-friction oracle validates (signal schema + quiet-path invariant)')
    exit 0
}
finally {
    Remove-Item -Recurse -Force $SigDir, $QuietDir -ErrorAction SilentlyContinue
}
