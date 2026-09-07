"""Exercises the static fixture catalogs under tests/fixtures/{ok,placeholder-mismatch,
drift,mojibake,plural-missing}/ directly (as opposed to the other test modules'
in-memory temp trees) -- reviewable-by-a-human catalog samples for each of the
nine validate.py defect classes / check-gaps.py's drift detection.
"""
import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import _catalog as cat  # noqa: E402

FIXTURES_ROOT = Path(__file__).resolve().parent / "fixtures"
PATCHED_ATTRS = ("ROOT", "I18N_DIR", "LOCALES_JSON", "CATALOG_DIR", "SOURCE_HASHES_JSON")


def _load(name: str):
    path = Path(__file__).resolve().parents[1] / f"{name}.py"
    spec = importlib.util.spec_from_file_location(f"_staticfix_{name.replace('-', '_')}", path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


VALIDATE = _load("validate")
CHECK_GAPS = _load("check-gaps")


class _FixtureDir:
    """Point _catalog at one static fixtures/<scenario>/ directory (its
    subdirectories ARE the locale dirs). A source-hashes.json inside the
    scenario dir is used if present, else an empty one."""

    def __init__(self, scenario: str):
        self.scenario_dir = FIXTURES_ROOT / scenario

    def __enter__(self):
        self._saved = {name: getattr(cat, name) for name in PATCHED_ATTRS}
        cat.ROOT = FIXTURES_ROOT
        cat.I18N_DIR = FIXTURES_ROOT
        cat.LOCALES_JSON = FIXTURES_ROOT / "locales.json"
        cat.CATALOG_DIR = self.scenario_dir
        own_hashes = self.scenario_dir / "source-hashes.json"
        if own_hashes.exists():
            cat.SOURCE_HASHES_JSON = own_hashes
        else:
            # scenarios with no drift baseline of their own get a throwaway
            # empty one -- never written into the checked-in fixtures tree.
            self._tmpdir = tempfile.TemporaryDirectory(prefix="i18n-fixture-hashes-")
            empty = Path(self._tmpdir.name) / "source-hashes.json"
            empty.write_text(cat.render_json({ns: {} for ns in cat.NAMESPACES}), encoding="utf-8", newline="\n")
            cat.SOURCE_HASHES_JSON = empty
        return self

    def __exit__(self, *exc_info) -> None:
        for name, value in self._saved.items():
            setattr(cat, name, value)
        if hasattr(self, "_tmpdir"):
            self._tmpdir.cleanup()


class TestStaticFixtures(unittest.TestCase):
    def test_ok_has_no_findings_at_all(self):
        with _FixtureDir("ok"):
            findings = VALIDATE.run("de", "widget")
            self.assertEqual(findings, [])

    def test_placeholder_mismatch_fixture_flags_it(self):
        with _FixtureDir("placeholder-mismatch"):
            findings = VALIDATE.run("de", "widget")
            classes = {f["class"] for f in findings}
            self.assertIn("placeholder_mismatch", classes)

    def test_drift_fixture_is_detected_by_check_gaps(self):
        with _FixtureDir("drift"):
            result = CHECK_GAPS.scan("de", "widget")
            self.assertEqual(result["drifted"], 1)
            self.assertEqual(result["missing"], 0)

    def test_mojibake_fixture_flags_it(self):
        with _FixtureDir("mojibake"):
            findings = VALIDATE.run("de", "widget")
            classes = {f["class"] for f in findings}
            self.assertIn("mojibake", classes)

    def test_plural_missing_fixture_flags_gap_for_russian(self):
        with _FixtureDir("plural-missing"):
            findings = VALIDATE.run("ru", "widget")
            gaps = [f for f in findings if f["class"] == "plural_category_gap"]
            self.assertEqual(len(gaps), 1)
            self.assertIn("few", gaps[0]["detail"])
            self.assertIn("many", gaps[0]["detail"])


if __name__ == "__main__":
    unittest.main()
