#!/usr/bin/env python3
"""Create the catalog skeleton for every locale × namespace (Contract C35). Idempotent.

  i18n/locales/<code>/<ns>.json   ns ∈ widget, public, admin, email, status
  i18n/source-hashes.json         {} on first run (filled by scripts/i18n/translate.py)

Existing files are never overwritten -- this only adds missing skeleton files, so it is safe to
re-run after a 32nd locale is added to i18n/locales.json. Python 3.8+, stdlib only.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TABLE = ROOT / "i18n" / "locales.json"
CATALOGS = ROOT / "i18n" / "locales"
HASHES = ROOT / "i18n" / "source-hashes.json"
NAMESPACES = ("widget", "public", "admin", "email", "status")


def status_for(locale: dict, default: str) -> str:
    if locale["code"] == default:
        return "source"
    if locale["deepl"] is None:
        return "english-fallback — provider unsupported"
    return "untranslated"


def main() -> int:
    data = json.loads(TABLE.read_text(encoding="utf-8"))
    created = 0
    for locale in data["locales"]:
        d = CATALOGS / locale["code"]
        d.mkdir(parents=True, exist_ok=True)
        for ns in NAMESPACES:
            p = d / f"{ns}.json"
            if p.exists():
                continue
            body = {"_meta": {"language": locale["name"], "status": status_for(locale, data["default"])}}
            p.write_text(json.dumps(body, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")
            created += 1
    if not HASHES.exists():
        HASHES.write_text(json.dumps({ns: {} for ns in NAMESPACES}, indent=2) + "\n", encoding="utf-8", newline="\n")
        created += 1
    print(f"catalog skeleton: {created} file(s) created, {len(data['locales'])} locales × {len(NAMESPACES)} namespaces")
    return 0


if __name__ == "__main__":
    sys.exit(main())
