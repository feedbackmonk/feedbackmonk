# machine-quiescence — Windows raw-fact probe (QUIESCE-02)
#
# Emits RAW OS FACTS as TSV, one per line. It performs NO classification and
# reaches NO verdict — that logic is twinned in run.sh / run.ps1 and is what the
# parity smoke exercises over a fixture.
#
#   listener<TAB><port><TAB><pid><TAB><processName>
#   process<TAB><pid><TAB><name><TAB><commandLine>
#   degraded<TAB><reason>
#
# Shared by BOTH twins: run.ps1 dot-invokes it, and run.sh shells out to it once
# on MINGW/MSYS/CYGWIN. One OS probe, two classifiers — the opposite of the
# duplicated-and-wrong parse that DEC-208 exists to undo.
#
# Ground truth is the OS. Nothing here reads a session registry or a PID file:
# the brief's leg #3 (stale `active-sessions.json` PIDs reporting false
# liveness) is precisely why registries may name residue but must never be the
# thing that detects it.

$ErrorActionPreference = "Continue"
try {
    [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
    $OutputEncoding = [System.Text.UTF8Encoding]::new()
} catch {}

$lines = New-Object System.Collections.ArrayList

# ---- Listening TCP sockets ------------------------------------------------
$gotListeners = $false
try {
    $conns = Get-NetTCPConnection -State Listen -ErrorAction Stop
    $gotListeners = $true
    $seen = @{}
    foreach ($c in $conns) {
        $key = "$($c.LocalPort)/$($c.OwningProcess)"
        if ($seen.ContainsKey($key)) { continue }
        $seen[$key] = $true
        $pname = "?"
        try {
            $p = Get-Process -Id $c.OwningProcess -ErrorAction SilentlyContinue
            if ($p) { $pname = $p.ProcessName }
        } catch {}
        [void]$lines.Add("listener`t$($c.LocalPort)`t$($c.OwningProcess)`t$pname")
    }
} catch {
    # Fall back to netstat before giving up — a degraded probe must be declared,
    # never silently emptied (an empty listener set reads as "quiet").
    try {
        $ns = netstat -ano 2>$null | Select-String -Pattern '^\s*TCP\s+\S+:(\d+)\s+\S+\s+LISTENING\s+(\d+)'
        if ($ns) {
            $gotListeners = $true
            $seen = @{}
            foreach ($m in $ns) {
                $port = $m.Matches[0].Groups[1].Value
                $procId = $m.Matches[0].Groups[2].Value
                $key = "$port/$procId"
                if ($seen.ContainsKey($key)) { continue }
                $seen[$key] = $true
                $pname = "?"
                try {
                    $p = Get-Process -Id ([int]$procId) -ErrorAction SilentlyContinue
                    if ($p) { $pname = $p.ProcessName }
                } catch {}
                [void]$lines.Add("listener`t$port`t$procId`t$pname")
            }
        }
    } catch {}
}
if (-not $gotListeners) {
    [void]$lines.Add("degraded`tno listening-socket probe available (Get-NetTCPConnection and netstat both failed)")
}

# ---- Processes ------------------------------------------------------------
$gotProcesses = $false
try {
    $procs = Get-CimInstance -ClassName Win32_Process -ErrorAction Stop
    $gotProcesses = $true
    foreach ($p in $procs) {
        $name = if ($p.Name) { $p.Name } else { "?" }
        $cmd = if ($p.CommandLine) { $p.CommandLine } else { "" }
        # TSV integrity: collapse embedded tabs/newlines in the command line.
        $cmd = $cmd -replace "[`t`r`n]", " "
        [void]$lines.Add("process`t$($p.ProcessId)`t$name`t$cmd")
    }
} catch {
    # No CIM (locked-down host / no WMI): fall back to names only. Command lines
    # are how a `node.exe` is told apart from a watcher, so declare the loss.
    try {
        foreach ($p in (Get-Process -ErrorAction SilentlyContinue)) {
            [void]$lines.Add("process`t$($p.Id)`t$($p.ProcessName)`t")
        }
        $gotProcesses = $true
        [void]$lines.Add("degraded`tcommand lines unavailable (no CIM) - build/watch processes under a generic host name cannot be identified")
    } catch {}
}
if (-not $gotProcesses) {
    [void]$lines.Add("degraded`tno process enumeration available")
}

$lines -join "`n"
