import importlib.util
import os
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import _catalog as cat  # noqa: E402
from fixtures_lib import TempCatalogTree  # noqa: E402
from mock_deepl import MockDeepLServer  # noqa: E402


def _load_translate():
    path = Path(__file__).resolve().parents[1] / "translate.py"
    spec = importlib.util.spec_from_file_location("_test_translate", path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


TR = _load_translate()


class _EnvGuard:
    """Set env vars for the block, restore whatever was there before."""

    def __init__(self, **kv):
        self.kv = kv
        self._saved = {}

    def __enter__(self):
        for k, v in self.kv.items():
            self._saved[k] = os.environ.get(k)
            if v is None:
                os.environ.pop(k, None)
            else:
                os.environ[k] = v
        return self

    def __exit__(self, *exc):
        for k, v in self._saved.items():
            if v is None:
                os.environ.pop(k, None)
            else:
                os.environ[k] = v


class TestTranslate(unittest.TestCase):
    def _write_missing_de(self, tree):
        cat.write_catalog_verified(cat.catalog_path("en", "widget"), {
            "_meta": {"language": "English", "status": "source"},
            "widget": {"a": "Hello {{name}}", "b": "Goodbye"}})
        cat.write_catalog_verified(cat.catalog_path("de", "widget"), {
            "_meta": {"language": "Deutsch", "status": "untranslated"}})

    def test_dry_run_default_when_no_key(self):
        with TempCatalogTree():
            self._write_missing_de(None)
            with _EnvGuard(DEEPL_API_KEY=None, DEEPL_BASE_URL=None):
                rc = TR.main(["--locale", "de", "--namespace", "widget"])
            self.assertEqual(rc, 0)
            # nothing written: still "untranslated"
            de = cat.load_catalog(cat.catalog_path("de", "widget"))
            self.assertEqual(de["_meta"]["status"], "untranslated")

    def test_null_deepl_locale_is_skipped_with_reason(self):
        with TempCatalogTree():
            cat.write_catalog_verified(cat.catalog_path("en", "widget"), {
                "_meta": {"language": "English", "status": "source"}, "widget": {"a": "Hi"}})
            cat.write_catalog_verified(cat.catalog_path("ga", "widget"), {
                "_meta": {"language": "Gaeilge", "status": "english-fallback — provider unsupported"}})
            plan_info = TR.build_plan("ga", "widget")
            self.assertEqual(plan_info["plan"], [])
            self.assertIn("ga", plan_info["fallback_skipped"])

    def test_owner_flag_refusal_without_tty(self):
        with TempCatalogTree():
            self._write_missing_de(None)
            old_interactive = TR._is_interactive
            TR._is_interactive = lambda: False  # deterministic: don't depend on the test runner's own stdin
            try:
                with MockDeepLServer(usage_response={"character_count": 0, "character_limit": 500_000}) as srv:
                    with _EnvGuard(DEEPL_API_KEY="test-key", DEEPL_BASE_URL=srv.base_url):
                        rc = TR.main(["--locale", "de", "--namespace", "widget"])
            finally:
                TR._is_interactive = old_interactive
            self.assertEqual(rc, 3)
            de = cat.load_catalog(cat.catalog_path("de", "widget"))
            self.assertEqual(de["_meta"]["status"], "untranslated", "refused run must not write")

    def test_owner_flag_bypasses_refusal_and_writes(self):
        with TempCatalogTree():
            self._write_missing_de(None)
            with MockDeepLServer(translate_fn=lambda t, target: f"[{target}] {t}") as srv:
                with _EnvGuard(DEEPL_API_KEY="test-key", DEEPL_BASE_URL=srv.base_url):
                    rc = TR.main(["--locale", "de", "--namespace", "widget", "--i-am-the-owner"])
            self.assertEqual(rc, 0)
            de = cat.load_catalog(cat.catalog_path("de", "widget"))
            self.assertEqual(de["_meta"]["status"], TR.MACHINE_STATUS)
            self.assertIn("DE", de["widget"]["a"])
            hashes = cat.load_source_hashes()
            self.assertEqual(hashes["widget"]["widget.a"], cat.sha256_text("Hello {{name}}"))

    def test_quota_refusal_when_insufficient(self):
        with TempCatalogTree():
            self._write_missing_de(None)
            with MockDeepLServer(usage_response={"character_count": 499_999, "character_limit": 500_000}) as srv:
                with _EnvGuard(DEEPL_API_KEY="test-key", DEEPL_BASE_URL=srv.base_url):
                    rc = TR.main(["--locale", "de", "--namespace", "widget", "--i-am-the-owner"])
            self.assertEqual(rc, 2)
            de = cat.load_catalog(cat.catalog_path("de", "widget"))
            self.assertEqual(de["_meta"]["status"], "untranslated", "refused-for-quota run must not write")

    def test_ignore_tags_and_placeholder_protection_sent(self):
        with TempCatalogTree():
            self._write_missing_de(None)
            with MockDeepLServer() as srv:
                with _EnvGuard(DEEPL_API_KEY="test-key", DEEPL_BASE_URL=srv.base_url):
                    rc = TR.main(["--locale", "de", "--namespace", "widget", "--i-am-the-owner"])
            self.assertEqual(rc, 0)
            self.assertTrue(srv.calls)
            call = srv.calls[0]
            self.assertEqual(call["tag_handling"], "xml")
            self.assertEqual(call["ignore_tags"], "x")
            self.assertTrue(any("<x>{{name}}</x>" in t for t in call["texts"]))

    def test_batching_respects_max_batch(self):
        with TempCatalogTree():
            en = {"_meta": {"language": "English", "status": "source"}, "widget": {}}
            for i in range(7):
                en["widget"][f"k{i}"] = f"value number {i}"
            cat.write_catalog_verified(cat.catalog_path("en", "widget"), en)
            cat.write_catalog_verified(cat.catalog_path("de", "widget"), {
                "_meta": {"language": "Deutsch", "status": "untranslated"}})
            old_max = TR.MAX_BATCH
            TR.MAX_BATCH = 3
            try:
                with MockDeepLServer() as srv:
                    with _EnvGuard(DEEPL_API_KEY="test-key", DEEPL_BASE_URL=srv.base_url):
                        rc = TR.main(["--locale", "de", "--namespace", "widget", "--i-am-the-owner"])
                self.assertEqual(rc, 0)
                sizes = [len(c["texts"]) for c in srv.calls]
                self.assertEqual(sizes, [3, 3, 1])
            finally:
                TR.MAX_BATCH = old_max

    def test_zh_hant_shared_between_zh_hk_and_zh_tw(self):
        with TempCatalogTree():
            en = {"_meta": {"language": "English", "status": "source"},
                  "widget": {"onlyTW": "Only for Taiwan", "onlyHK": "Only for Hong Kong"}}
            cat.write_catalog_verified(cat.catalog_path("en", "widget"), en)
            en_flat = cat.flatten(en)
            already_up_to_date_hashes = {k: cat.sha256_text(v) for k, v in en_flat.items()}
            cat.write_source_hashes({"widget": dict(already_up_to_date_hashes)})

            # zh-TW already has onlyHK's counterpart translated; needs onlyTW.
            cat.write_catalog_verified(cat.catalog_path("zh-TW", "widget"), {
                "_meta": {"language": "繁體中文", "status": "machine-translated — community review welcome"},
                "widget": {"onlyHK": "只適用於香港（TW）"}})
            # zh-HK needs onlyTW too (both share ZH-HANT), but not onlyHK (already has it).
            cat.write_catalog_verified(cat.catalog_path("zh-HK", "widget"), {
                "_meta": {"language": "繁體中文（香港）", "status": "machine-translated — community review welcome"},
                "widget": {"onlyHK": "只適用於香港（HK）"}})
            # de/ru/lv/ja are also in the fixture roster but out of scope for this
            # test -- give each the (already up to date) full translation so the
            # run has nothing to do for them and no "file does not exist" skip.
            for other in ("de", "ru", "lv", "ja"):
                cat.write_catalog_verified(cat.catalog_path(other, "widget"), {
                    "_meta": {"language": other, "status": "machine-translated — community review welcome"},
                    "widget": dict(en["widget"])})

            with MockDeepLServer(translate_fn=lambda t, target: f"XX-{t}") as srv:
                with _EnvGuard(DEEPL_API_KEY="test-key", DEEPL_BASE_URL=srv.base_url):
                    rc = TR.main(["--namespace", "widget", "--i-am-the-owner"])
            self.assertEqual(rc, 0)

            # exactly one DeepL call for the ZH-HANT group (namespace widget)
            zh_calls = [c for c in srv.calls if c["target_lang"] == "ZH-HANT"]
            self.assertEqual(len(zh_calls), 1)

            tw = cat.load_catalog(cat.catalog_path("zh-TW", "widget"))
            hk = cat.load_catalog(cat.catalog_path("zh-HK", "widget"))
            self.assertIn("onlyTW", tw["widget"])
            self.assertIn("onlyTW", hk["widget"])
            # each keeps its own pre-existing onlyHK value untouched (distinct
            # per member -- proves the shared call didn't cross-contaminate).
            self.assertEqual(tw["widget"]["onlyHK"], "只適用於香港（TW）")
            self.assertEqual(hk["widget"]["onlyHK"], "只適用於香港（HK）")

    def test_roundtrip_safety_refusal_skips_bad_file(self):
        with TempCatalogTree():
            cat.write_catalog_verified(cat.catalog_path("en", "widget"), {
                "_meta": {"language": "English", "status": "source"}, "widget": {"a": "Hi there"}})
            # hand-write a file with non-canonical formatting (4-space indent) so the
            # byte round-trip check fails and the write is refused.
            bad_path = cat.catalog_path("de", "widget")
            bad_path.write_text(
                '{\n    "_meta": {\n        "language": "Deutsch",\n        "status": "untranslated"\n    }\n}\n',
                encoding="utf-8")
            original = bad_path.read_text(encoding="utf-8")

            with MockDeepLServer() as srv:
                with _EnvGuard(DEEPL_API_KEY="test-key", DEEPL_BASE_URL=srv.base_url):
                    rc = TR.main(["--locale", "de", "--namespace", "widget", "--i-am-the-owner"])
            self.assertEqual(rc, 1, "completed with a skip, not a hard failure")
            self.assertEqual(bad_path.read_text(encoding="utf-8"), original, "refused file must be untouched")


if __name__ == "__main__":
    unittest.main()
