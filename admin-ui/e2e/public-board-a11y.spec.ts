import { test, expect, type Page, type Route } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

// A11y smoke for the public feedback board page (Public Feedback Board +
// Moderation Gate, Contract C29).
//
// Mirrors `public-roadmap-a11y.spec.ts`: FAKE_API mode intercepts
// `/api/v1/projects/.../board*` and `/vote` and serves fixture JSON so this
// runs without the Rust backend. Set PLAYWRIGHT_FAKE_API=0 to run against a
// seeded local server.
//
// WCAG 2.1 AA target: zero axe-core violations on:
//   1. Initial idle render with approved items present (read-only vote counts)
//   2. The board-disabled (404) "not available" state
//
// Voting is deferred to a follow-up (Worker A Task Zero) — Stage 1 renders
// `vote_count` read-only, so there is no vote button to exercise here.
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
