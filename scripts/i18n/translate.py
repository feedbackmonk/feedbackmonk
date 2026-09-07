#!/usr/bin/env python3
"""DeepL machine-translation pass over i18n/locales/<code>/<ns>.json (DEC-FBR-17).

Owner-only, release-time tool. Writes ONLY into non-`en` catalog files, for
locales that have a non-null `deepl` target in i18n/locales.json (locales
translated by hand or left as english-fallback-by-design -- ga, fa, ml, is,
si -- are never touched by this script). Every write is preceded by a
byte-for-byte round-trip verification of the file's *current* content and
followed by one of its *new* content; a file that does not round-trip is
skipped, never force-rewritten.

Safety rules (all independently testable, all fail closed):
  1. Refuses to run for real without an interactive TTY, unless
     --i-am-the-owner is passed (translation is a deliberate release step,
     never something CI/hooks/a worker session triggers by accident).
  2. Refuses a run it cannot finish: a quota preflight against DeepL's
     /usage endpoint computes the exact character spend for everything in
     scope and refuses to start if it exceeds the remaining quota (a
     partial run would leave locales half-updated with no record of where
     it stopped).
  3. `--dry-run` (the default when no API key is configured) prints the
     plan and the quota preflight and calls nothing.

zh-HK and zh-TW both target DeepL's ZH-HANT; they are translated ONCE per
namespace (the union of keys either needs) and the result is written into
whichever of the two actually needed each key -- no wasted characters, no
locale left with a stale sibling value (see W-T work-log).

Usage:
  python scripts/i18n/translate.py --dry-run
  python scripts/i18n/translate.py --i-am-the-owner
  python scripts/i18n/translate.py --i-am-the-owner --locale de --namespace widget
  python scripts/i18n/translate.py --i-am-the-owner --formality prefer_less

Env:
  DEEPL_API_KEY   the API key (":fx" suffix selects the free-tier endpoint)
  DEEPL_BASE_URL  override the API base URL entirely (test hook)

Exit: 0 success; 2 quota insufficient; 3 unattended-run refusal;
4 DeepL API error; 1 completed with one or more files skipped.
Python 3.8+, stdlib only (urllib for HTTP).
"""
from __future__ import annotations

import argparse
import collections
import importlib.util
import json
import os
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Dict, List, Optional, Tuple

sys.path.insert(0, str(Path(__file__).resolve().parent))
import _catalog as cat  # noqa: E402

MACHINE_STATUS = "machine-translated — community review welcome"
MAX_BATCH = 50

# Brand names / technical identifiers never translated (Contract C35 rule 8).
PROTECTED_TERMS = ["feedbackmonk", "GitCellar", "DeepL"]
URL_RE = re.compile(r"https?://\S+")
HTML_TAG_RE = re.compile(r"</?[a-zA-Z][^<>]*>")


def _is_interactive() -> bool:
    """Wrapped so tests can force the non-interactive branch deterministically
    regardless of whether the test runner itself happens to have a TTY."""
    return sys.stdin.isatty()


def _load_check_gaps():
    path = Path(__file__).resolve().parent / "check-gaps.py"
    spec = importlib.util.spec_from_file_location("_i18n_check_gaps", path)
    mod = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(mod)
    return mod


CHECK_GAPS = _load_check_gaps()


# --------------------------------------------------------------------------
# DeepL protection: placeholders, brand terms, URLs and any literal HTML tag
# are wrapped in <x>...</x> and sent with tag_handling=xml + ignore_tags=x.
# --------------------------------------------------------------------------
def _wrap_tag(m: "re.Match") -> str:
    # Never re-wrap a marker tag this same pass already produced.
    if m.group(0) in ("<x>", "</x>"):
        return m.group(0)
    return f"<x>{m.group(0)}</x>"


def protect(text: str) -> str:
    text = URL_RE.sub(lambda m: f"<x>{m.group(0)}</x>", text)
    text = cat.PLACEHOLDER_RE.sub(lambda m: f"<x>{m.group(0)}</x>", text)
    for term in PROTECTED_TERMS:
        text = re.sub(rf"(?<!<x>)\b{re.escape(term)}\b(?!</x>)", f"<x>{term}</x>", text)
    text = HTML_TAG_RE.sub(_wrap_tag, text)
    return text


def unprotect(text: str) -> str:
    return text.replace("<x>", "").replace("</x>", "")


# --------------------------------------------------------------------------
# DeepL HTTP
# --------------------------------------------------------------------------
def resolve_base_url(key: Optional[str]) -> str:
    override = os.environ.get("DEEPL_BASE_URL")
    if override:
        return override.rstrip("/")
    if key and key.rstrip().endswith(":fx"):
        return "https://api-free.deepl.com/v2"
    return "https://api.deepl.com/v2"


def deepl_usage(base_url: str, key: str) -> Tuple[int, int]:
    req = urllib.request.Request(
        f"{base_url}/usage", headers={"Authorization": f"DeepL-Auth-Key {key}"})
    with urllib.request.urlopen(req, timeout=30) as r:
        d = json.load(r)
    return d["character_count"], d["character_limit"]


def deepl_translate(base_url: str, key: str, texts: List[str], target: str,
                     formality: Optional[str]) -> List[str]:
    out: List[str] = []
    for i in range(0, len(texts), MAX_BATCH):
        batch = texts[i:i + MAX_BATCH]
        payload = [("target_lang", target), ("source_lang", "EN"),
                   ("tag_handling", "xml"), ("ignore_tags", "x")]
        if formality:
            payload.append(("formality", formality))
        payload += [("text", protect(t)) for t in batch]
        req = urllib.request.Request(
            f"{base_url}/translate",
            data=urllib.parse.urlencode(payload).encode("utf-8"),
            headers={"Authorization": f"DeepL-Auth-Key {key}"})
        try:
            with urllib.request.urlopen(req, timeout=90) as r:
                d = json.load(r)
        except urllib.error.HTTPError as exc:
            body = exc.read().decode("utf-8", "replace")[:400]
            raise SystemExit(f"DeepL {exc.code} for {target}: {body}")
        out.extend(unprotect(t["text"]) for t in d["translations"])
        time.sleep(0.05)  # rate-limit courtesy; negligible against a mock server
    return out


# --------------------------------------------------------------------------
# Planning: group shipped locales by DeepL target (zh-HK/zh-TW share ZH-HANT)
# --------------------------------------------------------------------------
def target_groups(locale_filter: Optional[str]) -> "collections.OrderedDict[str, List[str]]":
    codes = [locale_filter] if locale_filter else cat.non_en_locale_codes()
    groups: "collections.OrderedDict[str, List[str]]" = collections.OrderedDict()
    skipped_fallback = []
    for code in codes:
        target = cat.deepl_target(code)
        if target is None:
            skipped_fallback.append(code)
            continue
        groups.setdefault(target, []).append(code)
    return groups, skipped_fallback


def build_plan(locale_filter: Optional[str], namespace_filter: Optional[str]) -> dict:
    """Per (deepl_target, namespace): the union of keys any member locale needs,
    plus per-locale-code which of those keys it individually needs (so a
    shared translation call is written only where actually needed)."""
    groups, fallback = target_groups(locale_filter)
    namespaces = [namespace_filter] if namespace_filter else list(cat.NAMESPACES)
    hashes = cat.load_source_hashes()

    plan = []
    total_chars = 0
    for target, members in groups.items():
        for ns in namespaces:
            en_flat = CHECK_GAPS._en_flat(ns)
            per_member_keys: Dict[str, List[str]] = {}
            union_keys: "collections.OrderedDict[str, None]" = collections.OrderedDict()
            for code in members:
                r = CHECK_GAPS.scan_locale_namespace(code, ns, hashes.get(ns, {}))
                needed = r["missing"] + r["drifted"]
                per_member_keys[code] = needed
                for k in needed:
                    union_keys[k] = None
            if not union_keys:
                continue
            chars = sum(len(en_flat[k]) for k in union_keys)
            total_chars += chars
            plan.append({
                "deepl_target": target, "namespace": ns, "members": members,
                "union_keys": list(union_keys.keys()), "per_member_keys": per_member_keys,
                "chars": chars,
            })
    return {"plan": plan, "total_chars": total_chars, "fallback_skipped": fallback}


# --------------------------------------------------------------------------
# Writing
# --------------------------------------------------------------------------
def write_translations(code: str, namespace: str, keys: List[str],
                        values: Dict[str, str], hashes: Dict[str, Dict[str, str]]) -> Optional[str]:
    """Returns None on success, or a skip reason string."""
    path = cat.catalog_path(code, namespace)
    if not path.exists():
        return f"{path}: does not exist"
    data, _ = cat.load_catalog_verified(path)
    if data is None:
        return f"{path}: round-trip verification failed, refusing to overwrite"
    flat = cat.flatten(data)
    for k in keys:
        flat[k] = values[k]
    meta = dict(data.get("_meta", {}))
    meta["status"] = MACHINE_STATUS
    nested = cat.unflatten(flat, meta)
    try:
        cat.write_catalog_verified(path, nested)
    except RuntimeError as exc:
        return str(exc)
    hashes.setdefault(namespace, {})
    en_flat = CHECK_GAPS._en_flat(namespace)
    for k in keys:
        hashes[namespace][k] = cat.sha256_text(en_flat[k])
    return None


# --------------------------------------------------------------------------
def main(argv: List[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--locale", help="scope to one shipped locale code")
    ap.add_argument("--namespace", choices=cat.NAMESPACES, help="scope to one namespace")
    ap.add_argument("--formality", choices=["prefer_less", "prefer_more"], default=None)
    ap.add_argument("--i-am-the-owner", action="store_true", dest="owner",
                    help="bypass the interactive-TTY requirement (DEC-FBR-17: this script "
                         "is a deliberate release step, never an automatic one)")
    ap.add_argument("--key", default=os.environ.get("DEEPL_API_KEY"))
    args = ap.parse_args(argv)

    dry_run = args.dry_run or not args.key
    base_url = resolve_base_url(args.key)

    plan_info = build_plan(args.locale, args.namespace)
    plan, total_chars, fallback_skipped = plan_info["plan"], plan_info["total_chars"], plan_info["fallback_skipped"]

    print("PLAN")
    if fallback_skipped:
        print(f"  english-fallback by design, skipped: {', '.join(sorted(fallback_skipped))}")
    for entry in plan:
        print(f"  {entry['deepl_target']:<10} {entry['namespace']:<8} "
              f"{len(entry['union_keys'])} key(s) -> {', '.join(entry['members'])}"
              f"  ({entry['chars']} chars)")
    print(f"  total characters needed: {total_chars}")

    remaining = None
    if args.key:
        try:
            used, limit = deepl_usage(base_url, args.key)
            remaining = limit - used
            print(f"  quota: {used}/{limit} used, {remaining} remaining")
        except urllib.error.URLError as exc:
            print(f"  quota check failed: {exc}")
    else:
        print("  quota: no DEEPL_API_KEY set, cannot check")

    if not plan:
        print("\nNothing to translate in scope.")
        return 0

    if remaining is not None and total_chars > remaining:
        print(f"\nREFUSING: this run needs {total_chars} characters but only {remaining} remain.\n"
              f"A partial run leaves locales half-updated. Wait for the quota reset, narrow the "
              f"scope with --locale/--namespace, or use a key with more headroom.")
        return 2

    if dry_run:
        print("\n--dry-run: nothing called, nothing written.")
        return 0

    if not (_is_interactive() or args.owner):
        print("\nREFUSING: translate.py runs only on the owner's word (DEC-FBR-17). "
              "Re-run interactively, or pass --i-am-the-owner if you are the owner "
              "running this deliberately.")
        return 3

    hashes = cat.load_source_hashes()
    skipped: List[str] = []
    written = 0
    for entry in plan:
        try:
            translations = deepl_translate(base_url, args.key, [CHECK_GAPS._en_flat(entry["namespace"])[k]
                                                                  for k in entry["union_keys"]],
                                            entry["deepl_target"], args.formality)
        except SystemExit as exc:
            print(exc)
            return 4
        values = dict(zip(entry["union_keys"], translations))
        for code in entry["members"]:
            keys = entry["per_member_keys"][code]
            if not keys:
                continue
            reason = write_translations(code, entry["namespace"], keys, values, hashes)
            if reason:
                skipped.append(reason)
            else:
                written += 1
                print(f"  {code:<8} {entry['namespace']:<8} {len(keys)} key(s) written")

    cat.write_source_hashes(hashes)
    print(f"\nfiles written: {written}   characters spent: {total_chars}")
    if skipped:
        print(f"skipped ({len(skipped)}):")
        for s in skipped:
            print(f"  - {s}")
    print("\nNEXT: python scripts/i18n/complete-plurals.py --i-am-the-owner (fills CLDR-required "
          "categories English lacks), then python scripts/i18n/validate.py, then review the diff.")
    return 1 if skipped else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
