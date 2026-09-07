import { describe, expect, it } from "vitest";
import fixtures from "../../../i18n/resolution-fixtures.json";
import { canonicalise, resolveLocale, validateLocale } from "./resolve";
import { LOCALE_CODES } from "./locales.gen";

// The C34 truth table is shared with the widget resolver and the Rust resolver
// (`crates/feedbackmonk-i18n`). Adding a case here obliges all three.
describe("resolveLocale — i18n/resolution-fixtures.json (Contract C34)", () => {
  it("has fixtures to run", () => {
    expect(fixtures.cases.length).toBeGreaterThanOrEqual(40);
  });

  for (const c of fixtures.cases) {
    it(`${c.name}: [${c.candidates.join(", ")}] → ${c.expect}`, () => {
      expect(resolveLocale(c.candidates)).toBe(c.expect);
    });
  }

  it("never returns a code outside the shipped table", () => {
    for (const c of fixtures.cases) {
      expect(LOCALE_CODES).toContain(resolveLocale(c.candidates));
    }
  });
});

describe("canonicalise", () => {
  it("canonicalises casing, separators and whitespace", () => {
    expect(canonicalise("pt_br")).toBe("pt-BR");
    expect(canonicalise("ZH-HANT")).toBe("zh-Hant");
    expect(canonicalise("  De-de  ")).toBe("de-DE");
    expect(canonicalise("es-419")).toBe("es-419");
  });
});

describe("validateLocale — external input gate", () => {
  it("accepts an exact shipped code", () => {
    expect(validateLocale("de")).toBe("de");
    expect(validateLocale("pt-BR")).toBe("pt-BR");
  });

  it("canonicalises a user typo into a shipped code", () => {
    expect(validateLocale("pt_br")).toBe("pt-BR");
    expect(validateLocale(" FA ")).toBe("fa");
  });

  it("rejects anything that is not an exact shipped code", () => {
    // Deliberately stricter than resolveLocale: no falling back to a base
    // language or a bare default, because these values are attacker-reachable.
    expect(validateLocale("de-AT")).toBeNull();
    expect(validateLocale("pt")).toBeNull();
    expect(validateLocale("da")).toBeNull();
    expect(validateLocale("")).toBeNull();
    expect(validateLocale("   ")).toBeNull();
    expect(validateLocale("../../etc/passwd")).toBeNull();
    expect(validateLocale('"><script>alert(1)</script>')).toBeNull();
    expect(validateLocale(null)).toBeNull();
    expect(validateLocale(undefined)).toBeNull();
    expect(validateLocale(42)).toBeNull();
    expect(validateLocale({ code: "de" })).toBeNull();
  });

  it("rejects Object.prototype names, `constructor` included (R-SEC, collab-20260907-034037)", () => {
    // The shipped-code gate used to be `code in BY_CODE`, and `in` walks the
    // prototype chain. Canonicalisation mangles every other Object.prototype
    // name (`__proto__` -> `--proto--`, `toString` -> `tostring`), so exactly
    // one survived and passed the gate: `constructor`, in any casing. It then
    // reached `formatRelative`, where `Intl` throws a RangeError, and with no
    // React error boundary in the SPA a crafted `?lang=constructor` link
    // rendered a BLANK public board for whoever followed it.
    for (const hostile of ["constructor", "CONSTRUCTOR", "Constructor", "__proto__", "toString", "valueOf", "hasOwnProperty"]) {
      expect(validateLocale(hostile), `validateLocale(${hostile})`).toBeNull();
    }
  });

  it("resolveLocale keeps its ALWAYS-a-shipped-code contract for the same inputs", () => {
    // The gate was not the only prototype-chain read: `resolveOne`'s
    // `BARE_DEFAULTS[base]` and `TAG_OVERRIDES[prefix]` are plain index reads,
    // which walk the chain exactly as `in` does — so `resolveLocale` used to
    // hand back the `Object` constructor, and this function's docstring
    // promises callers they may pass its result straight to `<html lang>`,
    // `changeLanguage` and the catalog `import()` without re-validating.
    for (const hostile of ["constructor", "CONSTRUCTOR", "__proto__", "toString", "valueOf"]) {
      expect(resolveLocale([hostile]), `resolveLocale([${hostile}])`).toBe("en");
      expect(typeof resolveLocale([hostile]), `typeof resolveLocale([${hostile}])`).toBe("string");
    }
    // A real candidate list still resolves normally.
    expect(resolveLocale(["constructor", "de-AT"])).toBe("de");
  });
});
