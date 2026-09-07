import importlib.util
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import _catalog as cat  # noqa: E402
from fixtures_lib import TempCatalogTree  # noqa: E402


def _load_validate():
    path = Path(__file__).resolve().parents[1] / "validate.py"
    spec = importlib.util.spec_from_file_location("_test_validate", path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


V = _load_validate()


def classes(findings):
    return {f["class"] for f in findings}


class TestValidate(unittest.TestCase):
    def test_ok_fixture_has_no_errors(self):
        with TempCatalogTree():
            self._write_en_de_ok()
            findings = V.run("de", "widget")
            self.assertEqual([f for f in findings if f["severity"] == "error"], [])

    def _write_en_de_ok(self):
        cat.catalog_path("en", "widget").parent.mkdir(parents=True, exist_ok=True)
        cat.write_catalog_verified(cat.catalog_path("en", "widget"), {
            "_meta": {"language": "English", "status": "source"},
            "widget": {"greeting": "Hello {{name}}, welcome to the product experience today"}})
        cat.write_catalog_verified(cat.catalog_path("de", "widget"), {
            "_meta": {"language": "Deutsch", "status": "machine-translated — community review welcome"},
            "widget": {"greeting": "Hallo {{name}}, willkommen beim Produkterlebnis heute"}})

    def test_extra_key_is_error(self):
        with TempCatalogTree():
            self._write_en_de_ok()
            de = cat.load_catalog(cat.catalog_path("de", "widget"))
            de["widget"]["ghost"] = "Something not in English"
            cat.write_catalog_verified(cat.catalog_path("de", "widget"), de)
            findings = V.run("de", "widget")
            self.assertIn("extra_key", classes(findings))
            self.assertTrue(any(f["class"] == "extra_key" and f["severity"] == "error" for f in findings))

    def test_placeholder_mismatch_is_error(self):
        with TempCatalogTree():
            self._write_en_de_ok()
            de = cat.load_catalog(cat.catalog_path("de", "widget"))
            de["widget"]["greeting"] = "Hallo, willkommen beim Produkterlebnis heute"  # dropped {{name}}
            cat.write_catalog_verified(cat.catalog_path("de", "widget"), de)
            findings = V.run("de", "widget")
            self.assertIn("placeholder_mismatch", classes(findings))

    def test_empty_value_is_error(self):
        with TempCatalogTree():
            self._write_en_de_ok()
            de = cat.load_catalog(cat.catalog_path("de", "widget"))
            de["widget"]["greeting"] = "   "
            cat.write_catalog_verified(cat.catalog_path("de", "widget"), de)
            findings = V.run("de", "widget")
            self.assertIn("empty_value", classes(findings))

    def test_mojibake_is_error(self):
        with TempCatalogTree():
            self._write_en_de_ok()
            de = cat.load_catalog(cat.catalog_path("de", "widget"))
            de["widget"]["greeting"] = "Hallo, willkommen beim ProduktÃ¼bererlebnis heute"
            cat.write_catalog_verified(cat.catalog_path("de", "widget"), de)
            findings = V.run("de", "widget")
            self.assertIn("mojibake", classes(findings))

    def test_leaked_entity_is_error(self):
        with TempCatalogTree():
            self._write_en_de_ok()
            de = cat.load_catalog(cat.catalog_path("de", "widget"))
            de["widget"]["greeting"] = "Hallo {{name}} &amp; willkommen beim Produkterlebnis heute"
            cat.write_catalog_verified(cat.catalog_path("de", "widget"), de)
            findings = V.run("de", "widget")
            self.assertIn("leaked_entity", classes(findings))

    def test_untranslated_authored_is_warning_not_error(self):
        with TempCatalogTree():
            self._write_en_de_ok()
            de = cat.load_catalog(cat.catalog_path("de", "widget"))
            de["widget"]["greeting"] = "Hello {{name}}, welcome to the product experience today"
            cat.write_catalog_verified(cat.catalog_path("de", "widget"), de)
            findings = V.run("de", "widget")
            hit = [f for f in findings if f["class"] == "untranslated_authored"]
            self.assertEqual(len(hit), 1)
            self.assertEqual(hit[0]["severity"], "warning")
            self.assertEqual(V.main(["--locale", "de", "--namespace", "widget"]), 0,
                              "a warning-only tree must exit 0")

    def test_stripped_diacritics_warns_and_ratchets(self):
        with TempCatalogTree() as tree:
            cat.write_catalog_verified(cat.catalog_path("en", "widget"), {
                "_meta": {"language": "English", "status": "source"},
                "widget": {"long": "This is a genuinely long English sentence for the test"}})
            cat.write_catalog_verified(cat.catalog_path("de", "widget"), {
                "_meta": {"language": "Deutsch", "status": "machine-translated — community review welcome"},
                "widget": {"long": "Dies ist ein Test ohne jegliche Umlaute oder Akzente enthalten"}})
            findings = V.run("de", "widget")
            hits = [f for f in findings if f["class"] == "stripped_diacritics"]
            self.assertEqual(len(hits), 1)
            self.assertEqual(hits[0]["severity"], "warning")

            # freeze the baseline; the same tree must no longer report it
            rc = V.main(["--update-baseline", "--locale", "de", "--namespace", "widget"])
            self.assertEqual(rc, 0)
            findings2 = V.run("de", "widget")
            self.assertEqual([f for f in findings2 if f["class"] == "stripped_diacritics"], [])

    def test_short_value_never_flagged_for_diacritics(self):
        with TempCatalogTree():
            cat.write_catalog_verified(cat.catalog_path("en", "widget"), {
                "_meta": {"language": "English", "status": "source"},
                "widget": {"short": "Save"}})
            cat.write_catalog_verified(cat.catalog_path("de", "widget"), {
                "_meta": {"language": "Deutsch", "status": "machine-translated — community review welcome"},
                "widget": {"short": "OK"}})
            findings = V.run("de", "widget")
            self.assertEqual([f for f in findings if f["class"] == "stripped_diacritics"], [])

    def test_plural_category_gap_for_russian(self):
        with TempCatalogTree():
            cat.write_catalog_verified(cat.catalog_path("en", "widget"), {
                "_meta": {"language": "English", "status": "source"},
                "widget": {"items_one": "{{count}} item", "items_other": "{{count}} items"}})
            # ru translated _one/_other but not the required _few/_many
            cat.write_catalog_verified(cat.catalog_path("ru", "widget"), {
                "_meta": {"language": "Русский", "status": "machine-translated — community review welcome"},
                "widget": {"items_one": "{{count}} элемент", "items_other": "{{count}} элементов"}})
            findings = V.run("ru", "widget")
            gaps = [f for f in findings if f["class"] == "plural_category_gap"]
            self.assertEqual(len(gaps), 1)
            self.assertIn("few", gaps[0]["detail"])
            self.assertIn("many", gaps[0]["detail"])

    def test_plural_category_complete_for_russian_is_clean(self):
        with TempCatalogTree():
            cat.write_catalog_verified(cat.catalog_path("en", "widget"), {
                "_meta": {"language": "English", "status": "source"},
                "widget": {"items_one": "{{count}} item", "items_other": "{{count}} items"}})
            cat.write_catalog_verified(cat.catalog_path("ru", "widget"), {
                "_meta": {"language": "Русский", "status": "machine-translated — community review welcome"},
                "widget": {"items_one": "{{count}} элемент", "items_few": "{{count}} элемента",
                           "items_many": "{{count}} элементов", "items_other": "{{count}} элемента"}})
            findings = V.run("ru", "widget")
            self.assertEqual([f for f in findings if f["class"] == "plural_category_gap"], [])

    def test_untranslated_plural_base_is_not_a_gap(self):
        # a base never touched at all (no categories present in the locale)
        # is a MISSING-key concern, not a plural-completeness error.
        with TempCatalogTree():
            cat.write_catalog_verified(cat.catalog_path("en", "widget"), {
                "_meta": {"language": "English", "status": "source"},
                "widget": {"items_one": "{{count}} item", "items_other": "{{count}} items"}})
            cat.write_catalog_verified(cat.catalog_path("ru", "widget"), {
                "_meta": {"language": "Русский", "status": "untranslated"}})
            findings = V.run("ru", "widget")
            self.assertEqual([f for f in findings if f["class"] == "plural_category_gap"], [])

    def test_meta_shape_error_when_meta_missing(self):
        with TempCatalogTree():
            cat.write_catalog_verified(cat.catalog_path("en", "widget"), {
                "_meta": {"language": "English", "status": "source"}, "widget": {"a": "Hello there indeed"}})
            cat.catalog_path("de", "widget").write_text('{"widget": {"a": "Hallo"}}', encoding="utf-8")
            findings = V.run("de", "widget")
            self.assertIn("meta_shape", classes(findings))

    def test_exit_code_reflects_errors_only(self):
        with TempCatalogTree():
            self._write_en_de_ok()
            de = cat.load_catalog(cat.catalog_path("de", "widget"))
            de["widget"]["ghost"] = "extra"
            cat.write_catalog_verified(cat.catalog_path("de", "widget"), de)
            rc = V.main(["--locale", "de", "--namespace", "widget"])
            self.assertEqual(rc, 1)


if __name__ == "__main__":
    unittest.main()
