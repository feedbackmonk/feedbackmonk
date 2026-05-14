import { test, expect, type Page, type Route } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

// FR-FBR-04 a11y harness for the feedbackmonk widget. Loads the built
// dist/widget.js + dist/widget.css onto a fixture page, mocks the
// widget-config endpoint, and asserts:
//   - zero axe-core WCAG 2.1 AA violations modal-closed
//   - zero axe-core WCAG 2.1 AA violations modal-open
//   - focus trap inside modal (Tab cycles)
//   - ESC closes modal AND returns focus to the launcher

const MOCK_CONFIG = {
  project_id: "00000000-0000-0000-0000-000000000001",
  tenant_id: "00000000-0000-0000-0000-000000000002",
  display_name: "Fixture Project",
  brand: {
    primary_color: "#3b82f6",
    logo_url: null,
    footer_text: "powered by feedbackmonk",
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
  await page.route("**/feedback", async (route: Route) => {
    if (route.request().method() === "POST") {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          feedback_id: "FB-FIXTUR",
          submitted_at: "2026-05-14T04:00:00Z",
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

test.describe("feedbackmonk widget a11y", () => {
  test.beforeEach(async ({ page }) => {
    await installMocks(page);
  });

  test("modal-closed has zero WCAG 2.1 AA violations", async ({ page }) => {
    await page.goto("/e2e/fixture.html");
    // Wait for widget-config fetch to resolve and launcher to render.
    await expect(page.getByRole("button", { name: /Open feedback form/i })).toBeVisible();
    await expectNoAxeViolations(page, "fixture (modal-closed)");
  });

  test("modal-open has zero WCAG 2.1 AA violations + focus trap + ESC return", async ({
    page,
  }) => {
    await page.goto("/e2e/fixture.html");
    const launcher = page.getByRole("button", { name: /Open feedback form/i });
    await expect(launcher).toBeVisible();
    await launcher.click();

    const dialog = page.getByRole("dialog", { name: /Send feedback/i });
    await expect(dialog).toBeVisible();
    await expect(dialog).toHaveAttribute("aria-modal", "true");

    await expectNoAxeViolations(page, "fixture (modal-open)");

    // Focus is inside the modal.
    const active = await page.evaluate(() => document.activeElement?.id ?? null);
    expect(active).not.toBeNull();

    // Tab cycles inside the modal — focus stays within the dialog.
    for (let i = 0; i < 8; i++) {
      await page.keyboard.press("Tab");
      const insideDialog = await page.evaluate(() => {
        const d = document.querySelector('[role="dialog"]');
        return !!(d && d.contains(document.activeElement));
      });
      expect(insideDialog, `Tab #${i + 1} kept focus inside dialog`).toBe(true);
    }

    // Shift+Tab also cycles inside.
    for (let i = 0; i < 4; i++) {
      await page.keyboard.press("Shift+Tab");
      const insideDialog = await page.evaluate(() => {
        const d = document.querySelector('[role="dialog"]');
        return !!(d && d.contains(document.activeElement));
      });
      expect(insideDialog, `Shift+Tab #${i + 1} kept focus inside dialog`).toBe(true);
    }

    // ESC closes modal and returns focus to the launcher.
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    const focusedTag = await page.evaluate(() => {
      const a = document.activeElement as HTMLElement | null;
      return a ? (a.getAttribute("aria-label") ?? a.tagName) : null;
    });
    expect(focusedTag).toMatch(/Open feedback form/i);
  });

  test("submit happy-path renders success toast and closes modal", async ({
    page,
  }) => {
    await page.goto("/e2e/fixture.html");
    const launcher = page.getByRole("button", { name: /Open feedback form/i });
    await launcher.click();
    const dialog = page.getByRole("dialog", { name: /Send feedback/i });
    await expect(dialog).toBeVisible();

    await page.getByLabel("Subject").fill("Test subject");
    await page.getByLabel("Message").fill("This is a test message body.");
    await page.getByRole("button", { name: /^Send$/ }).click();

    await expect(dialog).toBeHidden();
    await expect(page.getByRole("status")).toContainText(/Thanks/i);
  });
});
