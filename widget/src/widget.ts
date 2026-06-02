import "./styles.css";
import type {
  ApiError,
  MountOptions,
  SubmitFeedbackRequest,
  WidgetConfig,
  WidgetSubmissionKind,
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
import type { CapturedLogs } from "./types.js";

// feedbackmonk widget entry point.
//
// Auto-mounts when loaded as `<script type="module" src=".../widget.js"
//   data-project-id="…">`. Manual: `import { mountFeedbackMonk } from
// "@feedbackmonk/widget"; mountFeedbackMonk({ projectId: "…", jwt: "…" })`.
//
// Load-bearing constraints:
//   - CSP-safe (no inline scripts, no eval, no Function constructor, no
//     dynamic import). Verified by Probe B of widget-bundle-size oracle.
//   - No third-party trackers (DEC-FBR-02). Verified by Probe B.
//   - <30KB bundled (FR-FBR-04). Verified by Probe A.
//   - A11y: keyboard trap inside modal; ESC closes; focus returns to launcher.

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
    showError(els, {
      code: "invalid_input",
      message: "Subject and message are required.",
    });
    return;
  }
  const payload: SubmitFeedbackRequest = { kind, subject, body };
  if (els.emailInput && els.emailInput.value.trim()) {
    payload.email = els.emailInput.value.trim();
  }
  state.submitting = true;
  els.submitBtn.disabled = true;
  els.submitBtn.textContent = "Sending…";
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
      attachOk
        ? "Thanks — your feedback was sent."
        : "Feedback sent — but attachments couldn't be uploaded.",
      attachOk ? "success" : "error",
    );
  } catch (err) {
    const apiErr = err as ApiError;
    if (apiErr && typeof apiErr.code === "string") {
      showError(els, apiErr);
    } else {
      showError(els, {
        code: "network_error",
        message: "Could not send. Try again in a moment.",
      });
    }
  } finally {
    state.submitting = false;
    els.submitBtn.disabled = false;
    els.submitBtn.textContent = "Send";
  }
}

function openModal(state: WidgetState): void {
  if (!state.config || state.modalEls) return;
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

export async function mountFeedbackMonk(
  options: MountOptions = {},
): Promise<void> {
  const opts = resolveCaptureConsole(resolveApiBase(resolveJwt(options)));
  const projectId = resolveProjectId(opts);
  if (!projectId) {
    return;
  }
  // Install console capture eagerly (if opted in) so logs that precede the
  // user opening the modal are still captured. No-op + zero overhead otherwise.
  const consoleLog = opts.captureConsole ? installConsoleCapture() : null;
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
  };
  let config: WidgetConfig;
  try {
    config = await fetchWidgetConfig(projectId, opts);
  } catch {
    // Silently no-op if the project is unknown or backend unreachable.
    // The customer's page should not be impacted by widget errors.
    return;
  }
  state.config = config;
  applyTheme(root, config);
  const launcher = createLauncher(config.display_name, () => openModal(state));
  state.launcher = launcher;
  root.appendChild(launcher);
}

// Auto-mount on script-tag load. Customers who want manual control can pass
// `data-fbm-no-auto-mount` to suppress this.
function autoMount(): void {
  const scripts = document.querySelectorAll<HTMLScriptElement>(
    "script[data-project-id]",
  );
  for (const s of Array.from(scripts)) {
    if (s.hasAttribute("data-fbm-no-auto-mount")) return;
  }
  void mountFeedbackMonk();
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", autoMount, { once: true });
} else {
  autoMount();
}
