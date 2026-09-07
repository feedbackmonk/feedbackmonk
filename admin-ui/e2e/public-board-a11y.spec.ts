import { test, expect, type Page, type Route } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

// A11y smoke for the public feedback board page (Public Feedback Board +
// Moderation Gate, Contract C29 + C30 voting).
//
// Mirrors `public-roadmap-a11y.spec.ts`: FAKE_API mode intercepts
// `/api/v1/projects/.../board*` and `/vote` and serves fixture JSON so this
// runs without the Rust backend. Set PLAYWRIGHT_FAKE_API=0 to run against a
// seeded local server.
//
// WCAG 2.1 AA target: zero axe-core violations on:
//   1. Initial idle render with approved items present (each carrying an
//      accessible vote button — Contract C30)
//   2. The board-disabled (404) "not available" state
//
// The public route is /public/projects/:projectId/board and has NO admin
// chrome. The fixture deliberately carries NO submitter identity (C29 privacy
// invariant) — there is nothing to anonymize on the client.

const FAKE_API = process.env.PLAYWRIGHT_FAKE_API !== "0";

const PROJECT_ID = "00000000-0000-0000-0000-000000000abc";
const DISABLED_PROJECT_ID = "00000000-0000-0000-0000-0000000d15ab";

const LIST_BODY = {
  items: [
    {
      short_code: "FB-AAA111",
      body: "The export button silently fails on Safari.",
      kind: "bug",
      status: "triaged",
      vote_count: 9,
      accepted_at: "2026-06-18T00:00:00Z",
    },
    {
      short_code: "FB-BBB222",
      body: "Please add a dark theme.",
      kind: "feature",
      status: "in-progress",
      vote_count: 14,
      accepted_at: "2026-06-17T00:00:00Z",
    },
  ],
  total: 2,
  limit: 50,
  offset: 0,
};

async function installFakeApi(page: Page) {
  await page.route("**/api/v1/**", async (route: Route) => {
    const url = route.request().url();
    const method = route.request().method();
    // Board-disabled project → 404 (C29 inv. 2).
    if (
      url.includes(`/projects/${DISABLED_PROJECT_ID}/board`) &&
      method === "GET"
    ) {
      await route.fulfill({
        status: 404,
        contentType: "application/json",
        body: JSON.stringify({ error: "BoardNotEnabled" }),
      });
      return;
    }
    if (url.match(/\/projects\/[^/]+\/board(\?|$)/) && method === "GET") {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(LIST_BODY),
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

// ─────────────────────────────────────────────────────────────────────────
// Locale matrix (FR-FBR-36 / TGF-02).
//
// `test.use({ locale })` sets the real browser locale, so this exercises the
// C34 resolver on `navigator.languages` rather than a stub. `fa-IR` is in the
// matrix because axe cannot judge visual direction: the ONLY machine check
// that RTL is wired at all is the `dir` attribute, so this is where it is
// asserted (a human pass is R-A11Y's job).
//
// TEXT IS NOT ASSERTED PER LOCALE, deliberately: in this arc every non-English
// catalog is still a skeleton (DEC-FBR-17 — translation is the owner's release
// step), so a German page legitimately renders English strings. What must hold
// in every locale is the machinery: the resolved `lang`, the `dir`, the
// switcher, and zero axe violations.
// ─────────────────────────────────────────────────────────────────────────

const LOCALE_MATRIX = [
  { browser: "en-US", expectLang: "en", expectDir: "ltr", expectSelected: "en" },
  { browser: "de-DE", expectLang: "de", expectDir: "ltr", expectSelected: "de" },
  // R-A11Y A-2: `fa` has no MT provider, so its catalog is English permanently.
  // `lang` states the language of the WORDS; `dir` still follows the chosen
  // locale, so a Persian visitor keeps the mirrored layout.
  { browser: "fa-IR", expectLang: "en", expectDir: "rtl", expectSelected: "fa" },
] as const;

for (const { browser, expectLang, expectDir, expectSelected } of LOCALE_MATRIX) {
  test.describe(`Public board in ${browser}`, () => {
    test.use({ locale: browser });

    test(`resolves to lang=${expectLang} dir=${expectDir}, offers the switcher, and stays axe-clean`, async ({
      page,
    }) => {
      test.skip(!FAKE_API, "Real-backend mode requires a seeded project");

      await installFakeApi(page);
      await page.goto(`/public/projects/${PROJECT_ID}/board`);
      await expect(page.getByText("Please add a dark theme.")).toBeVisible();

      const html = page.locator("html");
      await expect(html).toHaveAttribute("lang", expectLang);
      await expect(html).toHaveAttribute("dir", expectDir);

      // The visitor can always change language, from the page, without an
      // account and without navigating away.
      const switcher = page.getByRole("combobox", { name: "Language" });
      await expect(switcher).toBeVisible();
      // The switcher shows the locale the visitor is ON, which since R-A11Y A-2
      // is not always the language the words are in: a Persian visitor sees
      // "فارسی" selected while <html lang> honestly says "en".
      await expect(switcher).toHaveValue(expectSelected);

      await expectNoAxeViolations(page, `public board ${browser}`);
    });
  });
}

test.describe("Public board locale precedence", () => {
  test.use({ locale: "en-US" });

  test("?lang= overrides the browser for this page view", async ({ page }) => {
    test.skip(!FAKE_API, "Real-backend mode requires a seeded project");

    await installFakeApi(page);
    await page.goto(`/public/projects/${PROJECT_ID}/board?lang=de`);
    await expect(page.getByText("Please add a dark theme.")).toBeVisible();
    await expect(page.locator("html")).toHaveAttribute("lang", "de");
    // …and is NOT remembered: a shared link must not re-language the site.
    const stored = await page.evaluate(() =>
      window.localStorage.getItem("fbm_lang"),
    );
    expect(stored).toBeNull();
  });

  test("an unknown ?lang= is ignored, not echoed", async ({ page }) => {
    test.skip(!FAKE_API, "Real-backend mode requires a seeded project");

    await installFakeApi(page);
    await page.goto(
      `/public/projects/${PROJECT_ID}/board?lang=${encodeURIComponent('"><script>x</script>')}`,
    );
    await expect(page.getByText("Please add a dark theme.")).toBeVisible();
    await expect(page.locator("html")).toHaveAttribute("lang", "en");
  });

  test("choosing a language in the switcher persists it on this origin", async ({
    page,
  }) => {
    test.skip(!FAKE_API, "Real-backend mode requires a seeded project");

    await installFakeApi(page);
    await page.goto(`/public/projects/${PROJECT_ID}/board`);
    await expect(page.getByText("Please add a dark theme.")).toBeVisible();

    const before = page.url();
    await page
      .getByRole("combobox", { name: "Language" })
      .selectOption("fa");
    await expect(page.locator("html")).toHaveAttribute("lang", "en");
    await expect(page.locator("html")).toHaveAttribute("dir", "rtl");
    // Changing language NEVER navigates (no /fa/ path, no ?lang= rewrite).
    expect(page.url()).toBe(before);

    // Survives a reload — the choice is the visitor's, not the page view's.
    await page.reload();
    await expect(page.getByText("Please add a dark theme.")).toBeVisible();
    await expect(page.locator("html")).toHaveAttribute("lang", "en");
    await expect(page.locator("html")).toHaveAttribute("dir", "rtl");
  });
});

test.describe("Public board a11y smoke", () => {
  test.beforeEach(async ({ page }) => {
    if (FAKE_API) {
      await installFakeApi(page);
    }
  });

  test("public board idle render has zero WCAG 2.1 AA violations", async ({
    page,
  }) => {
    test.skip(
      !FAKE_API,
      "Real-backend mode requires a seeded project + approved feedback (e2e seeding scripts)",
    );

    // Initial idle render — approved items as a list, read-only vote counts.
    // Includes an in-progress status badge (regression guard for the
    // --status-in-progress AA contrast fix).
    await page.goto(`/public/projects/${PROJECT_ID}/board`);
    await expect(
      page.getByRole("heading", { name: /^Feedback board$/, level: 1 }),
    ).toBeVisible();
    await expect(page.getByText("Please add a dark theme.")).toBeVisible();
    await expectNoAxeViolations(page, "public board idle");
  });

  test("board-disabled project renders an accessible unavailable state", async ({
    page,
  }) => {
    test.skip(!FAKE_API, "Real-backend mode requires a seeded disabled project");

    await page.goto(`/public/projects/${DISABLED_PROJECT_ID}/board`);
    await expect(
      page.getByText(/this feedback board isn’t available/i),
    ).toBeVisible();
    await expectNoAxeViolations(page, "public board disabled");
  });
});
