import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { beforeEach, describe, expect, it } from "vitest";
import {
  activeLocale,
  applyCatalog,
  canonicalise,
  contentLang,
  dir,
  hasKey,
  loadLocale,
  resolveLocale,
  resolveOne,
  setLocale,
  t,
  validateLocale,
} from "./i18n.js";

// FR-FBR-35 unit suite for the widget's runtime i18n.
//
// The resolver half runs the SHARED truth table `i18n/resolution-fixtures.json`
// (Contract C34) — the same file the SPA's `resolve.ts` suite and the Rust
// crate's test read. Two independent implementations against one fixture file
// is the cross-check; copying the cases in here would quietly break it, so the
// file is read at run time and every case is asserted.

/** Walk up from the working directory to the repo's shared fixture file. */
function fixturePath(): string {
  let dir = process.cwd();
  for (let i = 0; i < 5; i++) {
    const candidate = join(dir, "i18n", "resolution-fixtures.json");
    if (existsSync(candidate)) return candidate;
    dir = dirname(dir);
  }
  throw new Error("i18n/resolution-fixtures.json not found above " + process.cwd());
}

const FIXTURES = JSON.parse(readFileSync(fixturePath(), "utf8")) as {
  cases: { name: string; candidates: string[]; expect: string }[];
};

beforeEach(() => {
  setLocale("en");
});

describe("resolveLocale — i18n/resolution-fixtures.json (C34)", () => {
  it("has the fixture file with cases to run", () => {
    // Guards the suite against silently passing on an empty/renamed fixture.
    expect(FIXTURES.cases.length).toBeGreaterThanOrEqual(40);
  });

  for (const c of FIXTURES.cases) {
    it(`${c.name}: [${c.candidates.join(", ")}] → ${c.expect}`, () => {
      // The fixtures model ONE ordered candidate list, which is what
      // `navigator.languages` is; no explicit or html-lang candidate.
      expect(resolveLocale(c.candidates)).toBe(c.expect);
    });
  }
});

describe("resolveLocale — C36 precedence", () => {
  // The widget assembles [data-locale, <html lang>, ...navigator.languages]
  // in `widget.ts`; these are that list.
  it("prefers the explicit data-locale over the host page and the browser", () => {
    expect(resolveLocale(["fr", "de", "ja"])).toBe("fr");
  });

  it("falls to the host page's <html lang> when no explicit locale is set", () => {
    expect(resolveLocale([null, "de-AT", "ja"])).toBe("de");
  });

  it("falls to navigator.languages when the host page declares nothing", () => {
    expect(resolveLocale([null, null, "ja-JP"])).toBe("ja");
  });

  it("resolves a regional data-locale rather than discarding it", () => {
    // An embedder writing `data-locale="de-AT"` means German. Exact-only
    // validation would silently drop it to English, which reads as a bug to
    // the person who set the attribute.
    expect(resolveLocale(["de-AT"])).toBe("de");
    expect(resolveLocale(["pt"])).toBe("pt-BR");
  });

  it("falls through an UNSHIPPED explicit locale rather than forcing English", () => {
    // A host that asks for Danish and a browser that also offers Swedish
    // should get Swedish, not English: the explicit value is a preference,
    // not a veto.
    expect(resolveLocale(["da", null, "sv"])).toBe("sv");
  });

  it("never returns a code outside the table, whatever it is handed", () => {
    for (const hostile of [
      "../../evil",
      "javascript:alert(1)",
      "<script>",
      "en'; DROP TABLE",
      "https://evil.example/x.js",
    ]) {
      expect(resolveLocale([hostile])).toBe("en");
      expect(validateLocale(hostile)).toBeNull();
    }
  });

  it("rejects Object.prototype names, `constructor` included (R-SEC, collab-20260907-034037)", () => {
    // The gate used to read `code in LOADERS`, and `in` walks the prototype
    // chain. Canonicalisation mangles every other Object.prototype name
    // (`__proto__` -> `--proto--`, `toString` -> `tostring`), so exactly one
    // value survived it and passed: `constructor`, in any casing. The widget's
    // own failure was benign — LOADERS.constructor is a Function, calling it
    // yields no `default`, so it stayed English — but the SPA's identical hole
    // blank-paged a public board, and this is the sibling implementation.
    for (const hostile of ["constructor", "CONSTRUCTOR", "Constructor", "__proto__", "toString", "valueOf", "hasOwnProperty"]) {
      expect(validateLocale(hostile), `validateLocale(${hostile})`).toBeNull();
      expect(resolveLocale([hostile]), `resolveLocale([${hostile}])`).toBe("en");
      expect(resolveOne(hostile), `resolveOne(${hostile})`).toBeNull();
    }
  });

  it("hasKey/t report only OWN catalog keys, never Object.prototype names", () => {
    // Third instance of the same root cause (LD ruling: close the class).
    // `key in EN` was true for `constructor`, so hasKey claimed a key t()
    // cannot render. Unreachable through today's call sites; closed so that
    // widening `t()` to a dynamic key cannot reopen it.
    for (const name of ["constructor", "toString", "valueOf", "hasOwnProperty", "__proto__"]) {
      expect(hasKey(name), `hasKey(${name})`).toBe(false);
      expect(t(name), `t(${name})`).toBe(name); // renders the key itself, not a Function
    }
    // The real ones still work.
    expect(hasKey("widget.launcher.label")).toBe(true);
  });

  it("exposes the same helpers as the SPA resolver", () => {
    // Parity with admin-ui/src/i18n/resolve.ts (LEAD 22:42): same names, same
    // semantics, one fixture file.
    expect(canonicalise("pt_br")).toBe("pt-BR");
    expect(canonicalise("ZH-HANT")).toBe("zh-Hant");
    expect(resolveOne("de-AT")).toBe("de");
    expect(resolveOne("da")).toBeNull();
    // validateLocale is STRICTER on purpose: exact shipped codes only.
    expect(validateLocale("pt_BR")).toBe("pt-BR");
    expect(validateLocale("de-AT")).toBeNull();
    expect(validateLocale(undefined)).toBeNull();
    expect(validateLocale("")).toBeNull();
  });
});

describe("t()", () => {
  it("renders the English source string", () => {
    expect(t("widget.modal.title")).toBe("Send feedback");
  });

  it("interpolates {{name}} placeholders", () => {
    expect(t("widget.launcher.openAria", { brand: "Acme" })).toBe(
      "Open feedback form for Acme",
    );
  });

  it("interpolates the same placeholder wherever it appears", () => {
    expect(t("widget.attach.nameSize", { name: "shot.png", size: "12 KB" })).toBe(
      "shot.png (12 KB)",
    );
  });

  it("returns the key itself for an unknown key (never an empty label)", () => {
    expect(t("widget.nope.missing")).toBe("widget.nope.missing");
  });

  it("leaves an unmatched placeholder alone rather than rendering undefined", () => {
    expect(t("widget.launcher.openAria")).toBe("Open feedback form for {{brand}}");
  });

  it("selects the plural category from args.count", () => {
    expect(t("widget.attach.errorMax", { count: 1 })).toBe(
      "You can attach at most 1 screenshot.",
    );
    expect(t("widget.attach.errorMax", { count: 4 })).toBe(
      "You can attach at most 4 screenshots.",
    );
  });

  it("ignores args.count for a key that has no plural forms", () => {
    expect(t("widget.form.counter", { n: 12, max: 400 })).toBe("12 / 400");
  });
});

describe("per-key fallback to English (C35 rule 6)", () => {
  beforeEach(() => {
    setLocale("de");
    applyCatalog("de", {
      "widget.form.send": "Senden",
      "widget.modal.title": "Feedback senden",
    });
  });

  it("uses the active catalog where the key exists", () => {
    expect(t("widget.form.send")).toBe("Senden");
  });

  it("falls back per KEY, not per catalog", () => {
    expect(t("widget.form.cancel")).toBe("Cancel");
    expect(t("widget.modal.title")).toBe("Feedback senden");
  });

  it("keeps interpolation working through the fallback", () => {
    expect(t("widget.modal.description", { brand: "Acme" })).toBe(
      "Tell us what's on your mind. Submissions are sent to Acme.",
    );
  });

  it("reports hasKey for both the active catalog and the English source", () => {
    expect(hasKey("widget.form.send")).toBe(true);
    expect(hasKey("widget.form.cancel")).toBe(true);
    expect(hasKey("widget.form.nope")).toBe(false);
  });
});

describe("plural categories beyond English", () => {
  it("uses a translated extra CLDR category when the catalog has one", () => {
    setLocale("ru");
    applyCatalog("ru", {
      "widget.attach.errorMax_few": "не более {{count}} снимков.",
      "widget.attach.errorMax_many": "не более {{count}} снимков!",
    });
    // Russian: 3 → few, 5 → many.
    expect(t("widget.attach.errorMax", { count: 3 })).toBe("не более 3 снимков.");
    expect(t("widget.attach.errorMax", { count: 5 })).toBe("не более 5 снимков!");
  });

  it("falls back to the ENGLISH string when the category is untranslated", () => {
    setLocale("ru");
    applyCatalog("ru", { "widget.attach.errorMax_other": "прочее {{count}}" });
    // `few` is missing from the Russian catalog AND from English (English has
    // no `_few`), so this lands on `_other` — but on the RUSSIAN `_other`,
    // because that key does exist in the active catalog.
    expect(t("widget.attach.errorMax", { count: 3 })).toBe("прочее 3");
  });

  it("renders English when the active catalog has no form at all", () => {
    setLocale("ru");
    applyCatalog("ru", {});
    expect(t("widget.attach.errorMax", { count: 3 })).toBe(
      "You can attach at most 3 screenshots.",
    );
  });
});

describe("setLocale / loadLocale / dir", () => {
  it("defaults to English, left-to-right", () => {
    expect(activeLocale()).toBe("en");
    expect(dir()).toBe("ltr");
  });

  it("ignores an unshipped code rather than adopting it", () => {
    setLocale("da");
    expect(activeLocale()).toBe("en");
  });

  it("reports rtl for a right-to-left locale", () => {
    setLocale("fa");
    expect(activeLocale()).toBe("fa");
    expect(dir()).toBe("rtl");
  });

  // R-A11Y finding A-2. `ga, fa, ml, is, si` have no MT provider, so their
  // catalogs are English PERMANENTLY -- `/1-translate` fills the other 25 and
  // never these. `.fbm-root lang="fa"` over English words makes a screen reader
  // read English with Persian phonology. `dir()` deliberately does NOT change:
  // the visitor keeps the mirrored layout their locale asks for.
  it("contentLang reports the language the words are IN, not the active locale", () => {
    for (const code of ["ga", "fa", "ml", "is", "si"]) {
      setLocale(code);
      expect(activeLocale()).toBe(code);
      expect(contentLang()).toBe("en");
    }
    setLocale("fa");
    expect(dir()).toBe("rtl");
  });

  it("contentLang reports the locale itself when a provider covers it", () => {
    // No `if` guard: a code that stopped being shipped must FAIL here rather
    // than silently skip. (`zh-Hant` was in this list and is not a shipped
    // code, so its iteration asserted nothing — test-mod judge, A-2 record.)
    for (const code of ["de", "ja", "pt-BR", "zh-TW"]) {
      setLocale(code);
      expect(activeLocale()).toBe(code);
      expect(contentLang()).toBe(code);
    }
    setLocale("en");
    expect(contentLang()).toBe("en");
  });

  it("loads a shipped locale's chunk without throwing", async () => {
    await loadLocale("de");
    expect(activeLocale()).toBe("de");
    expect(dir()).toBe("ltr");
    // The catalog is an untranslated skeleton in this arc (DEC-FBR-17), so
    // every key still renders English — which is exactly the shippable state
    // C35 rule 6 promises.
    expect(t("widget.modal.title")).toBe("Send feedback");
  });

  it("stays English when the chunk cannot be fetched at all", async () => {
    // The self-hoster / vendoring case (LEAD ruling on MSG-002): someone
    // copies `widget.js` without the `dist/locales/` directory beside it.
    // Every chunk 404s, and the widget must still be a working English widget
    // — never raw keys, never a thrown error into the host page.
    const chunks = await import("./locale-chunks.js");
    const original = chunks.LOADERS.de;
    chunks.LOADERS.de = () => Promise.reject(new Error("404 Not Found"));
    try {
      await loadLocale("de");
      expect(activeLocale()).toBe("de");
      expect(dir()).toBe("ltr");
      expect(t("widget.modal.title")).toBe("Send feedback");
      expect(t("widget.form.send")).toBe("Send");
    } finally {
      chunks.LOADERS.de = original;
    }
  });

  it("degrades an unshipped locale to English instead of fetching anything", async () => {
    await loadLocale("da-DK");
    expect(activeLocale()).toBe("en");
    expect(t("widget.modal.title")).toBe("Send feedback");
  });

  it("resets the active catalog when the locale changes", async () => {
    setLocale("de");
    applyCatalog("de", { "widget.form.send": "Senden" });
    expect(t("widget.form.send")).toBe("Senden");
    setLocale("fr");
    expect(t("widget.form.send")).toBe("Send");
  });

  it("ignores a catalog that arrives for a locale we already left", () => {
    setLocale("de");
    setLocale("fr");
    applyCatalog("de", { "widget.form.send": "Senden" });
    expect(t("widget.form.send")).toBe("Send");
  });
});
