import { test, expect, type Page, type Route } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

// A11y smoke for the public roadmap page (FR-FBR-11/13, Contract C15).
//
// Mirrors the pattern from `a11y.spec.ts`: FAKE_API mode intercepts
// `/api/v1/projects/.../roadmap*` and `/vote` and serves fixture JSON so
// this runs without the Rust backend. Set PLAYWRIGHT_FAKE_API=0 to run
// against a seeded local server.
//
// WCAG 2.1 AA target: zero axe-core violations on:
//   1. Initial idle render with items present
//   2. After clicking a Vote button (toggles button state)
//
// The public route is /public/projects/:projectId/roadmap and has NO
// admin chrome (the layout is intentionally minimal so it can sit
// embedded under a customer's docs domain).

const FAKE_API = process.env.PLAYWRIGHT_FAKE_API !== "0";

const PROJECT_ID = "00000000-0000-0000-0000-000000000abc";

const LIST_BODY = {
  items: [
    {
      slug: "dark-mode",
      title: "Dark mode",
      body: "Add a dark theme option.",
      status: "considering",
      vote_count: 12,
      created_at: "2026-04-01T00:00:00Z",
      updated_at: "2026-04-01T00:00:00Z",
    },
    {
      slug: "csv-export",
      title: "CSV export",
      body: "Allow exporting feedback data to CSV.",
      status: "planned",
      vote_count: 7,
      created_at: "2026-04-02T00:00:00Z",
      updated_at: "2026-04-02T00:00:00Z",
    },
    {
      slug: "fixed-thing",
      title: "Fixed thing",
      body: "This is done.",
      status: "shipped",
      vote_count: 3,
      created_at: "2026-04-03T00:00:00Z",
      updated_at: "2026-04-03T00:00:00Z",
    },
  ],
  total: 3,
  limit: 50,
  offset: 0,
  cached_at: "2026-05-14T03:00:00Z",
};

const TOP_BODY = {
  items: [
    {
      slug: "dark-mode",
      title: "Dark mode",
      status: "considering",
      vote_count: 12,
    },
  ],
  cached_at: "2026-05-14T03:00:00Z",
};

const VOTE_RESPONSE = {
  item_slug: "dark-mode",
  voter_mode: "anon",
  cast_at: "2026-05-14T04:00:00Z",
};

async function installFakeApi(page: Page) {
  await page.route("**/api/v1/**", async (route: Route) => {
    const url = route.request().url();
    const method = route.request().method();
    if (
      url.match(/\/projects\/[^/]+\/roadmap\/top-voted\??/) &&
      method === "GET"
    ) {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(TOP_BODY),
      });
      return;
    }
    if (url.match(/\/projects\/[^/]+\/roadmap\??/) && method === "GET") {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(LIST_BODY),
      });
      return;
    }
    if (
      url.match(/\/projects\/[^/]+\/roadmap\/items\/[^/]+\/vote$/) &&
      method === "POST"
    ) {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(VOTE_RESPONSE),
      });
      return;
    }
    await route.fallback();
  });
}

async function expectNoAxeViolations(page: Page, label: string) {
  const results = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
    .analyze();
  expect(results.violations, `axe violations on ${label}`).toEqual([]);
}

// Locale matrix (FR-FBR-36 / TGF-02) — see the same block in
// `public-board-a11y.spec.ts` for why text is not asserted per locale while
// the catalogs are still skeletons, and why `fa` is in the matrix.
const LOCALE_MATRIX = [
  { browser: "en-US", expectLang: "en", expectDir: "ltr", expectSelected: "en" },
  { browser: "de-DE", expectLang: "de", expectDir: "ltr", expectSelected: "de" },
  // R-A11Y A-2: `fa` has no MT provider, so its catalog is English permanently.
  // `lang` states the language of the WORDS; `dir` still follows the chosen locale.
  { browser: "fa-IR", expectLang: "en", expectDir: "rtl", expectSelected: "fa" },
] as const;

for (const { browser, expectLang, expectDir, expectSelected } of LOCALE_MATRIX) {
  test.describe(`Public roadmap in ${browser}`, () => {
    test.use({ locale: browser });

    test(`resolves to lang=${expectLang} dir=${expectDir}, offers the switcher, and stays axe-clean`, async ({
      page,
    }) => {
      test.skip(!FAKE_API, "Real-backend mode requires a seeded project");

      await installFakeApi(page);
      await page.goto(`/public/projects/${PROJECT_ID}/roadmap`);
      await expect(page.getByText("CSV export")).toBeVisible();

      const html = page.locator("html");
      await expect(html).toHaveAttribute("lang", expectLang);
      await expect(html).toHaveAttribute("dir", expectDir);

      const switcher = page.getByRole("combobox", { name: "Language" });
      await expect(switcher).toBeVisible();
      // The switcher shows the locale the visitor is ON, which since R-A11Y A-2
      // is not always the language the words are in: a Persian visitor sees
      // "فارسی" selected while <html lang> honestly says "en".
      await expect(switcher).toHaveValue(expectSelected);

      await expectNoAxeViolations(page, `public roadmap ${browser}`);
    });
  });
}

test.describe("Public roadmap locale precedence", () => {
  test.use({ locale: "en-US" });

  test("?lang= overrides the browser and is not persisted", async ({ page }) => {
    test.skip(!FAKE_API, "Real-backend mode requires a seeded project");

    await installFakeApi(page);
    await page.goto(`/public/projects/${PROJECT_ID}/roadmap?lang=fa`);
    await expect(page.getByText("CSV export")).toBeVisible();
    await expect(page.locator("html")).toHaveAttribute("lang", "en");
    await expect(page.locator("html")).toHaveAttribute("dir", "rtl");
    const stored = await page.evaluate(() =>
      window.localStorage.getItem("fbm_lang"),
    );
    expect(stored).toBeNull();
  });
});

test.describe("Public roadmap a11y smoke", () => {
  test.beforeEach(async ({ page }) => {
    if (FAKE_API) {
      await installFakeApi(page);
    }
  });

  test("public roadmap idle + after-vote-click has zero WCAG 2.1 AA violations", async ({
    page,
  }) => {
    test.skip(
      !FAKE_API,
      "Real-backend mode requires a seeded project + roadmap items (P3 e2e seeding scripts)",
    );

    // 1. Initial idle render — items grouped by status, top-voted shortlist
    //    at top, cached_at footer at bottom.
    await page.goto(`/public/projects/${PROJECT_ID}/roadmap`);
    await expect(
      page.getByRole("heading", { name: /^Roadmap$/, level: 1 }),
    ).toBeVisible();
    await expect(page.getByText("CSV export")).toBeVisible();
    await expectNoAxeViolations(page, "public roadmap idle");

    // 2. Click the first Vote button (in the Considering section — Dark
    //    mode). The button toggles to "Voted" state with aria-pressed=true.
    //    Use a regex matcher because the accessible name embeds the live
    //    vote count, which moves across renders.
    const voteButton = page
      .getByRole("button", { name: /Vote for Dark mode/ })
      .first();
    await voteButton.click();
    // After mutation invalidates the query, the next render will refetch
    // — we don't depend on the response shape changing here (the fixture
    // returns the same body); the a11y assertion just checks the page
    // remains conformant during the in-flight state.
    await expectNoAxeViolations(page, "public roadmap post-vote-click");
  });
});
