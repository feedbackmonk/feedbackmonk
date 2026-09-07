import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { describe, expect, it } from "vitest";

// The ICU half of the C35 rule-5 plural truth table.
//
// `i18n/plural-fixtures.json` is the SHARED contract across the three runtimes
// that implement plural selection: the widget (this file — `Intl.PluralRules`,
// i.e. the platform's own CLDR data), the Rust renderer's `plural_category`
// (`crates/feedbackmonk-i18n/tests/plural_partition.rs`) and the catalog
// authoring partition (`scripts/i18n/tests/test_plural_partition.py`).
//
// The widget is the only one of the three that does NOT hand-roll a rule
// table, which is precisely what makes it usable here as the reference
// implementation: this suite is the DIFFERENTIAL that measures the hand-rolled
// Rust table against real CLDR, and pins the divergences that exist today so a
// new one cannot land unnoticed.

/** Walk up from the working directory to the repo's shared fixture file. */
function fixturePath(): string {
  let dir = process.cwd();
  for (let i = 0; i < 5; i++) {
    const candidate = join(dir, "i18n", "plural-fixtures.json");
    if (existsSync(candidate)) return candidate;
    dir = dirname(dir);
  }
  throw new Error("i18n/plural-fixtures.json not found above " + process.cwd());
}

type Row = {
  catalog_categories: string[];
  rust_reachable: string[];
  icu_reachable: string[];
  rust_map_sha256: string;
};

const FIXTURES = JSON.parse(readFileSync(fixturePath(), "utf8")) as {
  counts_checked: { from: number; through: number };
  locales: Record<string, Row>;
  ledger: {
    icu_partition_mismatch_locales: {
      codes: string[];
      examples: Record<string, { count: number; icu: string; rust: string }[]>;
    };
  };
};

const CODES = Object.keys(FIXTURES.locales);
const { from: FROM, through: THROUGH } = FIXTURES.counts_checked;
const ORDER = ["zero", "one", "two", "few", "many", "other"];
const sorted = (xs: Iterable<string>) => ORDER.filter((c) => new Set(xs).has(c));

/** The Rust rule table, transcribed from `catalogs.rs::plural_category`. */
function rustCategory(code: string, n: number): string {
  const n100 = n % 100;
  const n10 = n % 10;
  if (code === "lv") {
    if (n10 === 0 || (n100 >= 11 && n100 <= 19)) return "zero";
    if (n10 === 1 && n100 !== 11) return "one";
    return "other";
  }
  if (code === "ru" || code === "uk") {
    if (n10 === 1 && n100 !== 11) return "one";
    if (n10 >= 2 && n10 <= 4 && !(n100 >= 12 && n100 <= 14)) return "few";
    return "many";
  }
  if (code === "pl") {
    if (n === 1) return "one";
    if (n10 >= 2 && n10 <= 4 && !(n100 >= 12 && n100 <= 14)) return "few";
    return "many";
  }
  if (code === "cs" || code === "sk") {
    if (n === 1) return "one";
    if (n >= 2 && n <= 4) return "few";
    return "other";
  }
  return n === 1 ? "one" : "other";
}

describe("i18n/plural-fixtures.json — ICU is available and covers the table", () => {
  it("has a full-ICU Node (a language-neutral Intl would make this suite vacuous)", () => {
    // Without real CLDR data every locale collapses to English's one/other,
    // which would silently turn the differential below into a no-op.
    expect(new Intl.PluralRules("ru").select(3)).toBe("few");
    expect(new Intl.PluralRules("ja").select(1)).toBe("other");
  });

  it("covers every shipped locale", () => {
    expect(CODES.length).toBeGreaterThanOrEqual(31);
  });
});

describe("the transcribed Rust table is still faithful to the Rust source", () => {
  // `rustCategory` above is a TRANSCRIPTION of `catalogs.rs::plural_category`,
  // and a transcription is a second implementation that can drift. The Rust
  // suite pins the same digest against the real function
  // (`rust_per_count_mapping_matches_the_fixture_digest`), so agreeing with the
  // fixture here means agreeing with Rust — and a Rust rule change turns BOTH
  // suites red rather than silently invalidating the differential below.
  for (const code of CODES) {
    it(`${code}`, () => {
      const seq: string[] = [];
      for (let n = FROM; n <= THROUGH; n++) seq.push(rustCategory(code, n));
      const digest = createHash("sha256").update(seq.join(",")).digest("hex");
      expect(
        digest,
        `the table transcribed into this file no longer matches i18n/plural-fixtures.json for ` +
          `${code} — re-transcribe it from crates/feedbackmonk-i18n/src/catalogs.rs`,
      ).toBe(FIXTURES.locales[code].rust_map_sha256);
    });
  }
});

describe("Intl.PluralRules reachable categories match the fixture", () => {
  for (const code of CODES) {
    it(`${code}`, () => {
      const rules = new Intl.PluralRules(code);
      const got = new Set<string>();
      for (let n = FROM; n <= THROUGH; n++) got.add(rules.select(n));
      expect(sorted(got)).toEqual(sorted(FIXTURES.locales[code].icu_reachable));
    });
  }
});

describe("differential: the hand-rolled Rust table vs CLDR", () => {
  const pinned = new Set(FIXTURES.ledger.icu_partition_mismatch_locales.codes);

  const measured = new Set<string>();
  const firstExamples: Record<string, { count: number; icu: string; rust: string }[]> = {};
  for (const code of CODES) {
    const rules = new Intl.PluralRules(code);
    for (let n = FROM; n <= THROUGH; n++) {
      const icu = rules.select(n);
      const rust = rustCategory(code, n);
      if (icu !== rust) {
        measured.add(code);
        (firstExamples[code] ??= []).length < 4 &&
          firstExamples[code].push({ count: n, icu, rust });
      }
    }
  }

  it("no NEW locale has drifted away from CLDR", () => {
    const fresh = [...measured].filter((c) => !pinned.has(c));
    expect(
      fresh,
      `Rust's plural_category now disagrees with CLDR for ${JSON.stringify(fresh)} — ` +
        `examples: ${JSON.stringify(fresh.map((c) => firstExamples[c]))}. ` +
        `Fix the rule table rather than extending the fixture ledger.`,
    ).toEqual([]);
  });

  it("every pinned divergence is still real (the ratchet keeps its teeth)", () => {
    const healed = [...pinned].filter((c) => !measured.has(c));
    expect(
      healed,
      `${JSON.stringify(healed)} agree with CLDR now — remove them from ` +
        `ledger.icu_partition_mismatch_locales in i18n/plural-fixtures.json.`,
    ).toEqual([]);
  });

  it("the recorded examples still reproduce", () => {
    const recorded = FIXTURES.ledger.icu_partition_mismatch_locales.examples;
    for (const [code, cases] of Object.entries(recorded)) {
      const rules = new Intl.PluralRules(code);
      for (const c of cases) {
        expect(rules.select(c.count), `${code} n=${c.count} (ICU)`).toBe(c.icu);
        expect(rustCategory(code, c.count), `${code} n=${c.count} (Rust)`).toBe(c.rust);
      }
    }
  });
});
