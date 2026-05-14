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
