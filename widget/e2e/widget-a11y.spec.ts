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

  test("submit happy-path shows the confirmation in the dialog, then closes", async ({
    page,
  }) => {
    let sent: Record<string, unknown> | null = null;
    page.on("request", (req) => {
      if (req.method() === "POST" && req.url().endsWith("/feedback")) {
        sent = req.postDataJSON();
      }
    });
    await page.goto("/e2e/fixture.html");
    const launcher = page.getByRole("button", { name: /Open feedback form/i });
    await launcher.click();
    const dialog = page.getByRole("dialog", { name: /Send feedback/i });
    await expect(dialog).toBeVisible();

    const send = page.getByRole("button", { name: /^Send$/ });
    await expect(send).toBeDisabled();
    await page.getByRole("radio", { name: "Question" }).check();
    await page
      .getByLabel("Message")
      .fill("How do I export my data?\nI looked in settings and found nothing.");
    await send.click();

    const thanks = page.getByRole("dialog", { name: /Thanks for your feedback/i });
    await expect(thanks).toBeVisible();
    await expect(page.getByRole("status")).toContainText("Your feedback was sent.");
    expect(sent).toMatchObject({
      kind: "question",
      subject: "How do I export my data?",
      body: "How do I export my data?\nI looked in settings and found nothing.",
    });
    await expectNoAxeViolations(page, "confirmation");

    await page.getByRole("button", { name: "Done" }).click();
    await expect(thanks).toBeHidden();
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

  test("a pasted image becomes an attachment", async ({ page }) => {
    await openModal(page);
    await page.getByLabel("Message").focus();
    await page.evaluate((b64) => {
      const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
      const dt = new DataTransfer();
      dt.items.add(new File([bytes], "pasted.png", { type: "image/png" }));
      document
        .querySelector("textarea")!
        .dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true }));
    }, PNG_1x1);
    await expect(page.locator(".fbm-attach-item")).toHaveCount(1);
    await expect(page.locator(".fbm-attach-item")).toContainText("pasted.png");
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

    await openModal(page);
    await page.locator('input[type="file"]').setInputFiles(pngFile("bug.png"));
    await page.getByLabel("Message").fill("See the attached screenshot.");
    await page.getByRole("button", { name: /^Send$/ }).click();

    await expect(page.getByRole("status")).toContainText("Your feedback was sent.");
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

    await page.getByLabel("Message").fill("Reproduces intermittently.");
    await page.getByRole("button", { name: /^Send$/ }).click();

    await expect(page.getByRole("status")).toContainText("Your feedback was sent.");
    expect(attachBody).not.toBeNull();
    expect(attachBody!).toContain('name="console_log"');
    expect(attachBody!).toContain("diagnostic-marker-42");
  });
});

// P2-2: tenant-supplied brand values (footer_url / logo_url → href/src,
// primary_color → CSS var) must be scheme/value-checked so a `javascript:`
// URL can't reach an href and CSS can't be smuggled into the color property.
test.describe("feedbackmonk widget config-value hardening (P2-2)", () => {
  async function routeBrand(page: Page, brand: Record<string, unknown>) {
    await page.route("**/widget-config", async (route: Route) => {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          ...MOCK_CONFIG,
          brand: { ...MOCK_CONFIG.brand, ...brand },
        }),
      });
    });
  }

  test("drops javascript: URLs and a non-color primary_color", async ({ page }) => {
    await routeBrand(page, {
      footer_text: "powered by feedbackmonk",
      footer_url: "javascript:alert(document.domain)",
      logo_url: "javascript:alert(1)",
      primary_color: "red; } html { background: url(https://evil.example/x)",
    });
    await page.goto("/e2e/fixture.html");
    await page.getByRole("button", { name: /Open feedback form/i }).click();
    await expect(page.getByRole("dialog", { name: /Send feedback/i })).toBeVisible();

    const probe = await page.evaluate(() => {
      const footer = document.querySelector<HTMLAnchorElement>(".fbm-footer a");
      const root = document.querySelector<HTMLElement>("[data-fbm-root]");
      return {
        footerHref: footer?.href ?? null,
        logoCount: document.querySelectorAll("img.fbm-logo").length,
        primary: root?.style.getPropertyValue("--fbm-primary") ?? "",
      };
    });

    // A javascript: URL never reaches the href — it falls back to the badge default.
    expect(probe.footerHref).toBe("https://feedbackmonk.com/");
    // A javascript: logo_url renders no <img> at all.
    expect(probe.logoCount).toBe(0);
    // A non-color primary_color is ignored — the widget keeps its stylesheet default.
    expect(probe.primary).toBe("");
  });

  test("honors valid http(s) URLs and a plain color token", async ({ page }) => {
    await routeBrand(page, {
      footer_text: "powered by feedbackmonk",
      footer_url: "https://acme.example/feedback",
      logo_url: "https://acme.example/logo.png",
      primary_color: "#123456",
    });
    await page.goto("/e2e/fixture.html");
    await page.getByRole("button", { name: /Open feedback form/i }).click();
    await expect(page.getByRole("dialog", { name: /Send feedback/i })).toBeVisible();

    const probe = await page.evaluate(() => {
      const footer = document.querySelector<HTMLAnchorElement>(".fbm-footer a");
      const logo = document.querySelector<HTMLImageElement>("img.fbm-logo");
      const root = document.querySelector<HTMLElement>("[data-fbm-root]");
      return {
        footerHref: footer?.href ?? null,
        logoSrc: logo?.src ?? null,
        primary: root?.style.getPropertyValue("--fbm-primary") ?? "",
      };
    });

    expect(probe.footerHref).toBe("https://acme.example/feedback");
    expect(probe.logoSrc).toBe("https://acme.example/logo.png");
    expect(probe.primary).toBe("#123456");
  });
});

// DEC-FBR-IMPL-35: a signed-in host page serves the user's token at
// data-token-url. The widget must show where replies go, ask for no email,
// send the token — and send NO token when the person chooses anonymity.
test.describe("feedbackmonk widget signed-in host (DEC-FBR-IMPL-35)", () => {
  function userToken(email: string, sub = "u-1"): string {
    const payload = Buffer.from(JSON.stringify({ sub, email }))
      .toString("base64")
      .replace(/\+/g, "-")
      .replace(/\//g, "_")
      .replace(/=+$/, "");
    return "eyJhbGciOiJFZERTQSJ9." + payload + ".sig";
  }

  async function serveToken(page: Page, status: number, body?: unknown) {
    await page.route("**/e2e/mock/token", async (route: Route) => {
      await route.fulfill({
        status,
        contentType: "application/json",
        body: body === undefined ? "" : JSON.stringify(body),
      });
    });
  }

  function captureSubmit(page: Page): { auth: string | null | undefined } {
    const seen: { auth: string | null | undefined } = { auth: undefined };
    page.on("request", (req) => {
      if (req.method() === "POST" && req.url().endsWith("/feedback")) {
        seen.auth = req.headers()["authorization"] ?? null;
      }
    });
    return seen;
  }

  async function open(page: Page) {
    await page.goto("/e2e/fixture-signed-in.html");
    await page.getByRole("button", { name: /Open feedback form/i }).click();
    const dialog = page.getByRole("dialog", { name: /Send feedback/i });
    await expect(dialog).toBeVisible();
    return dialog;
  }

  test.beforeEach(async ({ page }) => {
    await installMocks(page);
  });

  test("shows where replies go, asks for no email, and sends the token", async ({ page }) => {
    const jwt = userToken("ada@example.com");
    await serveToken(page, 200, { token: jwt, expires_at: Date.now() / 1000 + 300 });
    const seen = captureSubmit(page);
    const dialog = await open(page);

    await expect(dialog).toContainText("Replies go to ada@example.com");
    await expect(page.locator('input[type="email"]')).toHaveCount(0);
    await expect(page.getByLabel("Send anonymously")).not.toBeChecked();
    await expectNoAxeViolations(page, "signed-in modal-open");

    await page.getByLabel("Message").fill("Signed-in report.");
    await page.getByRole("button", { name: /^Send$/ }).click();
    await expect(page.getByRole("status")).toContainText("We'll reply to ada@example.com.");
    expect(seen.auth).toBe("Bearer " + jwt);
  });

  test("Send anonymously sends no token at all", async ({ page }) => {
    await serveToken(page, 200, {
      token: userToken("ada@example.com"),
      expires_at: Date.now() / 1000 + 300,
    });
    const seen = captureSubmit(page);
    await open(page);

    await page.getByLabel("Send anonymously").check();
    await page.getByLabel("Message").fill("Anonymous report.");
    await page.getByRole("button", { name: /^Send$/ }).click();
    await expect(page.getByRole("status")).toContainText("Your feedback was sent.");
    await expect(page.getByRole("status")).not.toContainText("ada@example.com");
    expect(seen.auth).toBeNull();
  });

  test("a signed-out answer leaves the anonymous form with its email field", async ({
    page,
  }) => {
    await serveToken(page, 401, { error: "sign in required" });
    const seen = captureSubmit(page);
    await open(page);

    await expect(page.getByLabel("Email (optional)")).toBeVisible();
    await expect(page.getByLabel("Send anonymously")).toHaveCount(0);
    await page.getByLabel("Message").fill("Signed-out report.");
    await page.getByRole("button", { name: /^Send$/ }).click();
    await expect(page.getByRole("status")).toContainText("Your feedback was sent.");
    expect(seen.auth).toBeNull();
  });

  test("an expired token is replaced before the send", async ({ page }) => {
    const fresh = userToken("ada@example.com", "u-2");
    let calls = 0;
    await page.route("**/e2e/mock/token", async (route: Route) => {
      calls++;
      // The first token is already inside the 30-second refresh margin.
      const body =
        calls === 1
          ? { token: userToken("ada@example.com"), expires_at: Date.now() / 1000 + 5 }
          : { token: fresh, expires_at: Date.now() / 1000 + 300 };
      await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(body) });
    });
    const seen = captureSubmit(page);
    await open(page);

    await page.getByLabel("Message").fill("Long-open tab.");
    await page.getByRole("button", { name: /^Send$/ }).click();
    await expect(page.getByRole("status")).toContainText("Your feedback was sent.");
    expect(calls).toBe(2);
    expect(seen.auth).toBe("Bearer " + fresh);
  });
});
