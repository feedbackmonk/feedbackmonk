#!/usr/bin/env python3
"""widget-bundle-size Verification Oracle (canonical implementation).

Two probes:
  A) Total byte size of every file in `widget/dist/` matching `*.{js,mjs,css}`
     (post-minification, post-terser, pre-gzip) is at most 30720 bytes
     (= 30 * 1024). The cap defends FR-FBR-04's <30KB contract as a
     code-level invariant rather than aspiration.
  B) No canonical third-party tracker hostname appears anywhere in any
     `widget/dist/*` file. The hostname list is hashed and printed in
     the report so that silent shrinking of the list (Q5 drift) surfaces
     as a visible hash change.

The list of forbidden hostnames is sourced from
`expected-trackers.txt` (newline-delimited; `#` comments stripped;
blank lines ignored). The file's SHA-256 over the canonical-serialised
list is computed and emitted with every report — drift defender.

Cold-start (no `widget/dist/` yet) emits GREEN: 0 files = 0 bytes <= cap;
nothing to scan for trackers. This is intentional so the oracle can land
BEFORE the widget source, then re-evaluate after every build.

Output: machine-parseable PASS / FAIL. Exit 0 on PASS, 1 on FAIL,
2 on environment failure.

Lineage:
- FR-FBR-04 (<30KB widget) + DEC-FBR-02 (no third-party trackers brand promise)
- P2 plan § Oracle Pre-Build Plan (Probe A + Probe B + drift-detection)
- Three-leg defense pattern (DEC-FBR-IMPL-03):
    leg 1 = vite.config.ts terser+CSP-safe bundler config
    leg 2 = THIS oracle (size probe + hostname probe + list-hash drift)
    leg 3 = Playwright + axe-core a11y harness (catches behaviour regressions)
"""
from __future__ import annotations

import hashlib
import sys
from pathlib import Path
from typing import List, Tuple


SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parents[2]
WIDGET_DIST = REPO_ROOT / "widget" / "dist"
EXPECTED_TRACKERS = SCRIPT_DIR / "expected-trackers.txt"

# Cap = 30 KiB. Hard contract per FR-FBR-04. Reviewable change only:
# changing this constant requires a deliberate spec-level decision.
SIZE_CAP_BYTES = 30 * 1024

# File extensions in scope. `mjs` is included for ESM output if vite emits it.
BUNDLE_EXTENSIONS = {".js", ".mjs", ".css"}


def rel(p: Path) -> str:
    try:
        return str(p.relative_to(REPO_ROOT)).replace("\\", "/")
    except ValueError:
        return str(p).replace("\\", "/")


def load_trackers() -> Tuple[List[str], str]:
    """Returns (hostnames_sorted, sha256_hex_of_canonical_form).

    Canonical form: each hostname lowercased + stripped, sorted, joined with
    "\\n", UTF-8 encoded. The hash includes only the actual hostname list,
    not comments or blank lines — so cosmetic edits don't churn the hash.
    """
    if not EXPECTED_TRACKERS.exists():
        return [], ""
    raw = EXPECTED_TRACKERS.read_text(encoding="utf-8")
    hosts: List[str] = []
    for line in raw.splitlines():
        s = line.strip()
        if not s or s.startswith("#"):
            continue
        hosts.append(s.lower())
    hosts.sort()
    canonical = "\n".join(hosts).encode("utf-8")
    return hosts, hashlib.sha256(canonical).hexdigest()


def probe_a_size() -> Tuple[int, List[Tuple[str, int]], bool]:
    """Sum bundle sizes. Returns (total_bytes, per_file_list, exceeded_cap)."""
    per_file: List[Tuple[str, int]] = []
    total = 0
    if not WIDGET_DIST.exists():
        return 0, per_file, False
    for path in sorted(WIDGET_DIST.rglob("*")):
        if not path.is_file():
            continue
        if path.suffix.lower() not in BUNDLE_EXTENSIONS:
            continue
        try:
            size = path.stat().st_size
        except OSError:
            continue
        per_file.append((rel(path), size))
        total += size
    return total, per_file, total > SIZE_CAP_BYTES


def probe_b_trackers(hosts: List[str]) -> List[Tuple[str, str, int]]:
    """Returns offenders = [(hostname, file, line_no), ...]."""
    offenders: List[Tuple[str, str, int]] = []
    if not WIDGET_DIST.exists():
        return offenders
    if not hosts:
        # No tracker list at all → cannot scan. Treat as soft pass (the
        # expected-trackers.txt presence check at probe-time handles
        # missing-file failure separately).
        return offenders
    lower_hosts = [h.lower() for h in hosts]
    for path in sorted(WIDGET_DIST.rglob("*")):
        if not path.is_file():
            continue
        if path.suffix.lower() not in BUNDLE_EXTENSIONS:
            continue
        try:
            raw = path.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue
        lines = raw.splitlines()
        for i, line in enumerate(lines, start=1):
            lower = line.lower()
            for h in lower_hosts:
                if h in lower:
                    offenders.append((h, rel(path), i))
    return offenders


def main() -> int:
    hosts, list_hash = load_trackers()
    list_hash_short = list_hash[:16] if list_hash else "<missing>"

    if not EXPECTED_TRACKERS.exists():
        print("FAIL widget-bundle-size (missing expected-trackers.txt)")
        print(f"  Expected file: {rel(EXPECTED_TRACKERS)}")
        return 1

    if not hosts:
        print("FAIL widget-bundle-size (expected-trackers.txt has zero hostnames)")
        print(
            "  At least one canonical tracker hostname must be listed to defend "
            "DEC-FBR-02 brand promise. A genuinely empty list is forbidden — if "
            "no trackers are ever expected, the seed list is still the "
            "drift defender."
        )
        return 1

    total, per_file, over_cap = probe_a_size()
    tracker_hits = probe_b_trackers(hosts)

    fails = (1 if over_cap else 0) + (1 if tracker_hits else 0)

    header_lines = [
        f"  tracker-list hash: {list_hash} ({len(hosts)} hostnames)",
    ]

    if fails == 0:
        print("PASS widget-bundle-size")
        for ln in header_lines:
            print(ln)
        if not WIDGET_DIST.exists():
            print(
                f"  Probe A (size <= {SIZE_CAP_BYTES}B): vacuous PASS — "
                f"{rel(WIDGET_DIST)} does not exist yet (pre-build / cold-start)"
            )
            print(
                "  Probe B (no tracker hostnames): vacuous PASS — "
                "no built files to scan"
            )
        else:
            print(
                f"  Probe A (size <= {SIZE_CAP_BYTES}B): clean "
                f"({total}B used, {SIZE_CAP_BYTES - total}B headroom across "
                f"{len(per_file)} file(s))"
            )
            for f, sz in per_file:
                print(f"    {f}  {sz}B")
            print(
                f"  Probe B (no canonical tracker hostnames in {rel(WIDGET_DIST)}): clean"
            )
        return 0

    print(f"FAIL widget-bundle-size ({fails} probe(s) failed)")
    for ln in header_lines:
        print(ln)
    if over_cap:
        print()
        print(
            f"Probe A failure (bundle exceeds {SIZE_CAP_BYTES}B / 30KiB cap "
            f"per FR-FBR-04):"
        )
        print(f"  current_size={total}B  cap={SIZE_CAP_BYTES}B  over_by={total - SIZE_CAP_BYTES}B")
        for f, sz in per_file:
            print(f"    {f}  {sz}B")
        print(
            "  Remediation: drop a feature or aggressive-minify before "
            "re-running. Never silently raise SIZE_CAP_BYTES."
        )
    if tracker_hits:
        print()
        print(
            "Probe B failure (canonical third-party tracker hostname in built bundle — "
            "DEC-FBR-02 brand promise violation):"
        )
        for host, f, lineno in tracker_hits:
            print(f"  {f}:{lineno}  hostname='{host}' (canonical-tracker; not permitted in widget bundle)")
        print(
            "  Remediation: remove the offending import / script-src / fetch URL. "
            "feedbackmonk's widget calls home ONLY to feedbackmonk's own backend."
        )
    return 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(2)
