#!/usr/bin/env python3
"""translation-gap-status Oracle -- v1.0.0 (kind: project-state, advisory).

Assertion: see manifest.json `assertion` (frozen before this probe existed).

Wraps `scripts/i18n/check-gaps.py --json` over the working tree and prints
the manifest's one-line schema. Advisory only -- exit 0 always (never a
gate; DEC-FBR-17 makes translation a deliberate owner-invoked step, and this
oracle answers "is a pass due", never "block the commit").

Python 3.8+, stdlib only.
"""
from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
CATALOGS = ROOT / "i18n" / "locales"
CHECK_GAPS_PY = ROOT / "scripts" / "i18n" / "check-gaps.py"


def _load_check_gaps():
    spec = importlib.util.spec_from_file_location("_oracle_check_gaps", CHECK_GAPS_PY)
    mod = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(mod)
    return mod


def self_test() -> int:
    """Invertible against a throwaway temp catalog tree (monkeypatches
    _catalog's path constants; never touches the real i18n/ tree)."""
    import shutil
    import tempfile

    check_gaps = _load_check_gaps()
    cat = check_gaps.cat  # scripts/i18n/_catalog, imported by check-gaps.py

    tmp = Path(tempfile.mkdtemp(prefix="translation-gap-status-selftest-"))
    ok = True

    def check(name: str, condition: bool) -> None:
        nonlocal ok
        print(f"  self-test [{name}]: {'PASS' if condition else 'FAIL'}")
        if not condition:
            ok = False

    saved = {
        "ROOT": cat.ROOT, "I18N_DIR": cat.I18N_DIR, "LOCALES_JSON": cat.LOCALES_JSON,
        "CATALOG_DIR": cat.CATALOG_DIR, "SOURCE_HASHES_JSON": cat.SOURCE_HASHES_JSON,
    }
    try:
        (tmp / "i18n" / "locales" / "en").mkdir(parents=True)
        (tmp / "i18n" / "locales" / "de").mkdir(parents=True)
        (tmp / "i18n" / "locales.json").write_text(
            '{"default":"en","overrides":{},"bare_defaults":{},"locales":['
            '{"code":"en","name":"English","dir":"ltr","deepl":"EN-US","base":null},'
            '{"code":"de","name":"Deutsch","dir":"ltr","deepl":"DE","base":null}]}',
            encoding="utf-8")
        (tmp / "i18n" / "source-hashes.json").write_text(
            '{"widget":{},"public":{},"admin":{},"email":{},"status":{}}', encoding="utf-8")
        (tmp / "i18n" / "locales" / "en" / "widget.json").write_text(
            '{"_meta":{"language":"English","status":"source"},"widget":{"a":"Hello"}}', encoding="utf-8")
        (tmp / "i18n" / "locales" / "de" / "widget.json").write_text(
            '{"_meta":{"language":"Deutsch","status":"untranslated"}}', encoding="utf-8")

        cat.ROOT = tmp
        cat.I18N_DIR = tmp / "i18n"
        cat.LOCALES_JSON = tmp / "i18n" / "locales.json"
        cat.CATALOG_DIR = tmp / "i18n" / "locales"
        cat.SOURCE_HASHES_JSON = tmp / "i18n" / "source-hashes.json"

        result = check_gaps.scan("de", "widget")
        check("missing key detected", result["missing"] == 1 and result["due"] is True)

        (tmp / "i18n" / "locales" / "de" / "widget.json").write_text(
            '{"_meta":{"language":"Deutsch","status":"untranslated"},"widget":{"a":"Hallo"}}', encoding="utf-8")
        result2 = check_gaps.scan("de", "widget")
        check("present-but-unhashed key reported drifted (never a silent false PASS)",
              result2["missing"] == 0 and result2["drifted"] == 1)
    finally:
        for name, value in saved.items():
            setattr(cat, name, value)
        shutil.rmtree(tmp, ignore_errors=True)

    print("self-test:", "PASS" if ok else "FAIL")
    return 0 if ok else 1


def main(argv) -> int:
    if "--self-test" in argv:
        return self_test()
    if not CATALOGS.exists():
        print("translation-gap-status: 0 locales | 0 missing | 0 drifted | ~0 chars | translation pass: NOT DUE (vacuous -- i18n/locales/ absent)")
        return 0
    check_gaps = _load_check_gaps()
    result = check_gaps.scan(None, None)
    check_gaps.print_report(result)
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except Exception as exc:  # noqa: BLE001 -- advisory oracle: never crash the caller
        print(f"translation-gap-status: environment failure ({exc})")
        sys.exit(2)
