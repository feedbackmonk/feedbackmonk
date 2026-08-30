#!/usr/bin/env bash
#
# Install the `host-tenant-binding` Verification Oracle and the two
# `multi-tenant-isolation-check` allow-list entries FR-FBR-32 requires.
#
# Run this from a session that may write under `.claude/` (an LD/owner session).
# The build session that authored these files was a subordinate worker, and
# DEC-84 (CSI-08) hard-defers `.claude/` writes at every autonomy level — so the
# artefacts were staged here instead of being forced through. Nothing about them
# is provisional: the oracle is finished, adversarially self-tested, and green
# against the current tree; only the copy into the protected directory is left.
#
#   bash scripts/oracles-pending/host-tenant-binding/install.sh
#
# Idempotent: re-running overwrites the oracle files and skips allow-list entries
# that are already present.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$here/../../.." && pwd)"
dest="$repo_root/.claude/oracles/host-tenant-binding"
allowlist="$repo_root/.claude/oracles/multi-tenant-isolation-check/allowlist.toml"

echo "==> installing host-tenant-binding oracle into $dest"
mkdir -p "$dest"
for f in oracle.py oracle.sh oracle.ps1 manifest.json README.md; do
    [ -f "$here/$f" ] && cp "$here/$f" "$dest/$f"
done
chmod +x "$dest/oracle.sh" 2>/dev/null || true

echo "==> appending multi-tenant-isolation-check allow-list entries"
if [ ! -f "$allowlist" ]; then
    echo "    !! $allowlist not found — skipping (check the oracle is installed)" >&2
elif grep -q 'method = "resolve_host"' "$allowlist"; then
    echo "    already present — skipping"
else
    # Strip the explanatory header (everything before the first [[ table) so only
    # the entries are appended.
    {
        printf '\n# --- FR-FBR-32 host-based tenant resolution (added %s) ---\n' "$(date -u +%Y-%m-%d)"
        sed -n '/^\[\[/,$p' "$here/allowlist-additions.toml"
    } >> "$allowlist"
    echo "    appended 2 entries"
fi

echo
echo "==> verifying"
bash "$repo_root/.claude/oracles/multi-tenant-isolation-check/oracle.sh" || true
bash "$dest/oracle.sh" || true
echo
echo "Both should read PASS. Add --full to the host-tenant-binding run for the"
echo "behavioural leg (needs DATABASE_URL + Postgres)."
echo
echo "Once green, delete scripts/oracles-pending/host-tenant-binding/ — it is a"
echo "staging area, not a second home for the oracle."
