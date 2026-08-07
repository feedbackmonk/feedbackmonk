#!/bin/bash
# background-job-status oracle (Unix entry).
# The sentinel-writer wrapper is PowerShell on the primary (Windows) platform.
# On Unix with pwsh, delegate to run.ps1 for identical semantics. Otherwise,
# read the JSON sentinels directly (graceful, schema-compatible subset).

set -e
oracle_dir="$(cd "$(dirname "$0")" && pwd)"

if command -v pwsh >/dev/null 2>&1; then
    pwsh -NoProfile -File "$oracle_dir/run.ps1" "$@"
    exit $?
fi

# Pure-bash fallback: emit a minimal answer over the sentinel files.
repo_root="$(cd "$oracle_dir/../../.." && pwd)"
jobs_dir="$repo_root/.claude/session-state/jobs"

if [ ! -d "$jobs_dir" ]; then
    echo '{"ok":true,"jobs":[],"count":0,"total":0,"pending":[],"unacknowledged":[],"briefing":"","summary":"No tracked jobs (jobs dir does not exist yet)."}'
    exit 0
fi

if command -v jq >/dev/null 2>&1; then
    # Assemble with jq for correctness.
    jq -s '
      { ok: true,
        jobs: .,
        count: (. | length),
        total: (. | length),
        pending: ([.[] | select(.status=="running") | .job_id]),
        unacknowledged: ([.[] | select((.status=="done" or .status=="failed") and (.acknowledged != true)) | .job_id]),
        briefing: (
          ([.[] | select(.status=="running")] | length) as $p
          | ([.[] | select((.status=="done" or .status=="failed") and (.acknowledged != true))] | length) as $u
          | if ($p+$u)>0 then "tracked jobs need attention — \($p) running, \($u) finished-unacknowledged (status: .claude/oracles/background-job-status/run.ps1 <id>)" else "" end
        ),
        summary: "tracked-job summary (unix jq fallback)"
      }' "$jobs_dir"/*.json 2>/dev/null
else
    echo '{"ok":true,"jobs":[],"count":0,"briefing":"","summary":"pwsh and jq both unavailable; read sentinels under .claude/session-state/jobs/ directly.","_error":"no-pwsh-no-jq"}'
fi
exit 0
