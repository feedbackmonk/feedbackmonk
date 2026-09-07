import { test, expect, type Page, type Route } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

// A11y smoke for the hosting-settings page (FR-FBR-32/33, Contract C33).
// Mirrors `tier-settings-a11y.spec.ts`: FAKE_API mode intercepts
// `/api/v1/admin/hosting` and serves fixture JSON for each state. Set
// PLAYWRIGHT_FAKE_API=0 to run against a seeded local server.
//
// WCAG 2.1 AA target: zero axe-core violations across the four states this page
// actually has, which are genuinely different renders rather than cosmetic
// variants:
//
//   pro-empty      — the claim form, before any domain exists
//   pro-with-rows  — the domain table (live + waiting), where per-row controls
//                    and the status column live
//   free           — the upgrade prompt in place of the form
//   self-host      — no root domain configured, so neither section renders a
//                    control; the page explains itself instead
//
// The table state is the one worth the extra fixture: a row of identical
// "Remove" buttons is a classic accessible-name failure, so the component gives
// each one a visually-hidden domain suffix and this spec is what keeps that
// honest.

const FAKE_API = process.env.PLAYWRIGHT_FAKE_API !== "0";

type Scenario = "pro_empty" | "pro_with_rows" | "free" | "self_host";

const FIXTURES: Record<Scenario, unknown> = {
  pro_empty: {
    tier: "pro",
    subdomain: "acme",
    public_host: "acme.feedbackmonk.com",
    cname_target: "acme.feedbackmonk.com",
    custom_domain_available: true,
    domains: [],
  },
  pro_with_rows: {
    tier: "pro",
    subdomain: "acme",
    public_host: "acme.feedbackmonk.com",
    cname_target: "acme.feedbackmonk.com",
    custom_domain_available: true,
    domains: [
      {
        id: "11111111-1111-1111-1111-111111111111",
        domain: "feedback.acme.example",
        status: "active",
        created_at: "2026-08-20T00:00:00Z",
        verified_at: "2026-08-20T00:05:00Z",
      },
      {
        id: "22222222-2222-2222-2222-222222222222",
        domain: "ideas.acme.example",
        status: "pending",
        created_at: "2026-08-30T00:00:00Z",
        verified_at: null,
      },
    ],
  },
  free: {
    tier: "free",
    subdomain: "starter-co",
    public_host: "starter-co.feedbackmonk.com",
    cname_target: "starter-co.feedbackmonk.com",
    custom_domain_available: false,
    domains: [],
  },
  self_host: {
    tier: "self_host",
    subdomain: null,
    public_host: null,
    cname_target: null,
    custom_domain_available: true,
    domains: [],
  },
};

async function installFakeApi(page: Page, scenario: Scenario) {
  await page.route("**/api/v1/admin/hosting**", async (route: Route) => {
    if (route.request().method() === "GET") {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(FIXTURES[scenario]),
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

// Locale matrix (FR-FBR-38 / Stage 2 W-D). `de-DE` proves hosting-settings
// stays axe-clean once the admin console has a locale to resolve — the German
// catalog is a skeleton this arc (DEC-FBR-17), so the same English strings
// render per C35 rule 6 per-key fallback. Mirrors a11y.spec.ts.
const LOCALES = ["en-US", "de-DE"] as const;

for (const browser of LOCALES) {
  test.describe(`Hosting-settings a11y smoke (WCAG 2.1 AA, ${browser})`, () => {
    test.use({ locale: browser });

    for (const scenario of [
      "pro_empty",
      "pro_with_rows",
      "free",
      "self_host",
    ] as const) {
      test(`hosting view '${scenario}' has zero WCAG 2.1 AA violations`, async ({
        page,
      }) => {
        test.skip(!FAKE_API, "Real-backend mode requires a seeded tenant per tier");

        await installFakeApi(page, scenario);
        await page.goto("/admin/settings/hosting");

        await expect(
          page.getByRole("heading", { name: /^Public address$/, level: 1 }),
        ).toBeVisible();
        // Wait for the data-bound section to settle before axe runs.
        await expect(
          page.getByRole("heading", { name: /^Subdomain$/, level: 2 }),
        ).toBeVisible();

        await expectNoAxeViolations(page, `hosting-settings ${scenario} (${browser})`);
      });
    }

    test("each Remove control carries its own domain in its accessible name", async ({
      page,
    }) => {
      test.skip(!FAKE_API, "Real-backend mode requires a seeded tenant");

      await installFakeApi(page, "pro_with_rows");
      await page.goto("/admin/settings/hosting");

      // Two rows, two buttons, two DISTINCT names. Without the visually-hidden
      // suffix a screen-reader user hears "Remove, Remove" and cannot tell which
      // domain they are about to delete.
      await expect(
        page.getByRole("button", { name: /Remove feedback\.acme\.example/ }),
      ).toBeVisible();
      await expect(
        page.getByRole("button", { name: /Remove ideas\.acme\.example/ }),
      ).toBeVisible();
    });

    test("status is readable as text, not conveyed by colour alone (WCAG 1.4.1)", async ({
      page,
    }) => {
      test.skip(!FAKE_API, "Real-backend mode requires a seeded tenant");

      await installFakeApi(page, "pro_with_rows");
      await page.goto("/admin/settings/hosting");

      await expect(page.getByText("Live", { exact: true })).toBeVisible();
      await expect(page.getByText("Waiting for DNS", { exact: true })).toBeVisible();
    });
  });
}
