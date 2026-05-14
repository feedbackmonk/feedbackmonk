import { test, expect, type Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

// FR-FBR-16 a11y smoke. For every page in the C23 routing scheme:
//   - Page loads (status 200).
//   - axe-core WCAG 2.1 A + AA passes zero violations.
//
// Mirrors admin-ui/e2e/a11y.spec.ts conventions:
//   - withTags(['wcag2a', 'wcag2aa']) for the canonical level-A+AA pass.
//   - Expectation is `[]` so any violation surfaces in the diff, not a count.

const PAGES = [
  { path: "/", label: "home (hero)" },
  { path: "/pricing", label: "/pricing" },
  { path: "/docs/", label: "/docs (index)" },
  { path: "/docs/widget", label: "/docs/widget" },
  { path: "/docs/api", label: "/docs/api" },
  { path: "/docs/self-host", label: "/docs/self-host" },
  { path: "/blog/show-hn-draft", label: "/blog/show-hn-draft" },
] as const;

async function expectNoAxeViolations(page: Page, label: string) {
  const results = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa"])
    .analyze();
  expect(results.violations, `axe violations on ${label}`).toEqual([]);
}

test.describe("Marketing site a11y smoke (FR-FBR-16)", () => {
  for (const { path, label } of PAGES) {
    test(`${label} loads and has zero axe violations`, async ({ page }) => {
      const response = await page.goto(path);
      expect(response?.status(), `HTTP status on ${label}`).toBe(200);
      await expectNoAxeViolations(page, label);
    });
  }

  test("skip link is reachable from keyboard", async ({ page }) => {
    await page.goto("/");
    await page.keyboard.press("Tab");
    const focused = page.locator(":focus");
    await expect(focused).toHaveText(/skip to main content/i);
  });

  test("pricing page renders all four tiers from tier_quotas.json", async ({ page }) => {
    await page.goto("/pricing");
    await expect(page.getByRole("heading", { name: "Free", level: 2 })).toBeVisible();
    await expect(page.getByRole("heading", { name: "Starter", level: 2 })).toBeVisible();
    await expect(page.getByRole("heading", { name: "Pro", level: 2 })).toBeVisible();
    await expect(page.getByRole("heading", { name: "Self-host", level: 2 })).toBeVisible();
  });

  test("free tier card carries the FR-FBR-14 footer string verbatim", async ({ page }) => {
    await page.goto("/pricing");
    // The PricingCard renders the footer_text from tier_quotas.json verbatim;
    // Free is the only tier with a non-null footer per Contract C19.
    const freeCard = page.locator(".pricing-card").filter({ has: page.getByRole("heading", { name: "Free", level: 2 }) });
    await expect(freeCard).toContainText('"powered by feedbackmonk"');
  });

  test("footer carries the canonical brand-attribution string", async ({ page }) => {
    await page.goto("/");
    // BRAND.md: literal `powered by feedbackmonk`, all lowercase, on every
    // marketing-site page. Asserts presence on the home page; structure is
    // shared across pages via BaseLayout so one assertion guards them all.
    const footer = page.getByRole("contentinfo");
    await expect(footer).toContainText(/powered by\s+feedbackmonk/i);
  });
});
