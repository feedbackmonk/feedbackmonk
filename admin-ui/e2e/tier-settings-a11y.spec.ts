import { test, expect, type Page, type Route } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

// A11y smoke for the admin tier-settings page (P3 Stage 2, FR-FBR-14,
// Contract C17). Mirrors `public-roadmap-a11y.spec.ts`: FAKE_API mode
// intercepts `/api/v1/admin/tier` and serves Contract-C19 fixture JSON for
// each of the four tiers. Set PLAYWRIGHT_FAKE_API=0 to run against a
// seeded local server (requires TIER_OVERRIDE.md flips per dogfood tenant).
//
// WCAG 2.1 AA target: zero axe-core violations on each tier-view —
// Free / Starter / Pro / Self-host. The four tiers exercise:
//   - bounded vs unlimited progressbars (Free/Starter vs Pro/Self-host)
//   - meter color states (Free 0/50 → ok; Starter 480/500 → danger)
//   - capability matrix (custom domain ✓/✗, EU residency ✓/✗, footer)
//   - upgrade-prompt visibility (rendered for Free/Starter/Pro; absent on Self-host)

const FAKE_API = process.env.PLAYWRIGHT_FAKE_API !== "0";

type Tier = "free" | "starter" | "pro" | "self_host";

const FIXTURES: Record<Tier, unknown> = {
  free: {
    tier: "free",
    quotas: {
      projects_per_org: 1,
      monthly_feedback_volume: 50,
      custom_branding: false,
      custom_domain: false,
      eu_residency: false,
      footer_text: "powered by feedbackmonk",
    },
    usage: {
      projects: 1,
      feedback_monthly: 12,
      period_start: "2026-04-14T00:00:00Z",
    },
  },
  starter: {
    tier: "starter",
    quotas: {
      projects_per_org: 3,
      monthly_feedback_volume: 500,
      custom_branding: true,
      custom_domain: false,
      eu_residency: false,
      footer_text: null,
    },
    usage: {
      projects: 2,
      feedback_monthly: 480,
      period_start: "2026-04-14T00:00:00Z",
    },
  },
  pro: {
    tier: "pro",
    quotas: {
      projects_per_org: null,
      monthly_feedback_volume: 10000,
      custom_branding: true,
      custom_domain: true,
      eu_residency: true,
      footer_text: null,
    },
    usage: {
      projects: 12,
      feedback_monthly: 4500,
      period_start: "2026-04-14T00:00:00Z",
    },
  },
  self_host: {
    tier: "self_host",
    quotas: {
      projects_per_org: null,
      monthly_feedback_volume: null,
      custom_branding: true,
      custom_domain: true,
      eu_residency: true,
      footer_text: null,
    },
    usage: {
      projects: 5,
      feedback_monthly: 50000,
      period_start: "2026-04-14T00:00:00Z",
    },
  },
};

async function installFakeApi(page: Page, tier: Tier) {
  await page.route("**/api/v1/admin/tier**", async (route: Route) => {
    if (route.request().method() === "GET") {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(FIXTURES[tier]),
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

test.describe("Tier-settings a11y smoke (per-tier WCAG 2.1 AA)", () => {
  for (const tier of ["free", "starter", "pro", "self_host"] as const) {
    test(`tier-view '${tier}' has zero WCAG 2.1 AA violations`, async ({
      page,
    }) => {
      test.skip(
        !FAKE_API,
        "Real-backend mode requires a seeded tenant per tier (TIER_OVERRIDE.md)",
      );

      await installFakeApi(page, tier);
      await page.goto("/admin/settings/tier");

      await expect(
        page.getByRole("heading", { name: /^Plan & usage$/, level: 1 }),
      ).toBeVisible();

      // Wait for the tier label to appear so the data-bound state has
      // settled before axe runs.
      const tierLabels: Record<Tier, RegExp> = {
        free: /\bFree\b/,
        starter: /\bStarter\b/,
        pro: /\bPro\b/,
        self_host: /\bSelf-host\b/,
      };
      await expect(page.getByText(tierLabels[tier]).first()).toBeVisible();

      await expectNoAxeViolations(page, `tier-settings ${tier}`);
    });
  }
});
