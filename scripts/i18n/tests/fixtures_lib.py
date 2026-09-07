"""Shared temp-catalog-tree fixture for scripts/i18n/tests/*.

Monkeypatches _catalog's path constants to a temporary directory tree for
the duration of a `with TempCatalogTree() as tree:` block, so every script
under test (which all resolve paths through _catalog's functions, never
their own hardcoded path) operates on throwaway fixture data instead of the
real i18n/ tree. Never touches the real repo tree.
"""
from __future__ import annotations

import json
import shutil
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import _catalog as cat  # noqa: E402

# A small, deliberately varied locale roster covering every plural-category
# bucket in _catalog.plural_categories() plus a deepl-null fallback locale
# and the zh-HK/zh-TW ZH-HANT sharing case.
FIXTURE_LOCALES = {
    "default": "en",
    "overrides": {},
    "bare_defaults": {},
    "locales": [
        {"code": "en", "name": "English", "dir": "ltr", "deepl": "EN-US", "base": None},
        {"code": "de", "name": "Deutsch", "dir": "ltr", "deepl": "DE", "base": None},
        {"code": "ru", "name": "Русский", "dir": "ltr", "deepl": "RU", "base": None},
        {"code": "lv", "name": "Latviešu", "dir": "ltr", "deepl": "LV", "base": None},
        {"code": "ja", "name": "日本語", "dir": "ltr", "deepl": "JA", "base": None},
        {"code": "zh-TW", "name": "繁體中文", "dir": "ltr", "deepl": "ZH-HANT", "base": None},
        {"code": "zh-HK", "name": "繁體中文（香港）", "dir": "ltr", "deepl": "ZH-HANT", "base": None},
        {"code": "ga", "name": "Gaeilge", "dir": "ltr", "deepl": None, "base": None},
    ],
}

PATCHED_ATTRS = ("ROOT", "I18N_DIR", "LOCALES_JSON", "CATALOG_DIR", "SOURCE_HASHES_JSON")


class TempCatalogTree:
    def __enter__(self) -> "TempCatalogTree":
        self.tmpdir = Path(tempfile.mkdtemp(prefix="i18n-test-"))
        (self.tmpdir / "i18n" / "locales").mkdir(parents=True)
        (self.tmpdir / "i18n" / "locales.json").write_text(
            json.dumps(FIXTURE_LOCALES, ensure_ascii=False, indent=2), encoding="utf-8")
        (self.tmpdir / "i18n" / "source-hashes.json").write_text(
            json.dumps({ns: {} for ns in cat.NAMESPACES}, indent=2), encoding="utf-8")
        for entry in FIXTURE_LOCALES["locales"]:
            (self.tmpdir / "i18n" / "locales" / entry["code"]).mkdir(parents=True)

        self._saved = {name: getattr(cat, name) for name in PATCHED_ATTRS}
        cat.ROOT = self.tmpdir
        cat.I18N_DIR = self.tmpdir / "i18n"
        cat.LOCALES_JSON = self.tmpdir / "i18n" / "locales.json"
        cat.CATALOG_DIR = self.tmpdir / "i18n" / "locales"
        cat.SOURCE_HASHES_JSON = self.tmpdir / "i18n" / "source-hashes.json"
        return self

    def __exit__(self, *exc_info) -> None:
        for name, value in self._saved.items():
            setattr(cat, name, value)
        shutil.rmtree(self.tmpdir, ignore_errors=True)

    def write(self, code: str, namespace: str, obj: dict) -> Path:
        path = cat.catalog_path(code, namespace)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(cat.render_json(obj), encoding="utf-8", newline="\n")
        return path

    def read(self, code: str, namespace: str) -> dict:
        return cat.load_catalog(cat.catalog_path(code, namespace))

    def write_source_hashes(self, data: dict) -> None:
        cat.SOURCE_HASHES_JSON.write_text(cat.render_json(data), encoding="utf-8", newline="\n")
