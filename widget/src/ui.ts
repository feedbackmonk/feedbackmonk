import type { WidgetConfig, WidgetTheme } from "./types.js";
import { dir as localeDir, activeLocale, hasKey, t } from "./i18n.js";

// DOM construction helpers for the feedbackmonk widget. CSP-safe:
//   - No `innerHTML` with user input.
//   - No inline event handlers (`onclick="…"`); all listeners attached via
//     addEventListener.
//   - Only the SVG icon uses innerHTML, and its content is a static literal.

type Listener = () => void;

export interface ModalElements {
  scrim: HTMLDivElement;
  modal: HTMLDivElement;
  closeBtn: HTMLButtonElement;
  titleEl: HTMLElement;
  subjectInput: HTMLInputElement;
  bodyTextarea: HTMLTextAreaElement;
  kindSelect: HTMLSelectElement;
  emailInput: HTMLInputElement | null;
  emailField: HTMLDivElement | null;
  counter: HTMLSpanElement;
  errorRegion: HTMLDivElement;
  submitBtn: HTMLButtonElement;
  cancelBtn: HTMLButtonElement;
  // Mount point for the attachments controller (built by attachments.ts).
  attachContainer: HTMLDivElement;
  // Diagnostic-log consent checkbox; null unless log capture is available.
  logConsent: HTMLInputElement | null;
  focusables: HTMLElement[];
}

function makeId(prefix: string): string {
  return prefix + "-" + Math.random().toString(36).slice(2, 10);
}

export function createElement<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  if (className) el.className = className;
  if (text !== undefined) el.textContent = text;
  return el;
}

// Scheme allowlist for tenant-supplied URLs (P2-2). Config values like
// `footer_url`/`logo_url` are tenant-controlled and land on an `href`/`src`;
// a `javascript:` / `data:` / `vbscript:` value there is an injection vector.
// Returns the browser-canonical URL only when it resolves to http(s);
// otherwise null so the caller can fall back or skip the element.
export function safeHttpUrl(value: string | null | undefined): string | null {
  if (!value) return null;
  let parsed: URL;
  try {
    parsed = new URL(value, window.location.href);
  } catch {
    return null;
  }
  return parsed.protocol === "http:" || parsed.protocol === "https:"
    ? parsed.href
    : null;
}

// Value guard for `primary_color` (P2-2). It is injected as a CSS custom-
// property value; reject anything that isn't a plain color token so a value
// can't smuggle in `url(...)` or other CSS. Allows #hex (3/4/6/8),
// rgb/rgba/hsl/hsla(), and CSS named colors. null → caller keeps the default.
const CSS_COLOR_RE =
  /^#([0-9a-fA-F]{3}|[0-9a-fA-F]{4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$|^(rgb|rgba|hsl|hsla)\([0-9.,%\s/]+\)$|^[a-zA-Z]+$/;
export function safeCssColor(value: string | null | undefined): string | null {
  if (!value) return null;
  const v = value.trim();
  return CSS_COLOR_RE.test(v) ? v : null;
}

// Live focusable query for a container — robust to dynamically added controls
// (attachment buttons, redaction overlay) that a static array would miss.
const FOCUSABLE_SELECTOR =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), ' +
  'textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export function getFocusable(container: HTMLElement): HTMLElement[] {
  return Array.from(
    container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR),
  ).filter((el) => !el.hidden && el.getAttribute("aria-hidden") !== "true");
}

const LAUNCHER_ICON_SVG =
  '<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">' +
  '<path d="M4 4h16a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H8l-4 4V6a2 2 0 0 1 2-2z"/>' +
  "</svg>";

/// The widget root declares its OWN language and direction (FR-FBR-35 /
/// Contract C36). Without these the widget's content inherits the host page's
/// `<html lang>`, which misdeclares it to a screen reader whenever the two
/// differ (WCAG 3.1.2) — the D-FBR-31 defect. `dir` is what makes the Persian
/// layout mirror; both values come from the C34 table, never from raw input.
export function createRoot(): HTMLDivElement {
  const root = createElement("div", "fbm-root");
  root.setAttribute("data-fbm-root", "");
  root.lang = activeLocale();
  root.dir = localeDir();
  return root;
}

export function createLauncher(
  brandLabel: string,
  onOpen: Listener,
): HTMLButtonElement {
  const btn = createElement("button", "fbm-launcher");
  btn.type = "button";
  btn.setAttribute("aria-haspopup", "dialog");
  // SVG content is a static literal — CSP-safe; embedders' style-src does
  // not need unsafe-inline because we use external stylesheet for CSS.
  btn.innerHTML = LAUNCHER_ICON_SVG;
  const label = createElement("span", undefined, t("widget.launcher.label"));
  btn.appendChild(label);
  btn.setAttribute(
    "aria-label",
    t("widget.launcher.openAria", { brand: brandLabel }),
  );
  btn.addEventListener("click", onOpen);
  return btn;
}

export function createModal(
  config: WidgetConfig,
  mode: "auth" | "anonymous",
  onSubmit: () => Promise<void>,
  onClose: Listener,
  logCaptureAvailable: boolean,
): ModalElements {
  const titleId = makeId("fbm-title");
  const bodyId = makeId("fbm-body");
  const errorId = makeId("fbm-error");

  const scrim = createElement("div", "fbm-scrim");
  scrim.setAttribute("role", "presentation");

  const modal = createElement("div", "fbm-modal");
  modal.setAttribute("role", "dialog");
  modal.setAttribute("aria-modal", "true");
  modal.setAttribute("aria-labelledby", titleId);
  modal.setAttribute("aria-describedby", bodyId);
  modal.style.position = "relative";

  const closeBtn = createElement("button", "fbm-close");
  closeBtn.type = "button";
  closeBtn.setAttribute("aria-label", t("widget.modal.closeAria"));
  closeBtn.textContent = "×";
  closeBtn.addEventListener("click", onClose);

  // Optional per-tenant logo (DEC-FBR-IMPL-12) — rendered in the modal header.
  // Scheme-checked (P2-2): a non-http(s) logo_url is dropped, not rendered.
  let logoEl: HTMLImageElement | null = null;
  const logoSrc = safeHttpUrl(config.brand.logo_url);
  if (logoSrc) {
    logoEl = createElement("img", "fbm-logo");
    logoEl.src = logoSrc;
    logoEl.alt = t("widget.modal.logoAlt", { brand: config.display_name });
    logoEl.decoding = "async";
    logoEl.loading = "lazy";
  }

  const titleEl = createElement("h2", "fbm-title", t("widget.modal.title"));
  titleEl.id = titleId;

  const descEl = createElement(
    "p",
    "fbm-sr-only",
    t("widget.modal.description", { brand: config.display_name }),
  );
  descEl.id = bodyId;

  const subjectField = createElement("div", "fbm-field");
  const subjectLabel = createElement("label", undefined, t("widget.form.subject"));
  const subjectInput = createElement("input");
  subjectInput.type = "text";
  subjectInput.required = true;
  subjectInput.maxLength = 200;
  const subjectId = makeId("fbm-subject");
  subjectInput.id = subjectId;
  subjectLabel.htmlFor = subjectId;
  subjectField.append(subjectLabel, subjectInput);

  const kindField = createElement("div", "fbm-field");
  const kindLabel = createElement("label", undefined, t("widget.form.kind"));
  const kindSelect = createElement("select");
  const kindId = makeId("fbm-kind");
  kindSelect.id = kindId;
  kindLabel.htmlFor = kindId;
  for (const kind of config.submission_kinds) {
    const opt = createElement("option");
    opt.value = kind;
    // An unknown kind from a newer server renders its wire value rather than
    // a raw catalog key — `t()` returns the key when nothing matches, so the
    // `hasKey` guard is what keeps that path honest.
    const key = "widget.kind." + kind;
    opt.textContent = hasKey(key) ? t(key) : kind;
    kindSelect.appendChild(opt);
  }
  kindField.append(kindLabel, kindSelect);

  const bodyField = createElement("div", "fbm-field");
  const bodyLabel = createElement("label", undefined, t("widget.form.message"));
  const bodyTextarea = createElement("textarea");
  bodyTextarea.required = true;
  bodyTextarea.maxLength = config.max_body_chars;
  const bodyTextareaId = makeId("fbm-textarea");
  bodyTextarea.id = bodyTextareaId;
  bodyLabel.htmlFor = bodyTextareaId;
  const counterText = (n: number): string =>
    t("widget.form.counter", { n, max: config.max_body_chars });
  const counter = createElement("span", "fbm-counter", counterText(0));
  bodyField.append(bodyLabel, bodyTextarea, counter);
  bodyTextarea.addEventListener("input", () => {
    counter.textContent = counterText(bodyTextarea.value.length);
  });

  let emailField: HTMLDivElement | null = null;
  let emailInput: HTMLInputElement | null = null;
  if (mode === "anonymous") {
    emailField = createElement("div", "fbm-field");
    const emailLabel = createElement("label", undefined, t("widget.form.email"));
    emailInput = createElement("input");
    emailInput.type = "email";
    emailInput.autocomplete = "email";
    const emailId = makeId("fbm-email");
    emailInput.id = emailId;
    emailLabel.htmlFor = emailId;
    emailField.append(emailLabel, emailInput);
  }

  // Attachments mount point — populated by attachments.ts after the modal
  // is built (kept here so attachment controls live inside the focus trap).
  const attachContainer = createElement("div", "fbm-attach-mount");

  // Diagnostic-log consent. Only rendered when the embedder opted into log
  // capture; the user gives per-submission consent (default on, can opt out).
  let logConsentField: HTMLDivElement | null = null;
  let logConsent: HTMLInputElement | null = null;
  if (logCaptureAvailable) {
    logConsentField = createElement("div", "fbm-field fbm-consent");
    logConsent = createElement("input");
    logConsent.type = "checkbox";
    logConsent.checked = true;
    const consentId = makeId("fbm-logs");
    logConsent.id = consentId;
    const consentLabel = createElement(
      "label",
      "fbm-consent-label",
      t("widget.form.logConsent"),
    );
    consentLabel.htmlFor = consentId;
    logConsentField.append(logConsent, consentLabel);
  }

  const errorRegion = createElement("div", "fbm-error");
  errorRegion.id = errorId;
  errorRegion.setAttribute("role", "alert");
  errorRegion.setAttribute("aria-live", "polite");
  errorRegion.hidden = true;

  const actions = createElement("div", "fbm-actions");
  const cancelBtn = createElement(
    "button",
    "fbm-btn fbm-btn-secondary",
    t("widget.form.cancel"),
  );
  cancelBtn.type = "button";
  cancelBtn.addEventListener("click", onClose);
  const submitBtn = createElement(
    "button",
    "fbm-btn fbm-btn-primary",
    t("widget.form.send"),
  );
  submitBtn.type = "button";
  submitBtn.addEventListener("click", () => {
    void onSubmit();
  });
  actions.append(cancelBtn, submitBtn);

  modal.appendChild(closeBtn);
  if (logoEl) modal.appendChild(logoEl);
  modal.append(titleEl, descEl, subjectField, kindField, bodyField);
  modal.appendChild(attachContainer);
  if (emailField) modal.appendChild(emailField);
  if (logConsentField) modal.appendChild(logConsentField);
  modal.append(errorRegion, actions);

  if (config.brand.footer_text) {
    const footer = createElement("div", "fbm-footer");
    const link = createElement("a", undefined, config.brand.footer_text);
    // Configurable badge href (DEC-FBR-IMPL-11); defaults to the marketing
    // site when the tenant has no override.
    // Scheme-checked (P2-2): a non-http(s) footer_url falls back to the
    // marketing site rather than reaching the href.
    link.href = safeHttpUrl(config.brand.footer_url) || "https://feedbackmonk.com";
    link.target = "_blank";
    link.rel = "noopener noreferrer";
    footer.appendChild(link);
    modal.appendChild(footer);
  }

  scrim.appendChild(modal);

  const focusables: HTMLElement[] = [
    closeBtn,
    subjectInput,
    kindSelect,
    bodyTextarea,
  ];
  if (emailInput) focusables.push(emailInput);
  focusables.push(cancelBtn, submitBtn);

  return {
    scrim,
    modal,
    closeBtn,
    titleEl,
    subjectInput,
    bodyTextarea,
    kindSelect,
    emailInput,
    emailField,
    counter,
    errorRegion,
    submitBtn,
    cancelBtn,
    attachContainer,
    logConsent,
    focusables,
  };
}

/// Apply runtime theming to the widget root (DEC-FBR-IMPL-12).
///   - `theme` ("auto"|"light"|"dark") drives `data-fbm-theme`, which the
///     stylesheet keys the dark token set off (dark explicit, or `auto` under
///     a `prefers-color-scheme: dark` media query).
///   - `primary_color` is applied ONLY when the tenant set one; otherwise the
///     widget keeps its WCAG-AA-safe `#2563eb` CSS default.
export function applyTheme(
  root: HTMLElement,
  config: WidgetConfig,
  theme: WidgetTheme,
): void {
  root.setAttribute("data-fbm-theme", theme);
  // Value-guarded (P2-2): only a plain color token reaches the CSS var.
  const primary = safeCssColor(config.brand.primary_color);
  if (primary) {
    root.style.setProperty("--fbm-primary", primary);
  }
}

/// Map an `ApiError.code` to a catalog key (FR-FBR-35).
///
/// The widget NEVER renders `err.message`. Two reasons, one of which is not
/// about translation at all: (1) server text is English, so a localized widget
/// would suddenly speak two languages in the same dialog; (2) that string is
/// server-authored and reached `textContent` unfiltered.
///
/// `api.ts` synthesises `http_<status>` whenever the response body is not the
/// `{code, message}` shape — which is the common case today, since the API
/// emits `{"error": "..."}` — so the status classes below are the codes that
/// actually reach a user, not a theoretical fallback.
export function errorKey(code: string): string {
  const direct = "widget.error." + code;
  if (hasKey(direct)) return direct;
  const status = /^http_(\d{3})$/.exec(code);
  if (status) {
    const byStatus: Record<string, string> = {
      "401": "unauthorized",
      "402": "tier_cap",
      "403": "unauthorized",
      "404": "not_found",
      "413": "payload_too_large",
      "429": "rate_limited",
    };
    const named = byStatus[status[1]];
    if (named && hasKey("widget.error." + named)) return "widget.error." + named;
    const cls = "widget.error.http_" + status[1][0] + "xx";
    if (hasKey(cls)) return cls;
  }
  return "widget.error.generic";
}

/// Takes the CODE, not the `ApiError` — so "never render the server's message"
/// is enforced by the signature instead of by everyone remembering it.
export function showError(els: ModalElements, code: string): void {
  els.errorRegion.textContent = t(errorKey(code));
  els.errorRegion.hidden = false;
}

export function clearError(els: ModalElements): void {
  els.errorRegion.hidden = true;
  els.errorRegion.textContent = "";
}

export function showToast(
  root: HTMLElement,
  message: string,
  kind: "success" | "error",
): void {
  const toast = createElement(
    "div",
    "fbm-toast fbm-toast-" + kind,
    message,
  );
  toast.setAttribute("role", kind === "error" ? "alert" : "status");
  toast.setAttribute("aria-live", "polite");
  root.appendChild(toast);
  window.setTimeout(() => {
    toast.remove();
  }, 4000);
}
