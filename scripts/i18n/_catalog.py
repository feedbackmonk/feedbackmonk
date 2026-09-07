"""Shared catalog primitives for scripts/i18n/*.py (Contract C35).

Everything here is read-only with respect to i18n/locales.json and
i18n/locales/en/** -- those are frozen/owned-elsewhere surfaces (see the
project CLAUDE.md and i18n/README.md). This module only understands the
*shape* of the catalog tree; it has no opinion about translation quality.

Python 3.8+, stdlib only.
"""
from __future__ import annotations

import hashlib
import json
import re
from pathlib import Path
from typing import Any, Dict, Iterable, Iterator, List, Optional, Set, Tuple

ROOT = Path(__file__).resolve().parents[2]
I18N_DIR = ROOT / "i18n"
LOCALES_JSON = I18N_DIR / "locales.json"
CATALOG_DIR = I18N_DIR / "locales"
SOURCE_HASHES_JSON = I18N_DIR / "source-hashes.json"
NAMESPACES = ("widget", "public", "admin", "email", "status")

PLACEHOLDER_RE = re.compile(r"\{\{\s*(\w+)\s*\}\}")

# --------------------------------------------------------------------------
# Locale table
# --------------------------------------------------------------------------


def load_locales() -> dict:
    return json.loads(LOCALES_JSON.read_text(encoding="utf-8"))


def locale_codes() -> List[str]:
    return [l["code"] for l in load_locales()["locales"]]


def non_en_locale_codes() -> List[str]:
    return [c for c in locale_codes() if c != "en"]


def deepl_target(code: str) -> Optional[str]:
    for l in load_locales()["locales"]:
        if l["code"] == code:
            return l.get("deepl")
    raise KeyError(f"unknown locale code: {code}")


# --------------------------------------------------------------------------
# CLDR plural categories
# --------------------------------------------------------------------------
#
# Source: Unicode CLDR plural-rules chart (cldr.unicode.org/index/cldr-spec/
# plural-rules), cross-checked 2026-09-06. Three buckets beyond the universal
# "other":
#
#   EXTRA_FEW_MANY -- Slavic languages whose cardinal rule set is
#     {one, few, many, other} (e.g. Russian: n%10=1 & n%100!=11 -> one;
#     n%10 in 2..4 & n%100 not in 12..14 -> few; else many/other).
#   EXTRA_ZERO -- Latvian, whose rule set is {zero, one, other}.
#   OTHER_ONLY -- languages CLDR gives a single "other" category to (no
#     grammatical singular/plural distinction is ever selected): the
#     "Asian" family (Chinese, Japanese, Korean) plus Indonesian, Persian
#     and Turkish, all confirmed to collapse n=1 into "other" rather than
#     a distinct "one".
#
# This table matches i18n/README.md (Contract C35) rule 5 EXACTLY -- it is
# the frozen locale-category contract the i18n-catalog-integrity oracle's
# Probe D checks against. It is a deliberate simplification of true CLDR for
# three shipped locales: Irish (ga) has a real 5-category system (one, two,
# few, many, other) and Icelandic (is) / Sinhala (si) each have a genuine
# 2-category system with rules other than plain "n=1"; C35 does not ask for
# any of that, so they fall into the default {one, other} bucket here too.
# Widening this is a C35 amendment, not a change to make silently in this
# script -- see the CLAUDE-D work-log for the discovery record.
EXTRA_FEW_MANY = {"ru", "uk", "pl", "cs", "sk"}
EXTRA_ZERO = {"lv"}
OTHER_ONLY = {"ja", "ko", "zh-CN", "zh-HK", "zh-TW", "id", "fa", "tr"}


def plural_categories(code: str) -> Tuple[str, ...]:
    """The CLDR plural-category suffixes a *translated* key needs in this locale."""
    if code in OTHER_ONLY:
        return ("other",)
    if code in EXTRA_ZERO:
        return ("zero", "one", "other")
    if code in EXTRA_FEW_MANY:
        return ("one", "few", "many", "other")
    return ("one", "other")


PLURAL_SUFFIX_RE = re.compile(r"^(?P<base>.+)_(?P<category>zero|one|two|few|many|other)$")


def plural_base(dotted_key: str) -> Optional[Tuple[str, str]]:
    """Split `key_one` -> (`key`, `one`); None if `dotted_key` has no plural suffix."""
    m = PLURAL_SUFFIX_RE.match(dotted_key)
    if not m:
        return None
    return m.group("base"), m.group("category")


# --------------------------------------------------------------------------
# JSON flatten / unflatten
# --------------------------------------------------------------------------


def flatten(obj: Any, prefix: str = "") -> Dict[str, str]:
    """Flatten a nested catalog object to {dotted_key: string_value}.

    Non-string leaves (there should be none in a catalog file besides
    `_meta`) are skipped; `_meta` itself is never flattened -- callers that
    care about it read `obj["_meta"]` directly.
    """
    out: Dict[str, str] = {}
    if not isinstance(obj, dict):
        return out
    for k, v in obj.items():
        if prefix == "" and k == "_meta":
            continue
        dotted = f"{prefix}.{k}" if prefix else k
        if isinstance(v, dict):
            out.update(flatten(v, dotted))
        elif isinstance(v, str):
            out[dotted] = v
    return out


def unflatten(flat: Dict[str, str], meta: Optional[dict] = None) -> dict:
    """Inverse of flatten(): {dotted_key: value} -> nested dict, with _meta first."""
    out: dict = {}
    if meta is not None:
        out["_meta"] = meta
    for dotted, value in flat.items():
        parts = dotted.split(".")
        node = out
        for p in parts[:-1]:
            node = node.setdefault(p, {})
        node[parts[-1]] = value
    return out


# --------------------------------------------------------------------------
# Hashing / placeholders
# --------------------------------------------------------------------------


def sha256_text(value: str) -> str:
    return hashlib.sha256(value.encode("utf-8")).hexdigest()


def placeholders(value: str) -> Set[str]:
    return set(PLACEHOLDER_RE.findall(value))


# --------------------------------------------------------------------------
# Catalog file I/O -- byte-verified round-trip writes (never reformat what we
# do not intentionally change; see translate.py / complete-plurals.py).
# --------------------------------------------------------------------------


def catalog_path(code: str, namespace: str) -> Path:
    return CATALOG_DIR / code / f"{namespace}.json"


def render_json(data: dict) -> str:
    return json.dumps(data, ensure_ascii=False, indent=2) + "\n"


def load_catalog(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def load_catalog_verified(path: Path) -> Tuple[Optional[dict], str]:
    """Return (data, original_text); data is None if a no-op round trip is not
    byte-identical to what is on disk (refuse to touch a file we cannot prove
    we understand the formatting of)."""
    original = path.read_text(encoding="utf-8")
    try:
        data = json.loads(original)
    except ValueError:
        return None, original
    if render_json(data) != original:
        return None, original
    return data, original


def write_catalog_verified(path: Path, data: dict) -> None:
    """Write `data`, then immediately re-read + re-render it and byte-compare,
    refusing (raising) if the write did not round-trip cleanly."""
    rendered = render_json(data)
    path.write_text(rendered, encoding="utf-8", newline="\n")
    check = path.read_text(encoding="utf-8")
    if check != rendered:
        raise RuntimeError(f"round-trip verification failed writing {path}")
    reparsed = json.loads(check)
    if render_json(reparsed) != rendered:
        raise RuntimeError(f"re-parse round-trip verification failed writing {path}")


def iter_locale_namespace_files() -> Iterator[Tuple[str, str, Path]]:
    """Yield (locale_code, namespace, path) for every existing catalog file."""
    if not CATALOG_DIR.exists():
        return
    for code_dir in sorted(CATALOG_DIR.iterdir()):
        if not code_dir.is_dir():
            continue
        for ns in NAMESPACES:
            p = code_dir / f"{ns}.json"
            if p.exists():
                yield code_dir.name, ns, p


def load_source_hashes() -> Dict[str, Dict[str, str]]:
    if not SOURCE_HASHES_JSON.exists():
        return {ns: {} for ns in NAMESPACES}
    data = json.loads(SOURCE_HASHES_JSON.read_text(encoding="utf-8"))
    for ns in NAMESPACES:
        data.setdefault(ns, {})
    return data


def write_source_hashes(data: Dict[str, Dict[str, str]]) -> None:
    ordered = {ns: dict(sorted(data.get(ns, {}).items())) for ns in NAMESPACES}
    SOURCE_HASHES_JSON.write_text(render_json(ordered), encoding="utf-8", newline="\n")


MOJIBAKE_MARKERS = ("Ã", "â€", "Â")
HTML_ENTITY_RE = re.compile(r"&(amp|#\d+|#x[0-9a-fA-F]+);")


def has_mojibake(value: str) -> bool:
    return any(marker in value for marker in MOJIBAKE_MARKERS)


def has_leaked_entities(value: str) -> bool:
    return bool(HTML_ENTITY_RE.search(value))
