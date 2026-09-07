import { beforeEach, describe, expect, it } from "vitest";
import type { WidgetConfig } from "./types.js";
import { createLauncher, createModal, createRoot, errorKey, showError } from "./ui.js";
import { applyCatalog, setLocale, t } from "./i18n.js";

// The English-rendering guard for FR-FBR-35.
//
// Moving ~40 literals out of the DOM builders and into the catalog must be a
// PURE refactor for an English visitor: same words, same order, same
// punctuation. The snapshot below is the whole visible + accessible text of
// the modal, and it was captured from the pre-extraction build — if a key is
// mistyped, a value drifts, or a placeholder stops interpolating, it fails
// here rather than in front of a user.

const CONFIG: WidgetConfig = {
  project_id: "00000000-0000-0000-0000-000000000001",
  tenant_id: "00000000-0000-0000-0000-000000000002",
  display_name: "Fixture Project",
  brand: {
    primary_color: null,
    logo_url: "https://cdn.example/logo.png",
    footer_text: "powered by feedbackmonk",
    footer_url: null,
    theme: null,
  },
  auth_modes: ["auth", "anonymous"],
  submission_kinds: ["bug", "feature", "question", "other"],
  max_body_chars: 16384,
};

/** Every string a user can read or hear, in DOM order. */
function renderedText(el: HTMLElement): string[] {
  const out: string[] = [];
  const walk = (node: Element): void => {
    const aria = node.getAttribute("aria-label");
    if (aria) out.push(`[aria-label] ${aria}`);
    const alt = node.getAttribute("alt");
    if (alt) out.push(`[alt] ${alt}`);
    for (const child of Array.from(node.childNodes)) {
      if (child.nodeType === 3) {
        const text = (child.textContent ?? "").trim();
        if (text) out.push(text);
      } else if (child.nodeType === 1) {
        walk(child as Element);
      }
    }
  };
  walk(el);
  return out;
}

beforeEach(() => {
  setLocale("en");
});

describe("English rendering is unchanged by the catalog extraction", () => {
  it("renders the modal's text exactly as before", () => {
    const els = createModal(CONFIG, "anonymous", async () => {}, () => {}, true);
    expect(renderedText(els.scrim)).toEqual([
      "[aria-label] Close feedback form",
      "×",
      "[alt] Fixture Project logo",
      "Send feedback",
      "Tell us what's on your mind. Submissions are sent to Fixture Project.",
      "Subject",
      "Type",
      "Bug",
      "Feature request",
      "Question",
      "Other",
      "Message",
      "0 / 16384",
      "Email (optional)",
      "Include diagnostic logs to help us debug",
      "Cancel",
      "Send",
      "powered by feedbackmonk",
    ]);
  });

  it("renders the launcher's text exactly as before", () => {
    const btn = createLauncher("Fixture Project", () => {});
    expect(btn.getAttribute("aria-label")).toBe(
      "Open feedback form for Fixture Project",
    );
    expect(btn.textContent).toBe("Feedback");
  });

  it("keeps the tenant's own footer text out of the catalog", () => {
    // `footer_text` is tenant-authored content, not widget chrome: it must be
    // rendered verbatim and must never be looked up as a key.
    const els = createModal(
      { ...CONFIG, brand: { ...CONFIG.brand, footer_text: "widget.form.send" } },
      "auth",
      async () => {},
      () => {},
      false,
    );
    expect(els.scrim.querySelector(".fbm-footer a")?.textContent).toBe(
      "widget.form.send",
    );
  });

  it("updates the counter as the body is typed", () => {
    const els = createModal(CONFIG, "auth", async () => {}, () => {}, false);
    els.bodyTextarea.value = "hello";
    els.bodyTextarea.dispatchEvent(new Event("input"));
    expect(els.counter.textContent).toBe("5 / 16384");
  });
});

describe("the widget root declares its own language and direction (C36)", () => {
  it("carries lang and dir for English", () => {
    const root = createRoot();
    expect(root.lang).toBe("en");
    expect(root.dir).toBe("ltr");
  });

  it("carries lang and dir for a right-to-left locale", () => {
    setLocale("fa");
    const root = createRoot();
    expect(root.lang).toBe("fa");
    expect(root.dir).toBe("rtl");
  });
});

describe("the modal speaks the active locale", () => {
  it("renders translated chrome and falls back per key", () => {
    setLocale("de");
    applyCatalog("de", {
      "widget.modal.title": "Feedback senden",
      "widget.form.subject": "Betreff",
    });
    const els = createModal(CONFIG, "auth", async () => {}, () => {}, false);
    const text = renderedText(els.scrim);
    expect(text).toContain("Feedback senden");
    expect(text).toContain("Betreff");
    // Untranslated keys still render English, never a raw key.
    expect(text).toContain("Message");
    expect(text.some((s) => s.includes("widget."))).toBe(false);
  });
});

describe("server error text is never rendered verbatim", () => {
  const cases: [string, string][] = [
    ["invalid_input", "Subject and message are required."],
    ["network_error", "Could not send. Try again in a moment."],
    ["http_401", "Your session has expired. Reload the page and try again."],
    ["http_402", "This project has reached its feedback limit. Please contact the site owner."],
    ["http_403", "Your session has expired. Reload the page and try again."],
    ["http_404", "This feedback form is no longer available."],
    ["http_413", "That's too large to send. Try removing an attachment."],
    ["http_429", "Too many submissions just now. Please try again in a minute."],
    ["http_400", "Could not send. Check your entry and try again."],
    ["http_409", "Could not send. Check your entry and try again."],
    ["http_500", "Something went wrong on our side. Please try again shortly."],
    ["http_503", "Something went wrong on our side. Please try again shortly."],
    ["tier_cap", "This project has reached its feedback limit. Please contact the site owner."],
    ["something_new_from_the_server", "Could not send. Try again in a moment."],
  ];

  for (const [code, expected] of cases) {
    it(`renders our own copy for ${code}`, () => {
      const els = createModal(CONFIG, "auth", async () => {}, () => {}, false);
      showError(els, code);
      expect(els.errorRegion.textContent).toBe(expected);
      expect(els.errorRegion.hidden).toBe(false);
    });
  }

  it("never renders the server's message, even a hostile one", () => {
    const els = createModal(CONFIG, "auth", async () => {}, () => {}, false);
    showError(els, "http_500");
    expect(els.errorRegion.textContent).not.toContain("<");
    expect(els.errorRegion.textContent).toBe(t("widget.error.http_5xx"));
  });

  it("maps every code to a key that actually exists", () => {
    for (const [code] of cases) {
      expect(t(errorKey(code))).not.toBe(errorKey(code));
    }
  });
});
