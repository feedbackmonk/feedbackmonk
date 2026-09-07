import importlib.util
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import _catalog as cat  # noqa: E402
from fixtures_lib import TempCatalogTree  # noqa: E402
from mock_deepl import MockDeepLServer  # noqa: E402
from test_translate import _EnvGuard  # noqa: E402


def _load(name: str):
    path = Path(__file__).resolve().parents[1] / f"{name}.py"
    spec = importlib.util.spec_from_file_location(f"_test_{name.replace('-', '_')}", path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


CP = _load("complete-plurals")


class TestCompletePlurals(unittest.TestCase):
    def _write_ru_missing_plurals(self):
        cat.write_catalog_verified(cat.catalog_path("en", "widget"), {
            "_meta": {"language": "English", "status": "source"},
            "widget": {"items_one": "{{count}} item", "items_other": "{{count}} items"}})
        cat.write_catalog_verified(cat.catalog_path("ru", "widget"), {
            "_meta": {"language": "Русский", "status": "machine-translated — community review welcome"},
            "widget": {"items_one": "{{count}} элемент", "items_other": "{{count}} элементов"}})

    def test_rejects_locale_without_extra_categories(self):
        with TempCatalogTree():
            self._write_ru_missing_plurals()
            with self.assertRaises(SystemExit):
                CP.build_plan("de", "widget")

    def test_dry_run_plans_few_and_many_for_russian(self):
        with TempCatalogTree():
            self._write_ru_missing_plurals()
            with _EnvGuard(DEEPL_API_KEY=None, DEEPL_BASE_URL=None):
                rc = CP.main(["--dry-run", "--locale", "ru", "--namespace", "widget"])
            self.assertEqual(rc, 0)
            plan_info = CP.build_plan("ru", "widget")
            cats_needed = {w["category"] for entry in plan_info["plan"] for w in entry["work"]}
            self.assertEqual(cats_needed, {"few", "many"})

    def test_untranslated_base_is_skipped(self):
        with TempCatalogTree():
            cat.write_catalog_verified(cat.catalog_path("en", "widget"), {
                "_meta": {"language": "English", "status": "source"},
                "widget": {"items_one": "{{count}} item", "items_other": "{{count}} items"}})
            cat.write_catalog_verified(cat.catalog_path("ru", "widget"), {
                "_meta": {"language": "Русский", "status": "untranslated"}})
            plan_info = CP.build_plan("ru", "widget")
            self.assertEqual(plan_info["plan"], [])

    def test_owner_flag_writes_few_many_via_numeral_instantiation(self):
        with TempCatalogTree():
            self._write_ru_missing_plurals()

            def fake_translate(text, target):
                # "3 item(s)" -> "3 элемент-а" (few), "5 item(s)" -> "5 элементов" (many);
                # the numeral MUST survive so complete-plurals can swap it back.
                if text.startswith("3 "):
                    return "3 элемента"
                if text.startswith("5 "):
                    return "5 элементов"
                return text

            with MockDeepLServer(translate_fn=fake_translate) as srv:
                with _EnvGuard(DEEPL_API_KEY="test-key", DEEPL_BASE_URL=srv.base_url):
                    rc = CP.main(["--locale", "ru", "--namespace", "widget", "--i-am-the-owner"])
            self.assertEqual(rc, 0)
            ru = cat.load_catalog(cat.catalog_path("ru", "widget"))
            self.assertEqual(ru["widget"]["items_few"], "{{count}} элемента")
            self.assertEqual(ru["widget"]["items_many"], "{{count}} элементов")
            self.assertEqual(ru["_meta"]["status"], "machine-translated — community review welcome")

    def test_lv_zero_category_uses_numeral_ten(self):
        with TempCatalogTree():
            cat.write_catalog_verified(cat.catalog_path("en", "widget"), {
                "_meta": {"language": "English", "status": "source"},
                "widget": {"items_one": "{{count}} item", "items_other": "{{count}} items"}})
            cat.write_catalog_verified(cat.catalog_path("lv", "widget"), {
                "_meta": {"language": "Latviešu", "status": "machine-translated — community review welcome"},
                "widget": {"items_one": "{{count}} vienums", "items_other": "{{count}} vienumi"}})

            def fake_translate(text, target):
                self.assertEqual(target, "LV")
                self.assertTrue(text.startswith("10 "))
                return "10 vienumu"

            with MockDeepLServer(translate_fn=fake_translate) as srv:
                with _EnvGuard(DEEPL_API_KEY="test-key", DEEPL_BASE_URL=srv.base_url):
                    rc = CP.main(["--locale", "lv", "--namespace", "widget", "--i-am-the-owner"])
            self.assertEqual(rc, 0)
            lv = cat.load_catalog(cat.catalog_path("lv", "widget"))
            self.assertEqual(lv["widget"]["items_zero"], "{{count}} vienumu")

    def test_numeral_lost_in_translation_refuses_that_form(self):
        with TempCatalogTree():
            self._write_ru_missing_plurals()

            def fake_translate(text, target):
                return "a sentence with no numeral in it at all"

            with MockDeepLServer(translate_fn=fake_translate) as srv:
                with _EnvGuard(DEEPL_API_KEY="test-key", DEEPL_BASE_URL=srv.base_url):
                    rc = CP.main(["--locale", "ru", "--namespace", "widget", "--i-am-the-owner"])
            self.assertEqual(rc, 1, "refused forms report as a completed-with-skips exit code")
            ru = cat.load_catalog(cat.catalog_path("ru", "widget"))
            self.assertNotIn("items_few", ru["widget"])
            self.assertNotIn("items_many", ru["widget"])

    def test_already_complete_locale_has_nothing_to_do(self):
        with TempCatalogTree():
            cat.write_catalog_verified(cat.catalog_path("en", "widget"), {
                "_meta": {"language": "English", "status": "source"},
                "widget": {"items_one": "{{count}} item", "items_other": "{{count}} items"}})
            cat.write_catalog_verified(cat.catalog_path("ru", "widget"), {
                "_meta": {"language": "Русский", "status": "machine-translated — community review welcome"},
                "widget": {"items_one": "{{count}} элемент", "items_few": "{{count}} элемента",
                           "items_many": "{{count}} элементов", "items_other": "{{count}} элементов"}})
            plan_info = CP.build_plan("ru", "widget")
            self.assertEqual(plan_info["plan"], [])


if __name__ == "__main__":
    unittest.main()
