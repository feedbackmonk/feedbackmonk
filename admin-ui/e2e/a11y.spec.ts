import { test, expect, type Page, type Route } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

// Smoke a11y harness for FR-FBR-07. By default uses fake API mocks so it
// runs without the Rust backend on :14304. Set PLAYWRIGHT_FAKE_API=0 to run
// against a real backend.
const FAKE_API = process.env.PLAYWRIGHT_FAKE_API !== "0";

const FB_ID = "FB-ABCDEF";

const LIST_BODY = {
  items: [
    {
      feedback_id: FB_ID,
      kind: "bug",
      status: "submitted",
      body_excerpt: "Login button does not respond on mobile Safari.",
      submitted_at: "2026-05-13T22:00:00Z",
      submitter_label: "alice@example.com",
      reply_count: 0,
      sentiment: "negative",
    },
  ],
  total: 1,
  limit: 20,
  offset: 0,
};

const SENTIMENT_TREND_BODY = {
  bucket: "week",
  since: "2026-03-21T00:00:00Z",
  buckets: [
    {
      bucket_start: "2026-03-16T00:00:00Z",
      negative: 2,
      neutral: 5,
      positive: 11,
      total: 18,
    },
  ],
  totals: { negative: 2, neutral: 5, positive: 11, total: 18 },
};

const DETAIL_BODY = {
  feedback_id: FB_ID,
  kind: "bug",
  status: "submitted",
  body: "Login button does not respond on mobile Safari.\nSteps to reproduce: …",
  submitted_at: "2026-05-13T22:00:00Z",
  submitter: { kind: "authenticated", email: "alice@example.com" },
  status_history: [],
  replies: [],
  sentiment: "negative",
};

async function installFakeApi(page: Page) {
  await page.route("**/api/v1/**", async (route: Route) => {
    const url = route.request().url();
    const method = route.request().method();
    if (url.match(/\/api\/v1\/login$/) && method === "POST") {
      await route.fulfill({ status: 200, body: "{}" });
      return;
    }
    if (url.match(/\/admin\/feedback\/sentiment-trend/) && method === "GET") {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(SENTIMENT_TREND_BODY),
      });
      return;
    }
    if (url.match(/\/admin\/feedback\?/) && method === "GET") {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(LIST_BODY),
      });
      return;
    }
    if (url.match(/\/admin\/feedback\/[^/]+$/) && method === "GET") {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(DETAIL_BODY),
      });
      return;
    }
    if (url.match(/\/admin\/feedback\/[^/]+\/reply$/) && method === "POST") {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          reply_id: "00000000-0000-0000-0000-000000000001",
          feedback_id: FB_ID,
          visibility: "public",
          created_at: "2026-05-14T00:00:00Z",
          email_queued: true,
        }),
      });
      return;
    }
    if (url.match(/\/admin\/feedback\/[^/]+\/transition$/) && method === "POST") {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          feedback_id: FB_ID,
          from_status: "submitted",
          to_status: "triaged",
          transitioned_at: "2026-05-14T00:00:00Z",
          audit_id: "00000000-0000-0000-0000-000000000002",
          email_queued: true,
        }),
      });
      return;
    }
    await route.fallback();
  });
}

async function expectNoAxeViolations(page: Page, label: string) {
  const results = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa"])
    .analyze();
  expect(results.violations, `axe violations on ${label}`).toEqual([]);
}

// Locale matrix (FR-FBR-38 / Stage 2 W-D). Mirrors public-board-a11y.spec.ts:
// `de-DE` proves the admin flow still renders and stays axe-clean once the
// admin console itself has a locale to resolve — the German catalog is a
// skeleton this arc (DEC-FBR-17), so the SAME English strings are asserted
// per C35 rule 6 per-key fallback; only the machinery (lang/dir resolution)
// differs across locales, not the copy.
const LOCALE_MATRIX = [
  { browser: "en-US", expectLang: "en" },
  { browser: "de-DE", expectLang: "de" },
] as const;

for (const { browser, expectLang } of LOCALE_MATRIX) {
  test.describe(`Admin UI a11y smoke (${browser})`, () => {
    test.use({ locale: browser });

    test.beforeEach(async ({ page }) => {
      if (FAKE_API) {
        await installFakeApi(page);
      }
    });

    test("login → list → drawer → reply → transition has zero axe violations", async ({
      page,
    }) => {
      test.skip(
        !FAKE_API,
        "Real-backend mode requires a seeded admin (FIXME(stage-3): seed admin in e2e-p1-curl.sh)",
      );

      // 1. Login page
      await page.goto("/login");
      await expect(page.locator("html")).toHaveAttribute("lang", expectLang);
      await expectNoAxeViolations(page, "/login");

      // Fill + submit
      await page.getByLabel("Email").fill("admin@example.com");
      await page.getByLabel("Password", { exact: true }).fill("hunter2");
      await page.getByRole("button", { name: /Sign in/i }).click();

      // 2. List page
      await page.waitForURL("**/feedback");
      await expect(
        page.getByRole("heading", { name: "Feedback" }),
      ).toBeVisible();
      await expectNoAxeViolations(page, "/feedback (list)");

      // 3. Drawer
      await page.getByRole("row", { name: /Open FB-ABCDEF/ }).click();
      await expect(
        page.getByRole("dialog", { name: /FB-ABCDEF/ }),
      ).toBeVisible();
      await expectNoAxeViolations(page, "/feedback drawer");

      // 4. Reply
      await page.getByLabel("Reply body").fill("Thanks for the report.");
      await page.getByRole("button", { name: /Send reply/i }).click();
      await expectNoAxeViolations(page, "after reply submit");

      // 5. Transition — scope to within the drawer dialog (the page also has a
      // "Triaged" status-filter pill in the list-page nav, blocked by the scrim).
      const drawer = page.getByRole("dialog", { name: /FB-ABCDEF/ });
      await drawer.getByRole("button", { name: "Triaged" }).click();
      await expect(
        page.getByRole("dialog", { name: /Transition to Triaged/i }),
      ).toBeVisible();
      await expectNoAxeViolations(page, "transition dialog");
    });
  });
}

// RTL smoke (FR-FBR-38). Axe cannot judge visual direction, so this is the
// one machine check that the admin shell actually wires `dir="rtl"` and does
// not overflow horizontally when it does — same rationale as
// public-board-a11y.spec.ts's `fa-IR` row. A full RTL matrix across every
// admin a11y spec is not asked for (GUIDE task file step 7); this is the one.
test.describe("Admin UI RTL smoke (fa-IR)", () => {
  test.use({ locale: "fa-IR" });

  test.beforeEach(async ({ page }) => {
    if (FAKE_API) {
      await installFakeApi(page);
    }
  });

  test("login page resolves dir=rtl and never overflows horizontally", async ({
    page,
  }) => {
    test.skip(
      !FAKE_API,
      "Real-backend mode requires a seeded admin (FIXME(stage-3): seed admin in e2e-p1-curl.sh)",
    );

    await page.goto("/login");
    const html = page.locator("html");
    // R-A11Y A-2: `fa` has no MT provider, so its catalog is English permanently.
    // `lang` states the language of the WORDS (en); `dir` still follows the
    // chosen locale, so the mirrored RTL layout survives.
    await expect(html).toHaveAttribute("lang", "en");
    await expect(html).toHaveAttribute("dir", "rtl");

    const [scrollWidth, clientWidth] = await page.evaluate(() => [
      document.documentElement.scrollWidth,
      document.documentElement.clientWidth,
    ]);
    expect(scrollWidth, "no horizontal overflow in RTL").toBeLessThanOrEqual(
      clientWidth,
    );

    await expectNoAxeViolations(page, "/login (fa-IR)");
  });
});
