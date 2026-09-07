"""The Python half of the C35 rule-5 plural truth table.

`i18n/plural-fixtures.json` is the SHARED contract across the three runtimes
that each implement plural selection: this package's catalog-authoring
partition (`_catalog.plural_categories`, which decides which `key_<category>`
suffixes a translated catalog is expected to carry), the Rust renderer's
`plural_category` (`crates/feedbackmonk-i18n/tests/plural_partition.rs`), and
the widget's `Intl.PluralRules` (`widget/src/plural-partition.test.ts`).

The three drifted before this file existed -- see the fixture's
`ledger.english_fallback_locales`.
"""
import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import _catalog as cat  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[3]
FIXTURES = REPO_ROOT / "i18n" / "plural-fixtures.json"

# CLDR's own ordering, so a set comparison never fails on order alone.
ORDER = ("zero", "one", "two", "few", "many", "other")


def _load():
    with FIXTURES.open(encoding="utf-8") as fh:
        return json.load(fh)


class PluralPartitionFixtures(unittest.TestCase):
    def setUp(self):
        self.assertTrue(
            FIXTURES.is_file(),
            f"{FIXTURES} is missing -- it is the shared contract, not a generated artifact",
        )
        self.fx = _load()

    def test_fixture_covers_every_shipped_locale(self):
        """The fixture and i18n/locales.json must agree on what ships."""
        shipped = set(cat.locale_codes())
        self.assertEqual(
            shipped,
            set(self.fx["locales"]),
            "i18n/plural-fixtures.json and i18n/locales.json disagree about what ships",
        )

    def test_catalog_categories_match_plural_categories(self):
        """`_catalog.plural_categories` IS the fixture's `catalog_categories`.

        This is the authoring side of the contract: whatever this returns is
        what `complete-plurals.py` writes and what a translator is asked to
        fill, so the renderer may only ever select from it.
        """
        mismatches = []
        for code, row in sorted(self.fx["locales"].items()):
            want = tuple(row["catalog_categories"])
            got = cat.plural_categories(code)
            if tuple(sorted(got, key=ORDER.index)) != tuple(sorted(want, key=ORDER.index)):
                mismatches.append(f"  {code:<7} fixture={want} plural_categories()={got}")
        self.assertEqual(
            [],
            mismatches,
            "_catalog.plural_categories() no longer matches i18n/plural-fixtures.json:\n"
            + "\n".join(mismatches),
        )

    def test_every_category_is_a_real_cldr_category(self):
        for code, row in sorted(self.fx["locales"].items()):
            for field in ("catalog_categories", "rust_reachable", "icu_reachable"):
                for c in row[field]:
                    self.assertIn(c, ORDER, f"{code}.{field} has non-CLDR category {c!r}")

    def test_english_fallback_ledger_is_exactly_the_measured_divergence(self):
        """The ratchet, from the authoring side.

        A locale diverges when the renderer can select a category this side
        never writes -- the lookup then misses the target catalog and the
        reader gets English. The ledger pins the six locales measured on
        2026-09-07 so a seventh cannot appear silently.
        """
        pinned = set(self.fx["ledger"]["english_fallback_locales"]["codes"])
        measured = set()
        for code, row in self.fx["locales"].items():
            ships = set(cat.plural_categories(code))
            if not set(row["rust_reachable"]) <= ships:
                measured.add(code)

        self.assertEqual(
            set(),
            measured - pinned,
            f"NEW English-fallback divergence in {sorted(measured - pinned)}: the renderer selects a "
            "category complete-plurals.py never writes, so those counts render English. Fix one of "
            "the two partitions rather than extending the ledger.",
        )
        self.assertEqual(
            set(),
            pinned - measured,
            f"{sorted(pinned - measured)} no longer diverge -- remove them from "
            "ledger.english_fallback_locales.codes so the ratchet keeps its teeth.",
        )


if __name__ == "__main__":
    unittest.main()
