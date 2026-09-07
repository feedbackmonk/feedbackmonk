import "./styles.css";
import type {
  ApiError,
  FeedbackMonkHandle,
  MountOptions,
  SubmitFeedbackRequest,
  WidgetConfig,
  WidgetSubmissionKind,
  WidgetTheme,
} from "./types.js";
import { fetchWidgetConfig, submitFeedback } from "./api.js";
import {
  applyTheme,
  clearError,
  createLauncher,
  createModal,
  createRoot,
  getFocusable,
  showError,
  showToast,
  type ModalElements,
} from "./ui.js";
import {
  createAttachments,
  getServiceLog,
  installConsoleCapture,
  uploadAttachments,
  type AttachmentsController,
} from "./attachments.js";
import { activeLocale, loadLocale, resolveLocale, t } from "./i18n.js";
import type { CapturedLogs } from "./types.js";

// feedbackmonk widget entry point.
//
// Auto-mounts when loaded as `<script type="module" src=".../widget.js"
//   data-project-id="…">`. Manual: `import { mountFeedbackMonk } from
// "@feedbackmonk/widget"; mountFeedbackMonk({ projectId: "…", jwt: "…" })`.
//
// Load-bearing constraints:
//   - CSP-safe: no inline scripts, no eval, no Function constructor. Dynamic
//     `import()` IS used — for `redact.js` and for the per-locale catalog
//     chunks — but only same-origin and only from a generated list of literal
//     specifiers, which `script-src 'self'` permits (proved on every commit by
//     e2e/fixture-csp.html). No computed specifier ever reaches `import()`.
//   - No third-party trackers (DEC-FBR-02). Verified by Probe B.
//   - English page-load set <30KB (FR-FBR-04); each locale chunk <4KB (C42).
//     Verified by Probes A and C of the widget-bundle-size oracle.
//   - A11y: keyboard trap inside modal; ESC closes; focus returns to launcher.
//   - The widget speaks the host page's language (FR-FBR-35) and declares it:
//     `lang` + `dir` on its own root, never inherited from the host.

interface WidgetState {
  root: HTMLDivElement;
  launcher: HTMLButtonElement | null;
  modalEls: ModalElements | null;
  config: WidgetConfig | null;
  opts: MountOptions;
  projectId: string;
  previousFocus: HTMLElement | null;
  escListener: ((e: KeyboardEvent) => void) | null;
  trapListener: ((e: KeyboardEvent) => void) | null;
  submitting: boolean;
  // Console-log getter, installed at mount only if capture is opted in.
  consoleLog: (() => string) | null;
  // Per-modal attachments controller; null while the modal is closed.
  attachments: AttachmentsController | null;
  // Document-level click delegation that opens the modal from any
  // `[data-feedback-open]` element (DEC-FBR-IMPL-13). Removed on destroy.
  openDelegationListener: ((e: MouseEvent) => void) | null;
  // Set by `destroy()`; guards `open()` against re-opening a torn-down widget.
  destroyed: boolean;
  // Stable open() reference exposed via the handle / window.feedbackmonk;
  // captured so destroy() can clear the global only if it still points here.
  handleOpen: () => void;
}

function resolveProjectId(opts: MountOptions): string | null {
  if (opts.projectId) return opts.projectId;
  const scripts = document.querySelectorAll<HTMLScriptElement>(
    'script[data-project-id]',
  );
  for (const s of Array.from(scripts)) {
    const id = s.getAttribute("data-project-id");
    if (id) return id;
  }
  return null;
}

function resolveJwt(opts: MountOptions): MountOptions {
  if (opts.jwt) return opts;
  const scripts = document.querySelectorAll<HTMLScriptElement>(
    "script[data-jwt]",
  );
  for (const s of Array.from(scripts)) {
    const t = s.getAttribute("data-jwt");
    if (t) return { ...opts, jwt: t };
  }
  return opts;
}

function resolveApiBase(opts: MountOptions): MountOptions {
  if (opts.apiBase) return opts;
  const scripts = document.querySelectorAll<HTMLScriptElement>(
    "script[data-api-base]",
  );
  for (const s of Array.from(scripts)) {
    const a = s.getAttribute("data-api-base");
    if (a) return { ...opts, apiBase: a };
  }
  return opts;
}

function resolveCaptureConsole(opts: MountOptions): MountOptions {
  if (opts.captureConsole !== undefined) return opts;
  const scripts = document.querySelectorAll<HTMLScriptElement>(
    "script[data-capture-console]",
  );
  for (const s of Array.from(scripts)) {
    if (s.hasAttribute("data-capture-console")) {
      return { ...opts, captureConsole: true };
    }
  }
  return opts;
}

// Theme override from the script tag's `data-theme` attribute (DEC-FBR-IMPL-12).
// Highest precedence; falls back to the per-tenant brand default then "auto".
function resolveTheme(opts: MountOptions): MountOptions {
  if (opts.theme) return opts;
  const scripts = document.querySelectorAll<HTMLScriptElement>(
    "script[data-theme]",
  );
  for (const s of Array.from(scripts)) {
    const t = s.getAttribute("data-theme");
    if (t === "auto" || t === "light" || t === "dark") {
      return { ...opts, theme: t };
    }
  }
  return opts;
}

// UI language from the script tag's `data-locale` attribute (FR-FBR-35,
// Contract C36). The HOST decides the language — it already knows the user's
// (a cookie, an account setting, its own `<html lang>`); the widget only
// follows. Precedence, resolved once at mount:
//   data-locale / MountOptions.locale → host <html lang> → navigator.languages → en
// The value is resolved through the C34 table before it is used anywhere, so
// an unshipped or hostile attribute value degrades to `en` and can never reach
// the `lang` attribute or the `import()` path.
function resolveLocaleOption(opts: MountOptions): MountOptions {
  let explicit = opts.locale;
  if (!explicit) {
    const scripts = document.querySelectorAll<HTMLScriptElement>(
      "script[data-locale]",
    );
    for (const s of Array.from(scripts)) {
      const l = s.getAttribute("data-locale");
      if (l) {
        explicit = l;
        break;
      }
    }
  }
  // One ordered candidate list, resolved by the shared C34 resolver — the same
  // shape `navigator.languages` and `Accept-Language` take, so the widget, the
  // SPA and the Rust crate all answer with one implementation.
  return {
    ...opts,
    locale: resolveLocale([
      explicit,
      document.documentElement.getAttribute("lang"),
      ...(navigator.languages ?? []),
    ]),
  };
}

// Launcher-less mode from the script tag's `data-fbm-no-auto-mount` attribute
// (DEC-FBR-IMPL-13). When set, the widget initializes WITHOUT the floating
// launcher; the embedder opens the modal via `[data-feedback-open]` or
// `window.feedbackmonk.open()`.
function resolveNoLauncher(opts: MountOptions): MountOptions {
  if (opts.noLauncher !== undefined) return opts;
  const scripts = document.querySelectorAll<HTMLScriptElement>(
    "script[data-project-id]",
  );
  for (const s of Array.from(scripts)) {
    if (s.hasAttribute("data-fbm-no-auto-mount")) {
      return { ...opts, noLauncher: true };
    }
  }
  return opts;
}

function trapFocus(state: WidgetState, e: KeyboardEvent): void {
  if (!state.modalEls) return;
  if (e.key !== "Tab") return;
  // Live query so dynamically-added attachment controls are included in the
  // trap boundary (static `focusables` would miss them).
  const focusables = getFocusable(state.modalEls.modal);
  if (focusables.length === 0) return;
  const first = focusables[0];
  const last = focusables[focusables.length - 1];
  const active = document.activeElement as HTMLElement | null;
  if (e.shiftKey) {
    if (active === first || !state.modalEls.modal.contains(active)) {
      e.preventDefault();
      last.focus();
    }
  } else {
    if (active === last) {
      e.preventDefault();
      first.focus();
    }
  }
}

function closeModal(state: WidgetState): void {
  if (!state.modalEls) return;
  if (state.attachments) {
    state.attachments.destroy();
    state.attachments = null;
  }
  state.modalEls.scrim.remove();
  state.modalEls = null;
  if (state.escListener) {
    document.removeEventListener("keydown", state.escListener, true);
    state.escListener = null;
  }
  if (state.trapListener) {
    document.removeEventListener("keydown", state.trapListener, true);
    state.trapListener = null;
  }
  if (state.previousFocus && document.body.contains(state.previousFocus)) {
    state.previousFocus.focus();
  } else if (state.launcher) {
    state.launcher.focus();
  }
}

async function performSubmit(state: WidgetState): Promise<void> {
  if (!state.modalEls || !state.config || state.submitting) return;
  const els = state.modalEls;
  clearError(els);
  const subject = els.subjectInput.value.trim();
  const body = els.bodyTextarea.value.trim();
  const kind = els.kindSelect.value as WidgetSubmissionKind;
  if (!subject || !body) {
    showError(els, "invalid_input");
    return;
  }
  // C37: the resolved locale rides along with every submission so status
  // emails can answer in the language the person wrote in. Always a C34 code
  // (never raw input), and an unknown value is ignored server-side rather than
  // rejected — a locale must never be able to break a submission.
  const payload: SubmitFeedbackRequest = {
    kind,
    subject,
    body,
    locale: activeLocale(),
  };
  if (els.emailInput && els.emailInput.value.trim()) {
    payload.email = els.emailInput.value.trim();
  }
  state.submitting = true;
  els.submitBtn.disabled = true;
  els.submitBtn.textContent = t("widget.form.sending");
  try {
    const resp = await submitFeedback(state.projectId, payload, state.opts);
    // Gather attachments + consented logs BEFORE closeModal (which destroys
    // the attachments controller). Blobs stay valid after URL revocation.
    const inputs = state.attachments ? state.attachments.getInputs() : [];
    const wantLogs = els.logConsent ? els.logConsent.checked : false;
    let attachOk = true;
    if (inputs.length > 0 || wantLogs) {
      const logs: CapturedLogs = {};
      if (wantLogs) {
        if (state.consoleLog) logs.console_log = state.consoleLog();
        const svc = getServiceLog();
        if (svc) logs.service_log = svc;
      }
      try {
        await uploadAttachments(
          state.projectId,
          resp.feedback_id,
          inputs,
          logs,
          state.opts,
        );
      } catch {
        // Soft failure: the feedback itself is already recorded. Never lose
        // the submission because an attachment upload failed.
        attachOk = false;
      }
    }
    closeModal(state);
    showToast(
      state.root,
      t(attachOk ? "widget.status.sent" : "widget.status.sentAttachmentsFailed"),
      attachOk ? "success" : "error",
    );
  } catch (err) {
    const apiErr = err as ApiError;
    showError(
      els,
      apiErr && typeof apiErr.code === "string" ? apiErr.code : "network_error",
    );
  } finally {
    state.submitting = false;
    els.submitBtn.disabled = false;
    els.submitBtn.textContent = t("widget.form.send");
  }
}

function openModal(state: WidgetState): void {
  if (!state.config || state.modalEls || state.destroyed) return;
  const mode: "auth" | "anonymous" = state.opts.jwt ? "auth" : "anonymous";
  state.previousFocus = document.activeElement as HTMLElement | null;
  const logCaptureAvailable =
    state.consoleLog !== null || getServiceLog() !== undefined;
  const els = createModal(
    state.config,
    mode,
    () => performSubmit(state),
    () => closeModal(state),
    logCaptureAvailable,
  );
  state.modalEls = els;
  // Mount the attachments UI inside the modal (within the focus trap).
  const attachments = createAttachments(state.root);
  els.attachContainer.appendChild(attachments.element);
  state.attachments = attachments;
  state.root.appendChild(els.scrim);
  // initial focus on the subject input
  window.setTimeout(() => {
    els.subjectInput.focus();
  }, 0);

  const escListener = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      closeModal(state);
    }
  };
  const trapListener = (e: KeyboardEvent) => trapFocus(state, e);
  document.addEventListener("keydown", escListener, true);
  document.addEventListener("keydown", trapListener, true);
  state.escListener = escListener;
  state.trapListener = trapListener;
}

/// Tear the widget down: close the modal, remove the document-level trigger
/// delegation, and detach the root. After this, `open()` is a no-op.
function destroyWidget(state: WidgetState): void {
  closeModal(state);
  if (state.openDelegationListener) {
    document.removeEventListener("click", state.openDelegationListener);
    state.openDelegationListener = null;
  }
  state.destroyed = true;
  state.root.remove();
  const w = window as unknown as { feedbackmonk?: FeedbackMonkHandle };
  if (w.feedbackmonk && w.feedbackmonk.open === state.handleOpen) {
    delete w.feedbackmonk;
  }
}

export async function mountFeedbackMonk(
  options: MountOptions = {},
): Promise<FeedbackMonkHandle | undefined> {
  const opts = resolveLocaleOption(
    resolveNoLauncher(
      resolveTheme(resolveCaptureConsole(resolveApiBase(resolveJwt(options)))),
    ),
  );
  const projectId = resolveProjectId(opts);
  if (!projectId) {
    return undefined;
  }
  // Start the catalog chunk NOW, in parallel with the config fetch below, and
  // await both together: nothing is rendered before the config arrives anyway,
  // so the locale costs max(config, chunk) rather than config + chunk. A chunk
  // that fails or 404s leaves the widget in English (loadLocale never throws).
  const localeReady = loadLocale(opts.locale ?? "en");
  // Install console capture eagerly (if opted in) so logs that precede the
  // user opening the modal are still captured. No-op + zero overhead otherwise.
  const consoleLog = opts.captureConsole ? installConsoleCapture() : null;
  let config: WidgetConfig;
  try {
    const [fetched] = await Promise.all([
      fetchWidgetConfig(projectId, opts),
      localeReady,
    ]);
    config = fetched;
  } catch {
    // Silently no-op if the project is unknown or backend unreachable.
    // The customer's page should not be impacted by widget errors.
    return undefined;
  }
  // Root is created AFTER the locale settles so `lang`/`dir` are right on the
  // first paint — no flash of a mis-declared language.
  const root = createRoot();
  document.body.appendChild(root);
  const state: WidgetState = {
    root,
    launcher: null,
    modalEls: null,
    config: null,
    opts,
    projectId,
    previousFocus: null,
    escListener: null,
    trapListener: null,
    submitting: false,
    consoleLog,
    attachments: null,
    openDelegationListener: null,
    destroyed: false,
    handleOpen: () => openModal(state),
  };
  state.config = config;
  // Theme precedence: explicit option / data-theme → per-tenant brand default
  // → "auto" (DEC-FBR-IMPL-12).
  const theme: WidgetTheme = opts.theme ?? config.brand.theme ?? "auto";
  applyTheme(root, config, theme);

  // Floating launcher — unless the embedder opted out (DEC-FBR-IMPL-13).
  if (!opts.noLauncher) {
    const launcher = createLauncher(config.display_name, () => openModal(state));
    state.launcher = launcher;
    root.appendChild(launcher);
  }

  // Auto-wire any `[data-feedback-open]` element via a single document-level
  // click-delegation listener — robust to dynamically-added triggers, and the
  // host needs no JS glue (DEC-FBR-IMPL-13).
  const delegation = (e: MouseEvent): void => {
    const target = e.target as Element | null;
    if (target && target.closest("[data-feedback-open]")) {
      e.preventDefault();
      openModal(state);
    }
  };
  document.addEventListener("click", delegation);
  state.openDelegationListener = delegation;

  // Expose the programmatic handle. Single widget per page is the norm; if a
  // page mounts more than once, the last mount wins the global.
  const handle: FeedbackMonkHandle = {
    open: state.handleOpen,
    destroy: () => destroyWidget(state),
  };
  (window as unknown as { feedbackmonk?: FeedbackMonkHandle }).feedbackmonk =
    handle;
  return handle;
}

// Auto-mount on script-tag load. `data-fbm-no-auto-mount` no longer suppresses
// mounting — it now initializes launcher-less (resolveNoLauncher reads it), so
// the embedder's own `[data-feedback-open]` / `window.feedbackmonk.open()` can
// drive the modal (DEC-FBR-IMPL-13). Manual ESM importers have no
// `script[data-project-id]` tag, so this still no-ops for them.
function autoMount(): void {
  const scripts = document.querySelectorAll<HTMLScriptElement>(
    "script[data-project-id]",
  );
  if (scripts.length === 0) return;
  void mountFeedbackMonk();
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", autoMount, { once: true });
} else {
  autoMount();
}
