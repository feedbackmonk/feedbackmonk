#!/usr/bin/env python3
"""Report MISSING and DRIFTED catalog keys (Contract C35; oracle `translation-gap-status`).

MISSING = a key exists in i18n/locales/en/<ns>.json and is absent from
<code>/<ns>.json. DRIFTED = the key exists in both, but the English value's
SHA-256 no longer matches the baseline recorded in i18n/source-hashes.json
when it was last translated (or was never recorded at all).

Locales whose `deepl` target is null (ga, fa, ml, is, si -- Q28 "English
fallback by design") are reported in their own bucket and excluded from the
top-line MISSING/DRIFTED counts and from --strict's exit code: they are
never going to be machine-translated, so a gap there is not "due".

Usage:
  python scripts/i18n/check-gaps.py                       # human-readable report
  python scripts/i18n/check-gaps.py --json                # machine-readable
  python scripts/i18n/check-gaps.py --locale de --namespace widget
  python scripts/i18n/check-gaps.py --strict               # exit 1 if anything is due
  python scripts/i18n/check-gaps.py --update-baseline       # rewrite source-hashes.json from current en

Exit 0 always, unless --strict and something is MISSING/DRIFTED outside the
english-fallback-by-design bucket (then 1); 2 on environment failure.
Python 3.8+, stdlib only.
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Dict, List, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent))
import _catalog as cat  # noqa: E402


def _en_flat(namespace: str) -> Dict[str, str]:
    path = cat.catalog_path("en", namespace)
    if not path.exists():
        return {}
    return cat.flatten(cat.load_catalog(path))


def _locale_flat(code: str, namespace: str) -> Dict[str, str]:
    path = cat.catalog_path(code, namespace)
    if not path.exists():
        return {}
    return cat.flatten(cat.load_catalog(path))


def scan_locale_namespace(code: str, namespace: str, hashes: Dict[str, str]) -> dict:
    en_flat = _en_flat(namespace)
    loc_flat = _locale_flat(code, namespace)
    missing: List[str] = []
    drifted: List[str] = []
    chars = 0
    for key, en_value in en_flat.items():
        if key not in loc_flat:
            missing.append(key)
            chars += len(en_value)
            continue
        baseline = hashes.get(key)
        if cat.sha256_text(en_value) != baseline:
            drifted.append(key)
            chars += len(en_value)
    return {"missing": missing, "drifted": drifted, "chars": chars}


def scan(locale_filter: Optional[str], namespace_filter: Optional[str]) -> dict:
    locales = [locale_filter] if locale_filter else cat.non_en_locale_codes()
    namespaces = [namespace_filter] if namespace_filter else list(cat.NAMESPACES)
    all_hashes = cat.load_source_hashes()

    per_locale: Dict[str, dict] = {}
    fallback_locales: List[str] = []
    total_missing = total_drifted = total_chars = 0

    for code in locales:
        is_fallback = cat.deepl_target(code) is None
        if is_fallback:
            fallback_locales.append(code)
        loc_missing = loc_drifted = loc_chars = 0
        per_ns = {}
        for ns in namespaces:
            r = scan_locale_namespace(code, ns, all_hashes.get(ns, {}))
            per_ns[ns] = {"missing": len(r["missing"]), "drifted": len(r["drifted"]),
                          "missing_keys": r["missing"], "drifted_keys": r["drifted"]}
            loc_missing += len(r["missing"])
            loc_drifted += len(r["drifted"])
            loc_chars += r["chars"]
        per_locale[code] = {
            "missing": loc_missing, "drifted": loc_drifted, "chars": loc_chars,
            "fallback_by_design": is_fallback, "namespaces": per_ns,
        }
        if not is_fallback:
            total_missing += loc_missing
            total_drifted += loc_drifted
            total_chars += loc_chars

    return {
        "locales": len(locales),
        "missing": total_missing,
        "drifted": total_drifted,
        "chars": total_chars,
        "due": (total_missing + total_drifted) > 0,
        "per_locale": per_locale,
        "fallback_locales": fallback_locales,
    }


def update_baseline(namespace_filter: Optional[str]) -> None:
    hashes = cat.load_source_hashes()
    namespaces = [namespace_filter] if namespace_filter else list(cat.NAMESPACES)
    for ns in namespaces:
        en_flat = _en_flat(ns)
        hashes.setdefault(ns, {})
        hashes[ns] = {key: cat.sha256_text(value) for key, value in en_flat.items()}
    cat.write_source_hashes(hashes)


def print_report(result: dict) -> None:
    verdict = "DUE" if result["due"] else "NOT DUE"
    print(f"translation-gap-status: {result['locales']} locales | {result['missing']} missing | "
          f"{result['drifted']} drifted | ~{result['chars']} chars | translation pass: {verdict}")
    print()
    header = f"{'locale':<8} {'missing':>7} {'drifted':>7} {'chars':>7}  note"
    print(header)
    print("-" * len(header))
    for code, r in sorted(result["per_locale"].items()):
        note = "english-fallback by design" if r["fallback_by_design"] else ""
        print(f"{code:<8} {r['missing']:>7} {r['drifted']:>7} {r['chars']:>7}  {note}")


def main(argv: List[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--locale", help="scope to one locale code")
    ap.add_argument("--namespace", choices=cat.NAMESPACES, help="scope to one namespace")
    ap.add_argument("--strict", action="store_true", help="exit 1 if anything outside the fallback bucket is due")
    ap.add_argument("--update-baseline", action="store_true",
                    help="rewrite i18n/source-hashes.json from the current en/ values (does not scan/report)")
    args = ap.parse_args(argv)

    try:
        if args.update_baseline:
            update_baseline(args.namespace)
            print("source-hashes.json updated from current en/ values"
                  + (f" (namespace={args.namespace})" if args.namespace else ""))
            return 0

        result = scan(args.locale, args.namespace)
    except (OSError, ValueError, KeyError) as exc:
        print(f"check-gaps: environment failure: {exc}", file=sys.stderr)
        return 2

    if args.json:
        print(json.dumps(result, ensure_ascii=False))
    else:
        print_report(result)

    if args.strict and result["due"]:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
