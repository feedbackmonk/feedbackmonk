import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { test, expect, type Page, type Route } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

// FR-FBR-35 end-to-end suite: the locale matrix, the CSP fixture (TGF-…-01)
// and the RTL guards (TGF-…-02).
//
// What this suite exists to catch that the unit tests cannot:
//   - the lazy catalog chunk is actually REQUESTED over the network, from the
//     right URL, under a real embedder's CSP (a widget that silently falls
//     back to English looks identical to one that worked);
//   - `lang` / `dir` land on the real `.fbm-root` element in a real browser;
//   - the three-level precedence (embed → host page → browser) behaves in a
//     browser that supplies its own `navigator.languages`;
//   - axe stays clean in all three locales, including the RTL one.

const MOCK_CONFIG = {
  project_id: "00000000-0000-0000-0000-000000000001",
  tenant_id: "00000000-0000-0000-0000-000000000002",
  display_name: "Fixture Project",
  brand: {
    primary_color: "#2563eb",
    logo_url: null,
    footer_text: "powered by feedbackmonk",
    footer_url: null,
    theme: null,
  },
  auth_modes: ["auth", "anonymous"],
  submission_kinds: ["bug", "feature", "question", "other"],
  max_body_chars: 16384,
};

async function installMocks(page: Page) {
  await page.route("**/widget-config", async (route: Route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(MOCK_CONFIG),
    });
  });
}

/** Locale-chunk requests the page made, e.g. ["de"]. */
function trackChunks(page: Page): string[] {
  const seen: string[] = [];
  page.on("request", (req) => {
    const m = /\/dist\/locales\/([A-Za-z-]+)\.js(\?|$)/.exec(req.url());
    if (m) seen.push(m[1]);
  });
  return seen;
}

async function expectNoAxeViolations(page: Page, label: string, scope?: string) {
  let builder = new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa"]);
  if (scope) builder = builder.include(scope);
  const results = await builder.analyze();
  expect(results.violations, `axe violations on ${label}`).toEqual([]);
}

// --------------------------------------------------------------------------
// The matrix: browser language decides (fixture declares no lang, no attribute)
// --------------------------------------------------------------------------

// `lang` is the language of the WORDS, which is not always `code` (R-A11Y A-2):
// `fa` has no MT provider, so its catalog is English permanently. `dir` and the
// fetched chunk still follow the resolved locale.
const MATRIX = [
  { browser: "en-US", code: "en", lang: "en", dir: "ltr", chunk: null },
  { browser: "de-DE", code: "de", lang: "de", dir: "ltr", chunk: "de" },
  { browser: "fa-IR", code: "fa", lang: "en", dir: "rtl", chunk: "fa" },
] as const;

for (const row of MATRIX) {
  test.describe(`locale matrix — browser ${row.browser}`, () => {
    test.use({ locale: row.browser });

    test(`resolves ${row.code}, sets lang/dir, and stays axe-clean`, async ({
      page,
    }) => {
      await installMocks(page);
      const chunks = trackChunks(page);
      await page.goto("/e2e/fixture-locale.html");

      const root = page.locator(".fbm-root");
      await expect(root).toHaveAttribute("lang", row.lang);
      await expect(root).toHaveAttribute("dir", row.dir);

      // English is INLINED: an English page load must fetch no chunk at all.
      // A non-English one must fetch exactly its own.
      if (row.chunk === null) {
        expect(chunks, "English must not fetch a locale chunk").toEqual([]);
      } else {
        expect(chunks, `expected the ${row.chunk} chunk`).toContain(row.chunk);
      }

      await page.getByRole("button", { name: /Open feedback form/i }).click();
      await expect(page.getByRole("dialog")).toBeVisible();
      // Scoped to the widget: this fixture omits `lang` on <html> ON PURPOSE
      // (that is what puts `navigator.languages` in charge), and axe would
      // rightly report the FIXTURE for it. The widget's own subtree carries
      // the locale, and it is the subject here. `fixture-csp.html` below is a
      // well-formed page and gets the unscoped run.
      await expectNoAxeViolations(page, `modal-open in ${row.code}`, ".fbm-root");
    });
  });
}

// --------------------------------------------------------------------------
// Precedence (C36): embed > host page > browser
// --------------------------------------------------------------------------

test.describe("locale precedence", () => {
  test.use({ locale: "de-DE" });

  test("data-locale wins over the host page and the browser", async ({ page }) => {
    await installMocks(page);
    const chunks = trackChunks(page);
    await page.goto("/e2e/fixture-locale-attr.html");

    const root = page.locator(".fbm-root");
    // R-A11Y A-2: `fa` has no MT provider, so its catalog is English permanently.
    // `lang` states the language of the WORDS (en); `dir` still follows the
    // chosen locale, so the mirrored RTL layout survives.
    await expect(root).toHaveAttribute("lang", "en");
    await expect(root).toHaveAttribute("dir", "rtl");
    expect(chunks).toContain("fa");
    expect(chunks, "the German chunk must not be fetched").not.toContain("de");
  });

  test("the host page's <html lang> wins over the browser", async ({ page }) => {
    await installMocks(page);
    const chunks = trackChunks(page);
    // fixture.html declares lang="en" while the browser asks for German.
    await page.goto("/e2e/fixture.html");
    await expect(page.locator(".fbm-root")).toHaveAttribute("lang", "en");
    expect(chunks).toEqual([]);
  });
});

// --------------------------------------------------------------------------
// TGF-…-01: the lazy chunk under a real embedder's CSP
// --------------------------------------------------------------------------

test.describe("CSP (script-src 'self')", () => {
  test.use({ locale: "en-US" });

  test("loads the locale chunk with no CSP violation logged", async ({ page }) => {
    const violations: string[] = [];
    page.on("console", (msg) => {
      const text = msg.text();
      if (/content security policy|csp/i.test(text)) violations.push(text);
    });
    page.on("pageerror", (err) => violations.push(String(err)));

    await installMocks(page);
    const chunks = trackChunks(page);
    // The fixture is lang="de" and the browser is English: the host page wins,
    // so the German chunk is fetched — under `script-src 'self'`.
    await page.goto("/e2e/fixture-csp.html");

    const root = page.locator(".fbm-root");
    await expect(root).toHaveAttribute("lang", "de");
    expect(chunks, "the de chunk must load under CSP").toContain("de");
    expect(violations, "no CSP violation may be logged").toEqual([]);

    // And the widget is fully operable under that policy.
    await page.getByRole("button", { name: /Open feedback form/i }).click();
    await expect(page.getByRole("dialog")).toBeVisible();
    await expectNoAxeViolations(page, "modal-open under CSP");
  });
});

// --------------------------------------------------------------------------
// TGF-…-02: RTL cannot regress through a physical CSS property
// --------------------------------------------------------------------------

test.describe("RTL discipline in styles.css", () => {
  const CSS = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "..", "src", "styles.css"),
    "utf8",
  )
    // Comments explain the rule and name the forbidden properties; scanning
    // them would make this test fail on its own documentation.
    .replace(/\/\*[\s\S]*?\*\//g, "");

  const FORBIDDEN = [
    /margin-left\s*:/,
    /margin-right\s*:/,
    /padding-left\s*:/,
    /padding-right\s*:/,
    /border-left\s*:/,
    /border-right\s*:/,
    /(^|[^-\w])left\s*:/m,
    /(^|[^-\w])right\s*:/m,
    /text-align\s*:\s*(left|right)/,
    /float\s*:/,
  ];

  for (const pattern of FORBIDDEN) {
    test(`styles.css uses no ${pattern.source}`, () => {
      const match = pattern.exec(CSS);
      expect(
        match && match[0],
        `physical inline-axis property in widget/src/styles.css — use the logical form ` +
          `(inset-inline-*, margin-inline-*, padding-inline-*, text-align: start/end) ` +
          `so dir="rtl" mirrors without a per-direction rule`,
      ).toBeNull();
    });
  }
});
