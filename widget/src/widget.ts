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
import { fetchWidgetConfig, resolveApiBase, submitFeedback } from "./api.js";
import { boardLink, fetchBoardAccess, type BoardAccess } from "./board.js";
import {
  addBoardLink,
  applyTheme,
  clearError,
  createLauncher,
  createModal,
  createRoot,
  getFocusable,
  showError,
  showSent,
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
//   - Lightweight (FR-FBR-04): the widget-bundle-size oracle reports the English
//     page-load set and each locale chunk on every run (DEC-FBR-IMPL-36: reported,
//     not capped) and fails on a locale chunk that is not only strings (C42).
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
  // `exp` (epoch seconds) of a token fetched from `tokenUrl`; 0 when none.
  tokenExp: number;
  // Set while openModal awaits the token, so a double click opens one modal.
  opening: boolean;
}

// The first non-empty value of a script-tag attribute on the page that `ok`
// accepts. One helper for every `data-*` option, so each option costs a line.
function attr(
  name: string,
  ok: (v: string) => boolean = () => true,
): string | undefined {
  for (const s of Array.from(document.querySelectorAll("script[" + name + "]"))) {
    const v = s.getAttribute(name);
    if (v && ok(v)) return v;
  }
  return undefined;
}

function resolveProjectId(opts: MountOptions): string | null {
  return opts.projectId || attr("data-project-id") || null;
}

/// Fill every option the caller left unset from the script tag. Explicit
/// options always win over attributes.
///   - `data-jwt`: a user token ⇒ auth mode.
///   - `data-token-url`: a same-origin endpoint serving a short-lived user token
///     for whoever is signed in to the host page, so a web page can submit in
///     auth mode without baking into its HTML a token that would expire under
///     a long-open tab.
///   - `data-theme` (DEC-FBR-IMPL-12): falls back to the per-tenant brand
///     default, then "auto". An unknown value is ignored.
///   - `data-fbm-no-auto-mount` on the widget's own tag (DEC-FBR-IMPL-13):
///     initialize WITHOUT the floating launcher; the embedder opens the modal
///     via `[data-feedback-open]` or `window.feedbackmonk.open()`.
///   - `data-locale` (FR-FBR-35, Contract C36): the HOST decides the language —
///     it already knows the user's (a cookie, an account setting, its own
///     `<html lang>`); the widget only follows. Precedence, resolved once:
///       data-locale / MountOptions.locale → host <html lang> → navigator.languages → en
///     The value is resolved through the C34 table before it is used anywhere,
///     so an unshipped or hostile attribute value degrades to `en` and can never
///     reach the `lang` attribute or the `import()` path. One ordered candidate
///     list, resolved by the shared C34 resolver — the same shape
///     `navigator.languages` and `Accept-Language` take, so the widget, the SPA
///     and the Rust crate all answer with one implementation.
function resolveOptions(o: MountOptions): MountOptions {
  return {
    ...o,
    jwt: o.jwt || attr("data-jwt"),
    tokenUrl: o.tokenUrl || attr("data-token-url"),
    apiBase: o.apiBase || attr("data-api-base"),
    boardUrl: o.boardUrl || attr("data-board-url"),
    captureConsole:
      o.captureConsole ??
      (document.querySelector("script[data-capture-console]") ? true : undefined),
    theme:
      o.theme ??
      (attr("data-theme", (v) => v === "auto" || v === "light" || v === "dark") as
        | WidgetTheme
        | undefined),
    noLauncher:
      o.noLauncher ??
      (document.querySelector("script[data-project-id][data-fbm-no-auto-mount]")
        ? true
        : undefined),
    locale: resolveLocale([
      o.locale || attr("data-locale"),
      document.documentElement.getAttribute("lang"),
      ...(navigator.languages ?? []),
    ]),
  };
}

/// The `email` claim of a user token, for display only — the server verifies
/// the token; the widget just shows the person where a reply will go.
export function jwtEmail(jwt: string | undefined): string | null {
  try {
    const part = jwt!.split(".")[1].replace(/-/g, "+").replace(/_/g, "/");
    const email = JSON.parse(atob(part)).email;
    return typeof email === "string" && email ? email : null;
  } catch {
    return null;
  }
}

/// Fetch a fresh token from `tokenUrl` unless the one held has 30 s left. Any
/// answer without a token — signed out, an error, the network — leaves the
/// widget in anonymous mode rather than failing: feedback must stay sendable.
async function refreshToken(state: WidgetState): Promise<void> {
  const url = state.opts.tokenUrl;
  if (!url || state.tokenExp > Date.now() / 1000 + 30) return;
  let token: string | undefined;
  let exp = 0;
  try {
    const r = await fetch(url, {
      headers: { Accept: "application/json" },
      credentials: "same-origin",
    });
    if (r.ok) {
      const d = (await r.json()) as { token?: unknown; expires_at?: unknown };
      if (typeof d.token === "string" && d.token) {
        token = d.token;
        exp = typeof d.expires_at === "number" ? d.expires_at : 0;
      }
    }
  } catch {
    // anonymous it is
  }
  state.opts = { ...state.opts, jwt: token };
  state.tokenExp = exp;
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
  const body = els.bodyTextarea.value.trim();
  if (!body) {
    showError(els, "invalid_input");
    return;
  }
  // The API requires a subject; the form asks for one message, so the subject
  // is its first line, cut to a length a triage list can show.
  const first = body.split("\n")[0].trim();
  const subject = first.length > 120 ? first.slice(0, 119) + "…" : first;
  // C37: the resolved locale rides along with every submission so status
  // emails can answer in the language the person wrote in. Always a C34 code
  // (never raw input), and an unknown value is ignored server-side rather than
  // rejected — a locale must never be able to break a submission.
  const payload: SubmitFeedbackRequest = {
    kind: els.kind() as WidgetSubmissionKind,
    subject,
    body,
    locale: activeLocale(),
  };
  const email = els.emailInput ? els.emailInput.value.trim() : "";
  if (email) payload.email = email;
  state.submitting = true;
  els.submitBtn.disabled = true;
  els.submitBtn.textContent = t("widget.form.sending");
  try {
    // A token fetched from `tokenUrl` may have expired while the form was
    // open; a fresh one is fetched here, never a stale one sent.
    if (state.opts.tokenUrl) await refreshToken(state);
    // "Send anonymously" means no token on the wire at all — not a token the
    // server is asked to ignore.
    const anon = els.anonToggle ? els.anonToggle.checked : false;
    const opts: MountOptions = anon ? { ...state.opts, jwt: undefined } : state.opts;
    const resp = await submitFeedback(state.projectId, payload, opts);
    // Gather attachments + consented logs BEFORE the confirmation replaces the
    // form. Blobs stay valid after URL revocation.
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
        await uploadAttachments(state.projectId, resp.feedback_id, inputs, logs, opts);
      } catch {
        // Soft failure: the feedback itself is already recorded. Never lose
        // the submission because an attachment upload failed.
        attachOk = false;
      }
    }
    const replyTo = anon ? null : email || jwtEmail(opts.jwt);
    const lines = [
      t(attachOk ? "widget.status.sent" : "widget.status.sentAttachmentsFailed"),
    ];
    if (replyTo) lines.push(t("widget.sent.replyTo", { email: replyTo }));
    if (state.attachments) {
      state.attachments.destroy();
      state.attachments = null;
    }
    showSent(els, lines, () => closeModal(state));
  } catch (err) {
    const apiErr = err as ApiError;
    showError(
      els,
      apiErr && typeof apiErr.code === "string" ? apiErr.code : "network_error",
    );
    els.submitBtn.disabled = false;
  } finally {
    state.submitting = false;
    els.submitBtn.textContent = t("widget.form.send");
  }
}

async function openModal(state: WidgetState): Promise<void> {
  if (!state.config || state.modalEls || state.destroyed || state.opening) return;
  state.opening = true;
  // The board probe runs alongside the token fetch and never delays the form:
  // the link joins the open modal when the answer arrives (FR-FBR-45).
  const access = fetchBoardAccess(state.projectId, state.opts);
  try {
    await refreshToken(state);
  } finally {
    state.opening = false;
  }
  if (state.modalEls || state.destroyed) return;
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
    jwtEmail(state.opts.jwt),
  );
  state.modalEls = els;
  // Mount the attachments UI inside the modal (within the focus trap).
  const attachments = createAttachments(state.root);
  els.attachContainer.appendChild(attachments.element);
  state.attachments = attachments;
  // An image pasted or dropped anywhere in the form is an attachment.
  els.modal.addEventListener("paste", (e) => {
    const files = e.clipboardData && e.clipboardData.files;
    if (files && files.length) {
      e.preventDefault();
      attachments.add(files);
    }
  });
  els.modal.addEventListener("dragover", (e) => {
    if (e.dataTransfer && e.dataTransfer.types.includes("Files")) e.preventDefault();
  });
  els.modal.addEventListener("drop", (e) => {
    if (e.dataTransfer && e.dataTransfer.files.length) {
      e.preventDefault();
      attachments.add(e.dataTransfer.files);
    }
  });
  state.root.appendChild(els.scrim);
  // Initial focus on the kind chips, which come first in the form.
  window.setTimeout(() => {
    if (els.firstKind) els.firstKind.focus();
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
  void showBoardLink(state, els, access);
}

/// Add the "See all requests" link (FR-FBR-45) once the board probe answers,
/// if this modal is still the open one and `boardLink` says to show it: an
/// enabled board, and a token unless the board is public. A token holder's
/// link carries the token in the fragment of the board's sign-in page.
async function showBoardLink(
  state: WidgetState,
  els: ModalElements,
  pending: Promise<BoardAccess | null>,
): Promise<void> {
  const access = await pending;
  const href = (): string | null =>
    boardLink(
      state.opts.boardUrl || resolveApiBase(state.opts),
      state.projectId,
      access,
      state.opts.jwt,
    );
  const first = href();
  if (!first || state.modalEls !== els) return;
  addBoardLink(els, first, (e, link) => {
    // The handoff needs a token the exchange will accept. One from
    // `data-token-url` may have expired while the form sat open: open the tab
    // now, inside the click (or a popup blocker eats it), fetch a fresh token,
    // then send the tab on.
    if (state.opts.tokenUrl && state.tokenExp <= Date.now() / 1000 + 30) {
      e.preventDefault();
      const w = window.open("", "_blank");
      void refreshToken(state).then(() => {
        if (!w) return;
        w.opener = null;
        const h = href();
        if (h) w.location.href = h;
        else w.close();
      });
      return;
    }
    const h = href();
    if (h) link.href = h;
    else e.preventDefault();
  });
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
  const opts = resolveOptions(options);
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
    handleOpen: () => void openModal(state),
    tokenExp: 0,
    opening: false,
  };
  state.config = config;
  // Theme precedence: explicit option / data-theme → per-tenant brand default
  // → "auto" (DEC-FBR-IMPL-12).
  const theme: WidgetTheme = opts.theme ?? config.brand.theme ?? "auto";
  applyTheme(root, config, theme);

  // Floating launcher — unless the embedder opted out (DEC-FBR-IMPL-13).
  if (!opts.noLauncher) {
    const launcher = createLauncher(config.display_name, state.handleOpen);
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
      state.handleOpen();
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
