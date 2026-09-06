#!/usr/bin/env python3
"""i18n-catalog-integrity Verification Oracle -- Stage 0 skeleton.

Assertion: see manifest.json `assertion` (frozen before this probe existed).

Stage 0 implements Probe A (generated locale tables equal i18n/locales.json) and Probe B (every
catalog file parses and starts with a well-formed `_meta`). Probes C (keys subset of en +
placeholder equality), D (CLDR plural categories) and E (mojibake / entities) are implemented by
worker W-T in Stage 1 -- until then they report `not yet implemented` and do not affect the verdict.

Exit 0 PASS, 1 FAIL, 2 environment failure. Python 3.8+, stdlib only.
"""
from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
CATALOGS = ROOT / "i18n" / "locales"
GEN = ROOT / "scripts" / "i18n" / "gen-locales.py"
STATUSES = {
    "source",
    "untranslated",
    "machine-translated — community review welcome",
    "english-fallback — provider unsupported",
}


def probe_a() -> list[str]:
    if not GEN.exists():
        return [f"generator missing: {GEN.relative_to(ROOT).as_posix()}"]
    r = subprocess.run([sys.executable, str(GEN), "--check"], capture_output=True, text=True, cwd=ROOT)
    if r.returncode != 0:
        return [line for line in (r.stdout + r.stderr).splitlines() if line.strip()]
    return []


def probe_b() -> list[str]:
    fails: list[str] = []
    for path in sorted(CATALOGS.glob("*/*.json")):
        rel = path.relative_to(ROOT).as_posix()
        try:
            data = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError) as exc:
            fails.append(f"{rel}: does not parse ({exc})")
            continue
        if not isinstance(data, dict) or not data:
            fails.append(f"{rel}: not a non-empty JSON object")
            continue
        first = next(iter(data))
        if first != "_meta":
            fails.append(f"{rel}: first key must be _meta (found {first!r})")
            continue
        meta = data["_meta"]
        if not isinstance(meta, dict) or not meta.get("language") or meta.get("status") not in STATUSES:
            fails.append(f"{rel}: _meta needs language + a known status (got {meta!r})")
    return fails


def main() -> int:
    if not CATALOGS.exists():
        print("PASS i18n-catalog-integrity (vacuous -- i18n/locales/ absent)")
        return 0
    a = probe_a()
    b = probe_b()
    n_files = len(list(CATALOGS.glob("*/*.json")))
    total = len(a) + len(b)
    verdict = "PASS" if total == 0 else f"FAIL {total}"
    print(f"{verdict} i18n-catalog-integrity")
    print(f"  Probe A (generated tables == i18n/locales.json): {'PASS' if not a else 'FAIL'}")
    for line in a:
        print(f"    {line}")
    print(f"  Probe B ({n_files} catalog files parse + _meta): {'PASS' if not b else 'FAIL'}")
    for line in b:
        print(f"    {line}")
    print("  Probe C (keys ⊆ en + placeholder equality): not yet implemented (W-T, Stage 1)")
    print("  Probe D (CLDR plural categories): not yet implemented (W-T, Stage 1)")
    print("  Probe E (mojibake / entities): not yet implemented (W-T, Stage 1)")
    return 0 if total == 0 else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as exc:  # noqa: BLE001 -- environment failure is exit 2 by contract
        print(f"FAIL i18n-catalog-integrity (environment: {exc})")
        sys.exit(2)
