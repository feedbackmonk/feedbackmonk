# run.ps1 -- PowerShell entrypoint for the code-graph oracle (scrutiny Arc 3 A1).
# Thin wrapper: resolves a working python (PW-005 -- the WindowsApps python3 shim
# is broken) and forwards to code-graph.py, so a PowerShell caller and a bash
# caller (run.sh) drive BYTE-IDENTICAL JSON. The query logic lives in
# code-graph.py; the frozen schema + coverage-honesty rules + the reuse of the
# dependency-drift --emit-edges seam live in README.md. ASCII-only (PW-005).
#
# Usage:
#   run.ps1 [--deps M | --consumers M | --impact M | --cycles]
#           [--transitive] [--root DIR] [--config FILE] [--compact]
# Advisory: exits 0 always.
$ErrorActionPreference = 'Stop'
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$py = $null
foreach ($c in @('python', 'python3', 'py')) {
  $cmd = Get-Command $c -ErrorAction SilentlyContinue
  if ($cmd) {
    try { & $c -c 'import json,sys' 2>$null; if ($LASTEXITCODE -eq 0) { $py = $c; break } } catch { }
  }
}
if (-not $py) {
  Write-Output '{ "status": "no-data", "schemaVersion": "1.0", "query": { "verb": "summary", "target": null, "transitive": false }, "result": [], "coverage": "none", "coverageNote": "", "truncated": false, "reason": "no working python interpreter", "extractor": { "configured": false, "coveredModules": [] }, "briefing": "" }'
  exit 0
}
& $py (Join-Path $ScriptDir 'code-graph.py') @args
exit $LASTEXITCODE
