#!/usr/bin/env python3
"""widget-bundle-size Verification Oracle (canonical implementation).

Reports what the widget weighs, and fails on what it must never carry.

  Size (reported, never a failure): the byte size of the ENGLISH PAGE-LOAD
     SET — the TOP-LEVEL files in `widget/dist/` matching `*.{js,mjs,css}`
     (`widget.js` + `widget.css` + `redact.js`), post-terser, pre-gzip — and
     of every per-locale chunk `widget/dist/locales/*.js`, printed on every
     run so growth is visible in every check's output.
  Trackers (a failure): no canonical third-party tracker hostname appears
     anywhere in any `widget/dist/*` file — scanned RECURSIVELY, locale chunks
     included. The hostname list is hashed and printed in the report so that
     silent shrinking of the list (Q5 drift) surfaces as a visible hash change.
  Presence (a failure): `widget.js` exists, and locale chunks exist when
     non-English catalogs do.
  Chunk shape (a failure): every `widget/dist/locales/*.js` is
     `const <id> = {...}; export { <id> as default };` whose object is a flat
     map of `widget.*` keys to strings (Contract C42) — no code, no other
     namespace.

Why size is not a failure (DEC-FBR-IMPL-36, owner ruling 2026-10-10): the
30 KiB cap entered as one cell of DEC-FBR-08's scope table with no stated
reason, and a number no visitor can tell from its neighbour stalled a feature
for nothing. The page weight is judged by what it is for, with the number in
front of whoever changes it — not by a threshold. The per-chunk 4 KiB ceiling
went with it: it stood in for "a chunk is only strings", which the chunk-shape
probe now checks directly.

The list of forbidden hostnames is sourced from
`expected-trackers.txt` (newline-delimited; `#` comments stripped;
blank lines ignored). The file's SHA-256 over the canonical-serialised
list is computed and emitted with every report — drift defender.

Output: machine-parseable PASS / FAIL. Exit 0 on PASS, 1 on FAIL,
2 on environment failure.

Lineage:
- FR-FBR-04 (lightweight widget) + DEC-FBR-02 (no third-party trackers brand promise)
- DEC-FBR-IMPL-36 (size reported, not capped)
- Three-leg defense pattern (DEC-FBR-IMPL-03):
    leg 1 = vite.config.ts terser+CSP-safe bundler config
    leg 2 = THIS oracle (size report + hostname probe + list-hash drift)
    leg 3 = Playwright + axe-core a11y harness (catches behaviour regressions)
"""
from __future__ import annotations

import hashlib
import json
import re
import sys
from pathlib import Path
from typing import List, Tuple


SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parents[2]
WIDGET_DIST = REPO_ROOT / "widget" / "dist"
WIDGET_LOCALE_DIR = WIDGET_DIST / "locales"
EXPECTED_TRACKERS = SCRIPT_DIR / "expected-trackers.txt"

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


def page_load_set() -> Tuple[int, List[Tuple[str, int]]]:
    """The ENGLISH page-load set's total and per-file sizes.

    TOP-LEVEL only (`glob`, not `rglob`): `widget.js` + `widget.css` +
    `redact.js`. `dist/locales/` holds per-locale chunks that no single page
    load fetches more than one of, so they are reported on their own.
    """
    per_file: List[Tuple[str, int]] = []
    total = 0
    for path in sorted(WIDGET_DIST.glob("*")):
        if not path.is_file() or path.suffix.lower() not in BUNDLE_EXTENSIONS:
            continue
        size = path.stat().st_size
        per_file.append((rel(path), size))
        total += size
    return total, per_file


def locale_chunks() -> List[Tuple[str, int]]:
    """Each per-locale chunk's size."""
    if not WIDGET_LOCALE_DIR.exists():
        return []
    return [
        (rel(p), p.stat().st_size)
        for p in sorted(WIDGET_LOCALE_DIR.glob("*.js"))
        if p.is_file()
    ]


# The bundler's emitted form of a catalog chunk (vite lib mode, unminified):
#   const catalog = { "widget.x": "…", … };
#   export {
#     catalog as default
#   };
CHUNK_RE = re.compile(
    r"^\s*const\s+(?P<id>[A-Za-z_$][\w$]*)\s*=\s*(?P<obj>\{.*\})\s*;\s*"
    r"export\s*\{\s*(?P=id)\s+as\s+default\s*\}\s*;?\s*$",
    re.S,
)


def chunk_problem(text: str) -> str:
    """Why a locale chunk is not a flat map of widget.* keys to strings; "" when it is."""
    m = CHUNK_RE.match(text)
    if not m:
        return "not a single `const <id> = {...}; export { <id> as default }` module"
    try:
        obj = json.loads(m.group("obj"))
    except ValueError as e:
        return f"its object is not a plain string map ({e.msg})"
    if not isinstance(obj, dict):
        return "its default export is not an object"
    for key, value in obj.items():
        if not key.startswith("widget."):
            return f"key {key!r} is outside the widget.* namespace"
        if not isinstance(value, str):
            return f"key {key!r} holds a {type(value).__name__}, not a string"
    return ""


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

    # The widget exists, so its built entry point must too: a missing dist is
    # a broken commit (dist is committed and deployed from the repo), not a
    # cold start. This used to print a vacuous PASS.
    if not (WIDGET_DIST / "widget.js").is_file():
        print(f"FAIL widget-bundle-size ({rel(WIDGET_DIST)}/widget.js is missing)")
        print("  Run `npm run build` in widget/ and commit dist/.")
        return 1
    catalogs = REPO_ROOT / "i18n" / "locales"
    non_en = [d.name for d in catalogs.iterdir() if d.is_dir() and d.name != "en"] if catalogs.is_dir() else []
    if non_en and not list(WIDGET_LOCALE_DIR.glob("*.js")):
        print(f"FAIL widget-bundle-size (no {rel(WIDGET_LOCALE_DIR)}/*.js chunks, but i18n/locales has {len(non_en)} non-en locale(s))")
        print("  Run `npm run build` in widget/ and commit dist/.")
        return 1

    total, per_file = page_load_set()
    chunks = locale_chunks()
    tracker_hits = probe_b_trackers(hosts)
    bad_chunks = [
        (f, why)
        for f, why in (
            (rel(p), chunk_problem(p.read_text(encoding="utf-8")))
            for p in sorted(WIDGET_LOCALE_DIR.glob("*.js"))
        )
        if why
    ] if WIDGET_LOCALE_DIR.exists() else []

    size_lines = [f"  size: English page-load set {total}B pre-gzip across {len(per_file)} file(s)"]
    size_lines += [f"    {f}  {sz}B" for f, sz in per_file]
    if chunks:
        largest = max(chunks, key=lambda c: c[1])
        size_lines.append(
            f"  size: {len(chunks)} locale chunk(s), largest {largest[0]} at {largest[1]}B "
            f"(a visitor loads at most one)"
        )

    if not tracker_hits and not bad_chunks:
        print("PASS widget-bundle-size")
        print(f"  tracker-list hash: {list_hash} ({len(hosts)} hostnames)")
        for ln in size_lines:
            print(ln)
        print(f"  trackers: no canonical tracker hostname in {rel(WIDGET_DIST)} (recursive)")
        print(f"  chunks: every locale chunk is a flat map of widget.* strings ({len(chunks)} checked)")
        return 0

    what = [w for w, bad in (("tracker hostname in the built widget", tracker_hits),
                            ("locale chunk that is not only strings", bad_chunks)) if bad]
    print(f"FAIL widget-bundle-size ({'; '.join(what)})")
    print(f"  tracker-list hash: {list_hash} ({len(hosts)} hostnames)")
    for ln in size_lines:
        print(ln)
    if tracker_hits:
        print()
        print("Tracker failure (DEC-FBR-02 brand promise violation):")
        for host, f, lineno in tracker_hits:
            print(f"  {f}:{lineno}  hostname='{host}' (canonical-tracker; not permitted in widget bundle)")
        print(
            "  Remediation: remove the offending import / script-src / fetch URL. "
            "feedbackmonk's widget calls home ONLY to feedbackmonk's own backend."
        )
    if bad_chunks:
        print()
        print("Chunk failure (Contract C42: a locale chunk is a flat map of widget.* strings):")
        for f, why in bad_chunks:
            print(f"  {f}: {why}")
        print(
            "  Remediation: check widget/scripts/slice-locales.mjs for leaked code, a non-widget "
            "namespace or a non-string value; never hand-edit dist/."
        )
    return 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(2)
