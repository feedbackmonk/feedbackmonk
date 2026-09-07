#!/usr/bin/env python3
"""Fill CLDR plural categories a locale needs but English does not have (Contract C35 rule 5).

English only ever authors `key_one` / `key_other`. Russian, Ukrainian,
Polish, Czech and Slovak also need `key_few` / `key_many`; Latvian needs
`key_zero` (see scripts/i18n/_catalog.py's plural_categories() table, cited
from CLDR). i18next does NOT fall back to `_other` for a category it cannot
find -- it falls all the way to English -- so a translated key missing one
of these categories silently regresses to an English sentence for exactly
the counts that select it (Contract C35 rule 6).

A grammatical category cannot be requested from DeepL directly, so each
missing form is produced by INSTANTIATING the English `_other` value with a
numeral that selects the wanted category in the target language (3 -> few,
5 -> many, 10 -> Latvian zero), translating that, and swapping the numeral
back for `{{count}}`. If the numeral does not survive translation the form
is REFUSED (never written) -- a plural form with no count placeholder is
worse than the English fallback it would replace.

Shares translate.py's owner-only / dry-run / quota-preflight rules exactly
(it calls translate.py's DeepL functions directly).

Usage:
  python scripts/i18n/complete-plurals.py --dry-run
  python scripts/i18n/complete-plurals.py --i-am-the-owner
  python scripts/i18n/complete-plurals.py --i-am-the-owner --locale ru --namespace widget

Exit: 0 success; 2 quota insufficient; 3 unattended-run refusal; 4 DeepL API
error; 1 completed with one or more forms refused. Python 3.8+, stdlib only.
"""
from __future__ import annotations

import argparse
import collections
import sys
import urllib.error
from pathlib import Path
from typing import Dict, List, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent))
import _catalog as cat  # noqa: E402
import translate as tr  # noqa: E402  (no hyphen in "translate" -- a plain import works)

NUMERALS = {"few": "3", "many": "5", "zero": "10"}
PLURAL_ALL_CATEGORIES = ("zero", "one", "two", "few", "many", "other")
EXTRA_LOCALES = sorted(cat.EXTRA_FEW_MANY | cat.EXTRA_ZERO)


def extra_categories_for(locale: str) -> List[str]:
    return [c for c in cat.plural_categories(locale) if c not in ("one", "other")]


def insert_after(flat: Dict[str, str], new_key: str, value: str, after_key: Optional[str]) -> Dict[str, str]:
    items = list(flat.items())
    idx = next((i for i, (k, _) in enumerate(items) if k == after_key), None) if after_key else None
    if idx is None:
        items.append((new_key, value))
    else:
        items.insert(idx + 1, (new_key, value))
    return dict(items)


def build_plan(locale_filter: Optional[str], namespace_filter: Optional[str]) -> dict:
    locales = [locale_filter] if locale_filter else EXTRA_LOCALES
    unknown = [c for c in locales if c not in EXTRA_LOCALES]
    if unknown:
        raise SystemExit(f"not a plural-extension locale (no extra CLDR categories needed): {unknown}")
    namespaces = [namespace_filter] if namespace_filter else list(cat.NAMESPACES)

    plan = []
    total_chars = 0
    for locale in locales:
        extra_cats = extra_categories_for(locale)
        target = cat.deepl_target(locale)
        if target is None:
            continue  # english-fallback-by-design locale; never reached since these 6 all have deepl targets
        for ns in namespaces:
            en_flat = cat.flatten(cat.load_catalog(cat.catalog_path("en", ns)))
            loc_path = cat.catalog_path(locale, ns)
            if not loc_path.exists():
                continue
            loc_flat = cat.flatten(cat.load_catalog(loc_path))
            bases = {cat.plural_base(k)[0] for k in en_flat if cat.plural_base(k)}
            work = []
            for base in sorted(bases):
                other_key = f"{base}_other"
                if other_key not in en_flat:
                    continue
                present_cats = {c for c in PLURAL_ALL_CATEGORIES if f"{base}_{c}" in loc_flat}
                if not present_cats:
                    continue  # base not translated at all yet -- not this script's job
                missing = [c for c in extra_cats if f"{base}_{c}" not in loc_flat]
                if not missing:
                    continue
                en_other = en_flat[other_key]
                if cat.PLACEHOLDER_RE.search(en_other) is None:
                    continue  # no {{count}} to instantiate -- cannot synthesize, skip silently
                for category in missing:
                    numeral = NUMERALS[category]
                    instantiated = cat.PLACEHOLDER_RE.sub(numeral, en_other)
                    work.append({"base": base, "category": category, "numeral": numeral,
                                 "instantiated": instantiated})
            if work:
                chars = sum(len(w["instantiated"]) for w in work)
                total_chars += chars
                plan.append({"locale": locale, "namespace": ns, "target": target,
                              "work": work, "chars": chars})
    return {"plan": plan, "total_chars": total_chars}


def main(argv: List[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--locale", choices=EXTRA_LOCALES)
    ap.add_argument("--namespace", choices=cat.NAMESPACES)
    ap.add_argument("--formality", choices=["prefer_less", "prefer_more"], default=None)
    ap.add_argument("--i-am-the-owner", action="store_true", dest="owner")
    ap.add_argument("--key", default=None)
    args = ap.parse_args(argv)

    import os
    key = args.key or os.environ.get("DEEPL_API_KEY")
    dry_run = args.dry_run or not key
    base_url = tr.resolve_base_url(key)

    try:
        plan_info = build_plan(args.locale, args.namespace)
    except SystemExit as exc:
        print(exc)
        return 2
    plan, total_chars = plan_info["plan"], plan_info["total_chars"]

    print("PLAN")
    for entry in plan:
        print(f"  {entry['locale']:<8} {entry['namespace']:<8} {len(entry['work'])} form(s) "
              f"({entry['chars']} chars): " + ", ".join(f"{w['base']}_{w['category']}" for w in entry["work"]))
    print(f"  total characters needed: {total_chars}")

    if not plan:
        print("\nNothing to complete in scope.")
        return 0

    remaining = None
    if key:
        try:
            used, limit = tr.deepl_usage(base_url, key)
            remaining = limit - used
            print(f"  quota: {used}/{limit} used, {remaining} remaining")
        except urllib.error.URLError as exc:
            print(f"  quota check failed: {exc}")
    else:
        print("  quota: no DEEPL_API_KEY set, cannot check")

    if remaining is not None and total_chars > remaining:
        print(f"\nREFUSING: this run needs {total_chars} characters but only {remaining} remain.")
        return 2

    if dry_run:
        print("\n--dry-run: nothing called, nothing written.")
        return 0

    if not (tr._is_interactive() or args.owner):
        print("\nREFUSING: complete-plurals.py runs only on the owner's word (DEC-FBR-17). "
              "Re-run interactively, or pass --i-am-the-owner if you are the owner running this deliberately.")
        return 3

    written = 0
    refused: List[str] = []
    for entry in plan:
        locale, ns = entry["locale"], entry["namespace"]
        try:
            translations = tr.deepl_translate(base_url, key, [w["instantiated"] for w in entry["work"]],
                                               entry["target"], args.formality)
        except SystemExit as exc:
            print(exc)
            return 4
        path = cat.catalog_path(locale, ns)
        data, _ = cat.load_catalog_verified(path)
        if data is None:
            refused.append(f"{path}: round-trip verification failed, refusing to overwrite")
            continue
        flat = cat.flatten(data)
        added = 0
        for w, text in zip(entry["work"], translations):
            numeral = w["numeral"]
            if numeral not in text:
                refused.append(f"{locale}/{ns}/{w['base']}_{w['category']}: numeral {numeral} "
                                f"did not survive translation -- refused")
                continue
            value = text.replace(numeral, "{{count}}", 1)
            new_key = f"{w['base']}_{w['category']}"
            anchor = f"{w['base']}_one" if f"{w['base']}_one" in flat else None
            flat = insert_after(flat, new_key, value, anchor)
            added += 1
        if added:
            meta = dict(data.get("_meta", {}))
            meta["status"] = tr.MACHINE_STATUS
            nested = cat.unflatten(flat, meta)
            try:
                cat.write_catalog_verified(path, nested)
            except RuntimeError as exc:
                refused.append(str(exc))
                continue
            written += 1
            print(f"  {locale:<8} {ns:<8} +{added} plural form(s)")

    print(f"\nfiles written: {written}   characters spent: {total_chars}")
    if refused:
        print(f"refused ({len(refused)}):")
        for r in refused:
            print(f"  - {r}")
    return 1 if refused else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
