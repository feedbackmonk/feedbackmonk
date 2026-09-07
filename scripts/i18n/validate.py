#!/usr/bin/env python3
"""Structural + quality validation of i18n/locales/<code>/<ns>.json (Contract C35).

Nine defect classes (the ones GitCellar's translation estate learned the hard
way -- see the ported reference scripts in ../GitCellar/scripts/i18n/):

  1. missing key           (info)    -- present in en, absent from the locale
  2. extra key              (error)   -- present in the locale, absent from en
  3. placeholder mismatch   (error)   -- {{name}} tokens differ from en's for a shared key
  4. empty value            (error)   -- a translated value is blank
  5. mojibake               (error)   -- Ã / â€ / Â byte-corruption sequences
  6. leaked HTML entities   (error)   -- literal &amp; / &#NN; that should have decoded
  7. untranslated-authored  (warning) -- value is byte-identical to en's (never translated)
  8. stripped diacritics    (warning) -- a locale that normally accents has zero non-ASCII
                                         letters in a >=20-char value; ratcheted against
                                         i18n/validate-baseline.json so already-accepted
                                         values (short loanwords, brand-heavy strings) do
                                         not re-warn on every run
  9. plural-category gap    (error)   -- a base key is translated (has >=1 plural suffix
                                         present) but is missing a category this locale's
                                         CLDR rule set requires -- see _catalog.plural_categories

Exit 0 if no errors (warnings do not fail the run); 1 if any error; 2 on
environment failure. `--json` for machine-readable output.

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

DIACRITIC_LOCALES = {"fr", "de", "es", "pt-BR", "pt-PT", "cs", "sk", "pl", "hu", "tr", "lv", "is"}
DIACRITIC_MIN_LEN = 20
PLURAL_ALL_CATEGORIES = ("zero", "one", "two", "few", "many", "other")


def _baseline_path() -> Path:
    # Computed at call time (not a module-level constant) so tests that
    # monkeypatch cat.I18N_DIR to a temp tree are honoured.
    return cat.I18N_DIR / "validate-baseline.json"


def _load_baseline() -> set:
    path = _baseline_path()
    if not path.exists():
        return set()
    data = json.loads(path.read_text(encoding="utf-8"))
    return set(data.get("stripped_diacritics", []))


def _write_baseline(entries: set) -> None:
    data = {"stripped_diacritics": sorted(entries)}
    _baseline_path().write_text(cat.render_json(data), encoding="utf-8", newline="\n")


def _finding(cls: str, severity: str, locale: str, namespace: str, key: str, detail: str) -> dict:
    return {"class": cls, "severity": severity, "locale": locale, "namespace": namespace,
            "key": key, "detail": detail}


def validate_meta(data: dict, locale: str, namespace: str) -> List[dict]:
    findings = []
    if not isinstance(data, dict) or not data:
        return [_finding("meta_shape", "error", locale, namespace, "_meta", "file is not a non-empty JSON object")]
    first = next(iter(data))
    if first != "_meta":
        return [_finding("meta_shape", "error", locale, namespace, "_meta", f"first key must be _meta (found {first!r})")]
    meta = data["_meta"]
    if not isinstance(meta, dict) or not meta.get("language"):
        findings.append(_finding("meta_shape", "error", locale, namespace, "_meta", f"missing language: {meta!r}"))
    return findings


def validate_locale_namespace(locale: str, namespace: str, baseline: set) -> List[dict]:
    findings: List[dict] = []
    en_path = cat.catalog_path("en", namespace)
    loc_path = cat.catalog_path(locale, namespace)
    if not en_path.exists() or not loc_path.exists():
        return findings
    en_data = cat.load_catalog(en_path)
    loc_data = cat.load_catalog(loc_path)
    findings += validate_meta(loc_data, locale, namespace)

    en_flat = cat.flatten(en_data)
    loc_flat = cat.flatten(loc_data)

    for key, en_value in en_flat.items():
        if key not in loc_flat:
            findings.append(_finding("missing_key", "info", locale, namespace, key, "absent from this locale"))

    for key, loc_value in loc_flat.items():
        if key not in en_flat:
            findings.append(_finding("extra_key", "error", locale, namespace, key, "not present in en"))
            continue

        en_value = en_flat[key]
        en_ph, loc_ph = cat.placeholders(en_value), cat.placeholders(loc_value)
        if en_ph != loc_ph:
            findings.append(_finding("placeholder_mismatch", "error", locale, namespace, key,
                                      f"en has {sorted(en_ph)}, locale has {sorted(loc_ph)}"))

        if loc_value.strip() == "":
            findings.append(_finding("empty_value", "error", locale, namespace, key, "blank value"))
            continue

        if cat.has_mojibake(loc_value):
            findings.append(_finding("mojibake", "error", locale, namespace, key, "mojibake byte sequence"))

        if cat.has_leaked_entities(loc_value):
            findings.append(_finding("leaked_entity", "error", locale, namespace, key, "leaked HTML entity"))

        if loc_value == en_value:
            findings.append(_finding("untranslated_authored", "warning", locale, namespace, key,
                                      "identical to en value"))

        if locale in DIACRITIC_LOCALES and len(en_value) >= DIACRITIC_MIN_LEN:
            has_non_ascii_letter = any(ord(c) > 127 and c.isalpha() for c in loc_value)
            if not has_non_ascii_letter:
                baseline_id = f"{locale}:{namespace}:{key}"
                if baseline_id not in baseline:
                    findings.append(_finding("stripped_diacritics", "warning", locale, namespace, key,
                                              "zero non-ASCII letters in a normally-accented locale"))

    # Plural-category completeness (class 9)
    required = cat.plural_categories(locale)
    bases_in_en = set()
    for key in en_flat:
        pb = cat.plural_base(key)
        if pb:
            bases_in_en.add(pb[0])
    for base in sorted(bases_in_en):
        present_cats = {c for c in PLURAL_ALL_CATEGORIES if f"{base}_{c}" in loc_flat}
        if not present_cats:
            continue  # entirely untranslated base -- not this class's concern
        missing = [c for c in required if f"{base}_{c}" not in loc_flat]
        if missing:
            findings.append(_finding("plural_category_gap", "error", locale, namespace, base,
                                      f"missing required categories {missing} for this locale"))

    return findings


def run(locale_filter: Optional[str], namespace_filter: Optional[str]) -> List[dict]:
    baseline = _load_baseline()
    locales = [locale_filter] if locale_filter else cat.non_en_locale_codes()
    namespaces = [namespace_filter] if namespace_filter else list(cat.NAMESPACES)
    findings: List[dict] = []
    for code in locales:
        for ns in namespaces:
            findings += validate_locale_namespace(code, ns, baseline)
    return findings


def current_diacritic_ids(locale_filter: Optional[str], namespace_filter: Optional[str]) -> set:
    findings = run(locale_filter, namespace_filter)
    return {f"{f['locale']}:{f['namespace']}:{f['key']}" for f in findings if f["class"] == "stripped_diacritics"}


def print_report(findings: List[dict]) -> None:
    by_class: Dict[str, List[dict]] = {}
    for f in findings:
        by_class.setdefault(f["class"], []).append(f)
    errors = [f for f in findings if f["severity"] == "error"]
    warnings = [f for f in findings if f["severity"] == "warning"]
    infos = [f for f in findings if f["severity"] == "info"]
    print(f"validate: {len(errors)} error(s), {len(warnings)} warning(s), {len(infos)} info")
    for cls, items in sorted(by_class.items()):
        print(f"\n  [{items[0]['severity']}] {cls}: {len(items)}")
        for f in items[:20]:
            print(f"    {f['locale']}/{f['namespace']}.json  {f['key']}  -- {f['detail']}")
        if len(items) > 20:
            print(f"    ... and {len(items) - 20} more")


def main(argv: List[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--locale", help="scope to one locale code")
    ap.add_argument("--namespace", choices=cat.NAMESPACES, help="scope to one namespace")
    ap.add_argument("--update-baseline", action="store_true",
                    help="rewrite i18n/validate-baseline.json to exactly the stripped-diacritics "
                         "findings currently present (review the diff before committing -- this "
                         "silences today's findings, it does not judge them)")
    args = ap.parse_args(argv)

    try:
        if args.update_baseline:
            ids = current_diacritic_ids(args.locale, args.namespace)
            _write_baseline(ids)
            print(f"validate-baseline.json updated: {len(ids)} accepted stripped-diacritics entries")
            return 0
        findings = run(args.locale, args.namespace)
    except (OSError, ValueError, KeyError) as exc:
        print(f"validate: environment failure: {exc}", file=sys.stderr)
        return 2

    if args.json:
        print(json.dumps(findings, ensure_ascii=False))
    else:
        print_report(findings)

    return 1 if any(f["severity"] == "error" for f in findings) else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
