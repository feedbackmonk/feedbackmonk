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
    // AA-compliant default (blue-600). White-on-blue-500 (#3b82f6) only
    // reaches 3.67:1 — a real contrast bug surfaced once this harness was
    // first made runnable; the widget default + this fixture both use blue-600.
    primary_color: "#2563eb",
    logo_url: null,
    footer_text: "powered by feedbackmonk",
    footer_url: null,
    theme: null,
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

test.describe("feedbackmonk widget launcher-less + theme (DEC-FBR-IMPL-12/13)", () => {
  test.beforeEach(async ({ page }) => {
    await installMocks(page);
  });

  test("no floating launcher; [data-feedback-open] opens modal; dark theme applied", async ({
    page,
  }) => {
    await page.goto("/e2e/fixture-no-launcher.html");
    // Mount completes once the programmatic handle is published.
    await page.waitForFunction(
      () => !!(window as unknown as { feedbackmonk?: unknown }).feedbackmonk,
    );

    // data-fbm-no-auto-mount ⇒ NO floating launcher in the DOM.
    await expect(page.locator(".fbm-launcher")).toHaveCount(0);

    // data-theme="dark" ⇒ root carries the dark theme attribute.
    await expect(page.locator(".fbm-root")).toHaveAttribute(
      "data-fbm-theme",
      "dark",
    );

    // The host's own [data-feedback-open] button opens the modal — no JS glue.
    await page.locator("#host-trigger").click();
    const dialog = page.getByRole("dialog", { name: /Send feedback/i });
    await expect(dialog).toBeVisible();

    // Dark surface token applied (#1f2937).
    const bg = await dialog.evaluate((el) => getComputedStyle(el).backgroundColor);
    expect(bg).toBe("rgb(31, 41, 55)");

    // Dark modal is still WCAG 2.1 AA clean.
    await expectNoAxeViolations(page, "launcher-less dark modal-open");
  });

  test("window.feedbackmonk.open() opens the modal; destroy() removes the widget", async ({
    page,
  }) => {
    await page.goto("/e2e/fixture-no-launcher.html");
    await page.waitForFunction(
      () => !!(window as unknown as { feedbackmonk?: unknown }).feedbackmonk,
    );

    await page.evaluate(() =>
      (
        window as unknown as { feedbackmonk: { open: () => void } }
      ).feedbackmonk.open(),
    );
    const dialog = page.getByRole("dialog", { name: /Send feedback/i });
    await expect(dialog).toBeVisible();

    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();

    // destroy() detaches the whole widget root.
    await page.evaluate(() =>
      (
        window as unknown as { feedbackmonk: { destroy: () => void } }
      ).feedbackmonk.destroy(),
    );
    await expect(page.locator(".fbm-root")).toHaveCount(0);
  });
});

// 1x1 transparent PNG — a valid image/png the redaction canvas can draw.
const PNG_1x1 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

function pngFile(name: string) {
  return {
    name,
    mimeType: "image/png",
    buffer: Buffer.from(PNG_1x1, "base64"),
  };
}

async function openModal(page: Page) {
  await page.goto("/e2e/fixture.html");
  await page.getByRole("button", { name: /Open feedback form/i }).click();
  const dialog = page.getByRole("dialog", { name: /Send feedback/i });
  await expect(dialog).toBeVisible();
  return dialog;
}

test.describe("feedbackmonk widget attachments", () => {
  test.beforeEach(async ({ page }) => {
    await installMocks(page);
  });

  test("attaching a screenshot lists it and stays axe-clean", async ({ page }) => {
    await openModal(page);
    await page.locator('input[type="file"]').setInputFiles(pngFile("shot.png"));

    const item = page.locator(".fbm-attach-item");
    await expect(item).toHaveCount(1);
    await expect(item).toContainText("shot.png");
    // Modal-open WITH an attachment present must remain WCAG 2.1 AA clean.
    await expectNoAxeViolations(page, "modal-open + attachment");
  });

  test("rejects a non-image file with an error message", async ({ page }) => {
    await openModal(page);
    await page.locator('input[type="file"]').setInputFiles({
      name: "notes.txt",
      mimeType: "text/plain",
      buffer: Buffer.from("hello"),
    });
    await expect(page.locator(".fbm-attach-error")).toContainText(/PNG, JPEG, or WebP/i);
    await expect(page.locator(".fbm-attach-item")).toHaveCount(0);
  });

  test("enforces a maximum of 4 screenshots", async ({ page }) => {
    await openModal(page);
    await page
      .locator('input[type="file"]')
      .setInputFiles([
        pngFile("a.png"),
        pngFile("b.png"),
        pngFile("c.png"),
        pngFile("d.png"),
        pngFile("e.png"),
      ]);
    await expect(page.locator(".fbm-attach-item")).toHaveCount(4);
    await expect(page.locator(".fbm-attach-error")).toContainText(/at most 4/i);
  });

  test("redaction overlay opens, is axe-clean, and applies", async ({ page }) => {
    await openModal(page);
    await page.locator('input[type="file"]').setInputFiles(pngFile("secret.png"));
    await page.getByRole("button", { name: /Redact secret\.png/i }).click();

    const redactDialog = page.getByRole("dialog", { name: /Redact screenshot/i });
    await expect(redactDialog).toBeVisible();
    await expectNoAxeViolations(page, "redaction overlay");

    await page.getByRole("button", { name: /Apply redaction/i }).click();
    await expect(redactDialog).toBeHidden();
    await expect(page.locator(".fbm-attach-name")).toContainText(/redacted/i);
  });

  test("uploads attachments as multipart files[] after submit", async ({ page }) => {
    let attachBody: string | null = null;
    await page.route("**/attachments", async (route: Route) => {
      attachBody = route.request().postDataBuffer()?.toString("latin1") ?? "";
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify([{ attachment_id: "AT-1", url: "https://cdn/x.png" }]),
      });
    });

    const dialog = await openModal(page);
    await page.locator('input[type="file"]').setInputFiles(pngFile("bug.png"));
    await page.getByLabel("Subject").fill("Has a screenshot");
    await page.getByLabel("Message").fill("See the attached screenshot.");
    await page.getByRole("button", { name: /^Send$/ }).click();

    await expect(dialog).toBeHidden();
    await expect(page.getByRole("status")).toContainText(/Thanks/i);
    expect(attachBody).not.toBeNull();
    expect(attachBody!).toContain('name="files[]"');
    expect(attachBody!).toContain("bug.png");
  });
});

test.describe("feedbackmonk widget log capture", () => {
  test.beforeEach(async ({ page }) => {
    await installMocks(page);
  });

  test("sends a console_log text part when capture is opted in", async ({ page }) => {
    let attachBody: string | null = null;
    await page.route("**/attachments", async (route: Route) => {
      attachBody = route.request().postDataBuffer()?.toString("latin1") ?? "";
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify([]),
      });
    });

    await page.goto("/e2e/fixture-capture.html");
    // Produce a console log BEFORE opening the modal — capture starts at mount.
    await page.evaluate(() => console.log("diagnostic-marker-42"));

    await page.getByRole("button", { name: /Open feedback form/i }).click();
    const dialog = page.getByRole("dialog", { name: /Send feedback/i });
    await expect(dialog).toBeVisible();
    // Consent checkbox is rendered and on by default.
    const consent = page.getByLabel(/Include diagnostic logs/i);
    await expect(consent).toBeChecked();

    await page.getByLabel("Subject").fill("With logs");
    await page.getByLabel("Message").fill("Reproduces intermittently.");
    await page.getByRole("button", { name: /^Send$/ }).click();

    await expect(dialog).toBeHidden();
    expect(attachBody).not.toBeNull();
    expect(attachBody!).toContain('name="console_log"');
    expect(attachBody!).toContain("diagnostic-marker-42");
  });
});
