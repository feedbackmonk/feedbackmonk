import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  LOCALE_STORAGE_KEY,
  applyDocumentLocale,
  contentLanguageOf,
  browserLocale,
  clearStoredLocale,
  dirOf,
  readQueryLocale,
  readStoredLocale,
  resolveInitialLocale,
  setLocale,
} from "./useLocale";
import { i18n } from "./index";

function setSearch(search: string) {
  window.history.replaceState(null, "", `/public/projects/p1/board${search}`);
}

function setNavigatorLanguages(langs: string[]) {
  Object.defineProperty(window.navigator, "languages", {
    value: langs,
    configurable: true,
  });
  Object.defineProperty(window.navigator, "language", {
    value: langs[0] ?? "en-US",
    configurable: true,
  });
}

describe("locale precedence (FR-FBR-36)", () => {
  beforeEach(() => {
    window.localStorage.clear();
    setSearch("");
    setNavigatorLanguages(["en-US"]);
  });

  it("?lang= wins over everything", () => {
    window.localStorage.setItem(LOCALE_STORAGE_KEY, "de");
    setNavigatorLanguages(["fr-FR"]);
    setSearch("?lang=ja");
    expect(resolveInitialLocale({ tenantLocale: "it" })).toBe("ja");
  });

  it("the stored choice wins over the tenant default and the browser", () => {
    window.localStorage.setItem(LOCALE_STORAGE_KEY, "de");
    setNavigatorLanguages(["fr-FR"]);
    expect(resolveInitialLocale({ tenantLocale: "it" })).toBe("de");
  });

  it("the tenant default wins over the browser", () => {
    setNavigatorLanguages(["fr-FR"]);
    expect(resolveInitialLocale({ tenantLocale: "it" })).toBe("it");
  });

  it("falls back to the browser preference list, resolved through C34", () => {
    setNavigatorLanguages(["da-DK", "pt-AO"]);
    expect(resolveInitialLocale()).toBe("pt-PT");
  });

  it("falls back to en when nothing resolves", () => {
    setNavigatorLanguages(["da", "nb-NO"]);
    expect(resolveInitialLocale()).toBe("en");
  });

  it("stored English is a real choice, not an absent one", () => {
    window.localStorage.setItem(LOCALE_STORAGE_KEY, "en");
    setNavigatorLanguages(["de-DE"]);
    expect(readStoredLocale()).toBe("en");
    expect(resolveInitialLocale()).toBe("en");
  });
});

describe("external inputs are validated, never echoed", () => {
  beforeEach(() => {
    window.localStorage.clear();
    setSearch("");
    setNavigatorLanguages(["de-DE"]);
  });

  it("ignores an unshipped ?lang= and keeps resolving", () => {
    setSearch("?lang=da");
    expect(readQueryLocale()).toBeNull();
    expect(resolveInitialLocale()).toBe("de");
  });

  it("ignores a hostile ?lang= value", () => {
    setSearch(`?lang=${encodeURIComponent("../../evil")}`);
    expect(readQueryLocale()).toBeNull();
    setSearch(`?lang=${encodeURIComponent('"><script>alert(1)</script>')}`);
    expect(readQueryLocale()).toBeNull();
  });

  it("ignores `constructor` on every external path (R-SEC, collab-20260907-034037)", () => {
    // `constructor` is the one Object.prototype name that survives
    // canonicalisation, and the shipped-code gate used to be `code in BY_CODE`,
    // which walks the prototype chain. All three external inputs accepted it:
    // `?lang=`, the localStorage READ path, and the tenant locale from C38.
    // Downstream it reached `formatRelative`, `Intl` threw, and with no React
    // error boundary the page rendered blank.
    setSearch("?lang=constructor");
    expect(readQueryLocale()).toBeNull();
    expect(resolveInitialLocale()).toBe("de");

    setSearch("?lang=CONSTRUCTOR");
    expect(readQueryLocale()).toBeNull();

    setSearch("");
    window.localStorage.setItem(LOCALE_STORAGE_KEY, "constructor");
    expect(readStoredLocale()).toBeNull();
    expect(resolveInitialLocale()).toBe("de");
    window.localStorage.clear();

    expect(resolveInitialLocale({ tenantLocale: "constructor" })).toBe("de");
  });

  it("ignores a junk localStorage value", () => {
    window.localStorage.setItem(LOCALE_STORAGE_KEY, "klingon");
    expect(readStoredLocale()).toBeNull();
    expect(resolveInitialLocale()).toBe("de");
  });

  it("ignores an unshipped tenant locale", () => {
    expect(resolveInitialLocale({ tenantLocale: "xx-YY" })).toBe("de");
    expect(resolveInitialLocale({ tenantLocale: null })).toBe("de");
  });

  it("survives localStorage throwing (private mode / blocked storage)", () => {
    const spy = vi
      .spyOn(Storage.prototype, "getItem")
      .mockImplementation(() => {
        throw new Error("SecurityError");
      });
    expect(readStoredLocale()).toBeNull();
    expect(resolveInitialLocale()).toBe("de");
    spy.mockRestore();
  });
});

describe("applying a locale", () => {
  beforeEach(async () => {
    window.localStorage.clear();
    setSearch("");
    await i18n.changeLanguage("en");
    applyDocumentLocale("en");
  });

  afterEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("sets <html lang> and dir, and persists the choice", async () => {
    await setLocale("de");
    expect(document.documentElement.lang).toBe("de");
    expect(document.documentElement.dir).toBe("ltr");
    expect(window.localStorage.getItem(LOCALE_STORAGE_KEY)).toBe("de");
    expect(i18n.language).toBe("de");
  });

  it("sets dir=rtl for an RTL locale", async () => {
    await setLocale("fa");
    expect(document.documentElement.dir).toBe("rtl");
    expect(dirOf("fa")).toBe("rtl");
  });

  // R-A11Y finding A-2. `fa` has no MT provider, so its catalog is English
  // PERMANENTLY -- `/1-translate` fills the other 25 and never this one. The
  // old assertion here was `lang === "fa"`, which declared Persian over English
  // words and made a screen reader read English with Persian phonology.
  // `dir` deliberately still follows the CHOSEN locale: the visitor keeps the
  // mirrored layout they asked for, announced in a voice that can pronounce
  // what is actually on screen.
  it("declares the language the words are IN, not the locale, for an english-fallback locale", async () => {
    await setLocale("fa");
    expect(document.documentElement.lang).toBe("en");
    expect(document.documentElement.dir).toBe("rtl");
    expect(i18n.language).toBe("fa");
  });

  it.each(["ga", "ml", "is", "si"])(
    "declares lang=en for english-fallback locale %s",
    async (code) => {
      await setLocale(code);
      expect(document.documentElement.lang).toBe("en");
      expect(i18n.language).toBe(code);
    },
  );

  it("still declares the locale itself when a provider covers it", async () => {
    await setLocale("de");
    expect(document.documentElement.lang).toBe("de");
    expect(contentLanguageOf("de")).toBe("de");
    expect(contentLanguageOf("pt-BR")).toBe("pt-BR");
  });

  it("contentLanguageOf falls back to en for an unknown code", () => {
    expect(contentLanguageOf("klingon")).toBe("en");
  });

  it("does not persist a default the visitor did not choose", async () => {
    await setLocale("ja", { persist: false });
    expect(i18n.language).toBe("ja");
    expect(window.localStorage.getItem(LOCALE_STORAGE_KEY)).toBeNull();
  });

  it("ignores an unshipped code entirely — no lang, no dir, no language change", async () => {
    await setLocale("de");
    await setLocale("klingon");
    expect(i18n.language).toBe("de");
    expect(document.documentElement.lang).toBe("de");
    expect(window.localStorage.getItem(LOCALE_STORAGE_KEY)).toBe("de");
  });

  it("clearStoredLocale forgets the choice", async () => {
    await setLocale("de");
    clearStoredLocale();
    expect(window.localStorage.getItem(LOCALE_STORAGE_KEY)).toBeNull();
  });

  it("browserLocale reads only the browser", () => {
    setNavigatorLanguages(["zh-Hant-CN"]);
    expect(browserLocale()).toBe("zh-TW");
  });
});
