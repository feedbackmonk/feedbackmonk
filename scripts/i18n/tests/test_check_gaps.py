import importlib.util
import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import _catalog as cat  # noqa: E402
from fixtures_lib import TempCatalogTree  # noqa: E402


def _load_check_gaps():
    path = Path(__file__).resolve().parents[1] / "check-gaps.py"
    spec = importlib.util.spec_from_file_location("_test_check_gaps", path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


CG = _load_check_gaps()


class TestCheckGaps(unittest.TestCase):
    def test_missing_key_detected(self):
        with TempCatalogTree() as tree:
            tree.write("en", "widget", {"_meta": {"language": "English", "status": "source"},
                                         "widget": {"a": "Hello", "b": "World"}})
            tree.write("de", "widget", {"_meta": {"language": "Deutsch", "status": "untranslated"}})
            result = CG.scan("de", "widget")
            self.assertEqual(result["missing"], 2)
            self.assertEqual(result["drifted"], 0)
            self.assertTrue(result["due"])

    def test_drifted_key_detected_when_hash_mismatches(self):
        with TempCatalogTree() as tree:
            tree.write("en", "widget", {"_meta": {"language": "English", "status": "source"},
                                         "widget": {"a": "Hello"}})
            tree.write("de", "widget", {"_meta": {"language": "Deutsch", "status": "machine-translated — community review welcome"},
                                         "widget": {"a": "Hallo"}})
            tree.write_source_hashes({"widget": {"widget.a": cat.sha256_text("A DIFFERENT ENGLISH VALUE")}})
            result = CG.scan("de", "widget")
            self.assertEqual(result["missing"], 0)
            self.assertEqual(result["drifted"], 1)

    def test_no_drift_when_hash_matches(self):
        with TempCatalogTree() as tree:
            tree.write("en", "widget", {"_meta": {"language": "English", "status": "source"},
                                         "widget": {"a": "Hello"}})
            tree.write("de", "widget", {"_meta": {"language": "Deutsch", "status": "machine-translated — community review welcome"},
                                         "widget": {"a": "Hallo"}})
            tree.write_source_hashes({"widget": {"widget.a": cat.sha256_text("Hello")}})
            result = CG.scan("de", "widget")
            self.assertEqual(result["missing"], 0)
            self.assertEqual(result["drifted"], 0)
            self.assertFalse(result["due"])

    def test_key_with_no_baseline_hash_counts_as_drifted(self):
        # A key present in both en and the locale, but never recorded in
        # source-hashes.json (e.g. hand-added outside the pipeline), is
        # reported drifted rather than silently trusted (assertion: DRIFTED
        # = sha256(en) != source-hashes[ns][key]; absent baseline never
        # equals a real hash).
        with TempCatalogTree() as tree:
            tree.write("en", "widget", {"_meta": {"language": "English", "status": "source"},
                                         "widget": {"a": "Hello"}})
            tree.write("de", "widget", {"_meta": {"language": "Deutsch", "status": "untranslated"},
                                         "widget": {"a": "Hallo"}})
            result = CG.scan("de", "widget")
            self.assertEqual(result["drifted"], 1)

    def test_fallback_locale_excluded_from_totals_but_reported(self):
        with TempCatalogTree() as tree:
            tree.write("en", "widget", {"_meta": {"language": "English", "status": "source"},
                                         "widget": {"a": "Hello"}})
            tree.write("ga", "widget", {"_meta": {"language": "Gaeilge", "status": "english-fallback — provider unsupported"}})
            result = CG.scan("ga", "widget")
            self.assertEqual(result["missing"], 0, "fallback locales excluded from the top-line total")
            self.assertIn("ga", result["fallback_locales"])
            self.assertTrue(result["per_locale"]["ga"]["fallback_by_design"])
            self.assertEqual(result["per_locale"]["ga"]["missing"], 1, "per-locale detail still counted")

    def test_update_baseline_rewrites_from_current_en(self):
        with TempCatalogTree() as tree:
            tree.write("en", "widget", {"_meta": {"language": "English", "status": "source"},
                                         "widget": {"a": "Hello"}})
            CG.update_baseline("widget")
            hashes = cat.load_source_hashes()
            self.assertEqual(hashes["widget"]["widget.a"], cat.sha256_text("Hello"))
            # after update-baseline, drift should have cleared
            tree.write("de", "widget", {"_meta": {"language": "Deutsch", "status": "machine-translated — community review welcome"},
                                         "widget": {"a": "Hallo"}})
            result = CG.scan("de", "widget")
            self.assertEqual(result["drifted"], 0)

    def test_strict_exit_code(self):
        with TempCatalogTree() as tree:
            tree.write("en", "widget", {"_meta": {"language": "English", "status": "source"},
                                         "widget": {"a": "Hello"}})
            tree.write("de", "widget", {"_meta": {"language": "Deutsch", "status": "untranslated"}})
            rc = CG.main(["--strict", "--locale", "de", "--namespace", "widget"])
            self.assertEqual(rc, 1)
            rc0 = CG.main(["--locale", "de", "--namespace", "widget"])
            self.assertEqual(rc0, 0, "non-strict mode always exits 0")

    def test_json_output_is_valid_json(self):
        with TempCatalogTree() as tree:
            tree.write("en", "widget", {"_meta": {"language": "English", "status": "source"},
                                         "widget": {"a": "Hello"}})
            tree.write("de", "widget", {"_meta": {"language": "Deutsch", "status": "untranslated"}})
            result = CG.scan("de", "widget")
            # round-trips through json exactly like --json output would
            json.loads(json.dumps(result))


if __name__ == "__main__":
    unittest.main()
