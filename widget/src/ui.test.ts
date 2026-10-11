import { beforeEach, describe, expect, it } from "vitest";
import type { WidgetConfig } from "./types.js";
import { createLauncher, createModal, createRoot, errorKey, showError, showSent } from "./ui.js";
import { jwtEmail } from "./widget.js";
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
      "Type",
      "Bug",
      "Feature request",
      "Question",
      "Other",
      "Message",
      "0 / 16384",
      "Email (optional)",
      "Only if you'd like a reply.",
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

  // R-A11Y A-2. `fa` has no MT provider, so its catalog is English PERMANENTLY;
  // the old assertion here was `lang === "fa"`, which declared Persian over
  // English words. `dir` is deliberately unchanged — the mirrored layout is what
  // the visitor's locale asks for and is still correct.
  it("carries the CONTENT language and the locale's dir for an english-fallback RTL locale", () => {
    setLocale("fa");
    const root = createRoot();
    expect(root.lang).toBe("en");
    expect(root.dir).toBe("rtl");
  });

  it("carries the locale itself when a provider covers it", () => {
    setLocale("de");
    const root = createRoot();
    expect(root.lang).toBe("de");
    expect(root.dir).toBe("ltr");
  });
});

describe("the modal speaks the active locale", () => {
  it("renders translated chrome and falls back per key", () => {
    setLocale("de");
    applyCatalog("de", {
      "widget.modal.title": "Feedback senden",
      "widget.form.message": "Nachricht",
    });
    const els = createModal(CONFIG, "auth", async () => {}, () => {}, false);
    const text = renderedText(els.scrim);
    expect(text).toContain("Feedback senden");
    expect(text).toContain("Nachricht");
    // Untranslated keys still render English, never a raw key.
    expect(text).toContain("Type");
    expect(text.some((s) => s.includes("widget."))).toBe(false);
  });
});

describe("server error text is never rendered verbatim", () => {
  const cases: [string, string][] = [
    // Reworded 2026-09-07 (FR-FBR-40, LEAD ruling on MSG-001 item 5): the
    // server now sends `code: "invalid_input"` for EVERY 400, including "body
    // too long" and "email is not a valid address" — both reachable from this
    // widget — so a subject/message-specific string would be confidently wrong.
    ["invalid_input", "Check your entry and try again."],
    // ADDED 2026-09-07 (critic finding C-005): the server now sends
    // `code: "internal"` for a 500, which reaches this map through the submit
    // call's `readError`. Without a key of its own it fell to
    // `widget.error.generic` ("Could not send…"), which is worse than the
    // `http_5xx` line the status fallback used to give. This case pins the
    // restored 5xx-quality wording so the regression cannot return silently.
    ["internal", "Something went wrong on our side. Please try again shortly."],
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

// A token whose payload carries the given claims. The widget never verifies a
// token — the server does — so the signature segment is a placeholder.
function token(claims: Record<string, unknown>): string {
  const b64 = btoa(JSON.stringify(claims))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
  return "eyJhbGciOiJFZERTQSJ9." + b64 + ".sig";
}

describe("the redesigned form (DEC-FBR-IMPL-35)", () => {
  it("shows the destination line, not a screen-reader-only one", () => {
    const els = createModal(CONFIG, "anonymous", async () => {}, () => {}, false);
    const desc = els.modal.querySelector(".fbm-desc");
    expect(desc?.textContent).toContain("Fixture Project");
    expect(els.modal.querySelector(".fbm-sr-only")).toBeNull();
  });

  it("offers the kinds as one radio group, first checked, and prompts per kind", () => {
    const els = createModal(CONFIG, "anonymous", async () => {}, () => {}, false);
    const radios = Array.from(
      els.modal.querySelectorAll<HTMLInputElement>('input[type="radio"]'),
    );
    expect(radios.map((r) => r.value)).toEqual(["bug", "feature", "question", "other"]);
    expect(new Set(radios.map((r) => r.name)).size).toBe(1);
    expect(els.kind()).toBe("bug");
    expect(els.bodyTextarea.placeholder).toBe(t("widget.prompt.bug"));
    radios[2].checked = true;
    radios[2].dispatchEvent(new Event("change"));
    expect(els.kind()).toBe("question");
    expect(els.bodyTextarea.placeholder).toBe(t("widget.prompt.question"));
  });

  it("asks for no subject", () => {
    const els = createModal(CONFIG, "anonymous", async () => {}, () => {}, false);
    expect(els.modal.querySelectorAll("input[type=text]").length).toBe(0);
  });

  it("keeps Send disabled until there is a message", () => {
    const els = createModal(CONFIG, "anonymous", async () => {}, () => {}, false);
    expect(els.submitBtn.disabled).toBe(true);
    els.bodyTextarea.value = "   ";
    els.bodyTextarea.dispatchEvent(new Event("input"));
    expect(els.submitBtn.disabled).toBe(true);
    els.bodyTextarea.value = "It broke";
    els.bodyTextarea.dispatchEvent(new Event("input"));
    expect(els.submitBtn.disabled).toBe(false);
  });

  it("hides the counter until the message nears the limit", () => {
    const els = createModal(
      { ...CONFIG, max_body_chars: 10 },
      "anonymous",
      async () => {},
      () => {},
      false,
    );
    expect(els.counter.hidden).toBe(true);
    els.bodyTextarea.value = "1234567";
    els.bodyTextarea.dispatchEvent(new Event("input"));
    expect(els.counter.hidden).toBe(true);
    els.bodyTextarea.value = "12345678";
    els.bodyTextarea.dispatchEvent(new Event("input"));
    expect(els.counter.hidden).toBe(false);
  });

  it("sends on Ctrl+Enter only once there is a message", () => {
    let sent = 0;
    const els = createModal(CONFIG, "anonymous", async () => { sent++; }, () => {}, false);
    const ctrlEnter = () =>
      els.bodyTextarea.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", ctrlKey: true, bubbles: true }),
      );
    ctrlEnter();
    expect(sent).toBe(0);
    els.bodyTextarea.value = "It broke";
    els.bodyTextarea.dispatchEvent(new Event("input"));
    ctrlEnter();
    expect(sent).toBe(1);
  });
});

describe("a signed-in sender (DEC-FBR-IMPL-35)", () => {
  it("asks for no email, says where replies go, and offers to send anonymously", () => {
    const els = createModal(CONFIG, "auth", async () => {}, () => {}, false, "ada@example.com");
    expect(els.emailInput).toBeNull();
    expect(els.modal.querySelector('input[type="email"]')).toBeNull();
    const text = renderedText(els.scrim);
    expect(text).toContain("Replies go to ada@example.com");
    expect(text).toContain("Send anonymously");
    expect(els.anonToggle?.checked).toBe(false);
  });

  it("marks the account line off and explains when Send anonymously is checked", () => {
    const els = createModal(CONFIG, "auth", async () => {}, () => {}, false, "ada@example.com");
    const who = els.modal.querySelector(".fbm-who")!;
    const hint = Array.from(els.modal.querySelectorAll<HTMLElement>(".fbm-hint")).find(
      (h) => h.textContent === t("widget.identity.anonymousHint"),
    )!;
    expect(hint.hidden).toBe(true);
    els.anonToggle!.checked = true;
    els.anonToggle!.dispatchEvent(new Event("change"));
    expect(who.classList.contains("fbm-off")).toBe(true);
    expect(hint.hidden).toBe(false);
  });

  it("offers no anonymous choice to a project that accepts only auth", () => {
    const els = createModal(
      { ...CONFIG, auth_modes: ["auth"] },
      "auth",
      async () => {},
      () => {},
      false,
      "ada@example.com",
    );
    expect(els.anonToggle).toBeNull();
  });

  it("says the account sends it when the token carries no email", () => {
    const els = createModal(CONFIG, "auth", async () => {}, () => {}, false, null);
    expect(renderedText(els.scrim)).toContain("Sent from your account");
  });

  it("reads the email claim from a token, and nothing from a malformed one", () => {
    expect(jwtEmail(token({ sub: "u1", email: "ada@example.com" }))).toBe("ada@example.com");
    expect(jwtEmail(token({ sub: "u1" }))).toBeNull();
    expect(jwtEmail("not-a-token")).toBeNull();
    expect(jwtEmail(undefined)).toBeNull();
  });
});

describe("the confirmation replaces the form", () => {
  it("shows the sent lines in a status region with a Done button", () => {
    let closed = 0;
    const els = createModal(CONFIG, "anonymous", async () => {}, () => { closed++; }, false);
    showSent(els, ["Your feedback was sent.", "We'll reply to ada@example.com."], () => { closed++; });
    expect(els.titleEl.textContent).toBe(t("widget.sent.title"));
    const status = els.modal.querySelector('[role="status"]')!;
    expect(status.textContent).toContain("We'll reply to ada@example.com.");
    expect(els.modal.querySelector("textarea")).toBeNull();
    const done = Array.from(els.modal.querySelectorAll("button")).find(
      (b) => b.textContent === t("widget.sent.close"),
    )!;
    done.click();
    expect(closed).toBe(1);
  });
});
