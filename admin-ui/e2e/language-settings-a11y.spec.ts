import { test, expect, type Page, type Route } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

// A11y smoke for the tenant Language settings page (FR-FBR-38, Contract C38).
//
// FAKE_API mode intercepts `GET/PUT /api/v1/admin/settings/locale` — the
// endpoint itself is the backend lane's half of this stage, so this spec is
// written against the contract shape and needs no change when it lands. Set
// PLAYWRIGHT_FAKE_API=0 to run against a seeded local server.
//
// The state worth a fixture each: "browser default" (locale null) and a chosen
// locale, because they render different explanatory text under the same
// control, and the control is a 32-option `<select>` whose accessible name is
// the thing most likely to regress.

const FAKE_API = process.env.PLAYWRIGHT_FAKE_API !== "0";

async function installFakeApi(page: Page, locale: string | null) {
  let current = locale;
  await page.route("**/api/v1/admin/settings/locale", async (route: Route) => {
    const method = route.request().method();
    if (method === "PUT") {
      const body = route.request().postDataJSON() as { locale?: string | null };
      current = body.locale ?? null;
    }
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ locale: current, translate_outbound: false }),
    });
  });
}

async function expectNoAxeViolations(page: Page, label: string) {
  const results = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
    .analyze();
  expect(results.violations, `axe violations on ${label}`).toEqual([]);
}

test.describe("Language settings a11y smoke (WCAG 2.1 AA)", () => {
  test.use({ locale: "en-US" });

  for (const [label, fixture] of [
    ["browser default", null],
    ["a chosen locale", "de"],
  ] as const) {
    test(`language settings with ${label} has zero WCAG 2.1 AA violations`, async ({
      page,
    }) => {
      test.skip(!FAKE_API, "Real-backend mode requires a seeded tenant");

      await installFakeApi(page, fixture);
      await page.goto("/admin/settings/language");

      await expect(
        page.getByRole("heading", { name: /^Language$/, level: 1 }),
      ).toBeVisible();
      await expect(
        page.getByRole("combobox", { name: "Default language" }),
      ).toBeVisible();

      await expectNoAxeViolations(page, `language settings (${label})`);
    });
  }

  test("saving a language applies it immediately, without a reload", async ({
    page,
  }) => {
    test.skip(!FAKE_API, "Real-backend mode requires a seeded tenant");

    await installFakeApi(page, null);
    await page.goto("/admin/settings/language");

    const select = page.getByRole("combobox", { name: "Default language" });
    await expect(select).toBeVisible();
    await select.selectOption("fa");

    // The document follows the saved setting — including direction.
    // R-A11Y A-2: `fa` has no MT provider, so its catalog is English permanently.
    // `lang` states the language of the WORDS (en); `dir` still follows the
    // chosen locale, so the mirrored RTL layout survives.
    await expect(page.locator("html")).toHaveAttribute("lang", "en");
    await expect(page.locator("html")).toHaveAttribute("dir", "rtl");
    await expectNoAxeViolations(page, "language settings after save (fa, RTL)");
  });

  test("every shipped language is offered by its own name", async ({ page }) => {
    test.skip(!FAKE_API, "Real-backend mode requires a seeded tenant");

    await installFakeApi(page, null);
    await page.goto("/admin/settings/language");

    const select = page.getByRole("combobox", { name: "Default language" });
    await expect(select).toBeVisible();
    // 31 shipped locales + the "Browser default" option.
    await expect(select.locator("option")).toHaveCount(32);
    // Endonyms, not English exonyms: a reader looking for their language does
    // not know what we call it in English.
    await expect(
      select.locator("option", { hasText: "Deutsch" }),
    ).toHaveCount(1);
    await expect(select.locator("option", { hasText: "日本語" })).toHaveCount(1);
  });
});
