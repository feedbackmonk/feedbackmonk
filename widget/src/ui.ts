import type {
  ApiError,
  WidgetConfig,
  WidgetSubmissionKind,
  WidgetTheme,
} from "./types.js";

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

export function createRoot(): HTMLDivElement {
  const root = createElement("div", "fbm-root");
  root.setAttribute("data-fbm-root", "");
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
  const label = createElement("span", undefined, "Feedback");
  btn.appendChild(label);
  btn.setAttribute("aria-label", "Open feedback form for " + brandLabel);
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
  closeBtn.setAttribute("aria-label", "Close feedback form");
  closeBtn.textContent = "×";
  closeBtn.addEventListener("click", onClose);

  // Optional per-tenant logo (DEC-FBR-IMPL-12) — rendered in the modal header.
  let logoEl: HTMLImageElement | null = null;
  if (config.brand.logo_url) {
    logoEl = createElement("img", "fbm-logo");
    logoEl.src = config.brand.logo_url;
    logoEl.alt = config.display_name + " logo";
    logoEl.decoding = "async";
    logoEl.loading = "lazy";
  }

  const titleEl = createElement("h2", "fbm-title", "Send feedback");
  titleEl.id = titleId;

  const descEl = createElement(
    "p",
    "fbm-sr-only",
    "Tell us what's on your mind. Submissions are sent to " +
      config.display_name +
      ".",
  );
  descEl.id = bodyId;

  const subjectField = createElement("div", "fbm-field");
  const subjectLabel = createElement("label", undefined, "Subject");
  const subjectInput = createElement("input");
  subjectInput.type = "text";
  subjectInput.required = true;
  subjectInput.maxLength = 200;
  const subjectId = makeId("fbm-subject");
  subjectInput.id = subjectId;
  subjectLabel.htmlFor = subjectId;
  subjectField.append(subjectLabel, subjectInput);

  const kindField = createElement("div", "fbm-field");
  const kindLabel = createElement("label", undefined, "Type");
  const kindSelect = createElement("select");
  const kindId = makeId("fbm-kind");
  kindSelect.id = kindId;
  kindLabel.htmlFor = kindId;
  const kindLabelMap: Record<WidgetSubmissionKind, string> = {
    bug: "Bug",
    feature: "Feature request",
    question: "Question",
    other: "Other",
  };
  for (const kind of config.submission_kinds) {
    const opt = createElement("option");
    opt.value = kind;
    opt.textContent = kindLabelMap[kind] ?? kind;
    kindSelect.appendChild(opt);
  }
  kindField.append(kindLabel, kindSelect);

  const bodyField = createElement("div", "fbm-field");
  const bodyLabel = createElement("label", undefined, "Message");
  const bodyTextarea = createElement("textarea");
  bodyTextarea.required = true;
  bodyTextarea.maxLength = config.max_body_chars;
  const bodyTextareaId = makeId("fbm-textarea");
  bodyTextarea.id = bodyTextareaId;
  bodyLabel.htmlFor = bodyTextareaId;
  const counter = createElement("span", "fbm-counter", "0 / " + config.max_body_chars);
  bodyField.append(bodyLabel, bodyTextarea, counter);
  bodyTextarea.addEventListener("input", () => {
    counter.textContent = bodyTextarea.value.length + " / " + config.max_body_chars;
  });

  let emailField: HTMLDivElement | null = null;
  let emailInput: HTMLInputElement | null = null;
  if (mode === "anonymous") {
    emailField = createElement("div", "fbm-field");
    const emailLabel = createElement("label", undefined, "Email (optional)");
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
      "Include diagnostic logs to help us debug",
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
  const cancelBtn = createElement("button", "fbm-btn fbm-btn-secondary", "Cancel");
  cancelBtn.type = "button";
  cancelBtn.addEventListener("click", onClose);
  const submitBtn = createElement("button", "fbm-btn fbm-btn-primary", "Send");
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
    link.href = config.brand.footer_url || "https://feedbackmonk.com";
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
  if (config.brand.primary_color) {
    root.style.setProperty("--fbm-primary", config.brand.primary_color);
  }
}

export function showError(els: ModalElements, err: ApiError): void {
  els.errorRegion.textContent = err.message + " (" + err.code + ")";
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
