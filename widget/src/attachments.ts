import type {
  ApiError,
  AttachmentInput,
  CapturedLogs,
  MountOptions,
} from "./types.js";
import { readError, resolveApiBase } from "./api.js";
import { createElement } from "./ui.js";

// Attachment capture + upload for the feedbackmonk widget.
//
// Responsibilities:
//   - Stage up to 4 user-attached images (≤5MB each, PNG/JPEG/WebP) with
//     preview thumbnails, a per-image canvas-redaction entry point, and remove.
//   - Capture console (+ optional host-exposed service) logs into bounded
//     buffers — opt-in only (DEC-FBR-02 privacy-by-default).
//   - Upload everything as multipart/form-data per the GUIDE §6 frozen
//     contract: `POST …/feedback/:fb/attachments`, field `files[]` (≤4) plus
//     `service_log`/`console_log` text parts. Logs are sent RAW; the server
//     (CLAUDE-ALPHA1) scrubs PII through the canonical chokepoint.
//
// Load-bearing constraints (mirror widget.ts):
//   - CSP-safe: no eval, no Function, no inline. The redaction canvas is a
//     SAME-ORIGIN dynamic import (`./redact.js`) — covered by the embedder's
//     existing `script-src` for the CDN origin, so it needs no policy change.
//   - Bundle budget: this file is in the base bundle; the heavy canvas editor
//     is code-split into `redact.ts` and only fetched when the user redacts.

export const MAX_FILES = 4;
export const MAX_BYTES = 5 * 1024 * 1024;
export const ALLOWED_TYPES = ["image/png", "image/jpeg", "image/webp"];

// ---------------------------------------------------------------------------
// Log capture (opt-in). Patches console.* into a bounded ring buffer. Reads
// console methods via computed member access and calls through a captured
// reference so terser's `drop_console` (which strips literal `console.x(...)`
// call expressions) cannot remove the passthrough.
// ---------------------------------------------------------------------------

const CONSOLE_MAX_LINES = 200;
const CONSOLE_MAX_LINE = 2000;
let consoleGetter: (() => string) | null = null;

function fmtArg(arg: unknown): string {
  if (typeof arg === "string") return arg;
  if (arg instanceof Error) return arg.name + ": " + arg.message;
  try {
    return JSON.stringify(arg);
  } catch {
    return String(arg);
  }
}

export function installConsoleCapture(): () => string {
  if (consoleGetter) return consoleGetter;
  const buf: string[] = [];
  const methods = ["log", "info", "warn", "error", "debug"] as const;
  for (const m of methods) {
    const orig = console[m] as (...a: unknown[]) => void;
    if (typeof orig !== "function") continue;
    console[m] = function (...args: unknown[]): void {
      try {
        let line = "[" + m + "] " + args.map(fmtArg).join(" ");
        if (line.length > CONSOLE_MAX_LINE) {
          line = line.slice(0, CONSOLE_MAX_LINE) + "…";
        }
        buf.push(line);
        if (buf.length > CONSOLE_MAX_LINES) buf.shift();
      } catch {
        // Never let capture break the host page's console.
      }
      return orig.apply(console, args);
    } as typeof console[typeof m];
  }
  consoleGetter = () => buf.join("\n");
  return consoleGetter;
}

// Optional host-page hook for app/service logs. The embedder may set
// `window.__feedbackmonkServiceLog` to a string or a `() => string`.
export function getServiceLog(): string | undefined {
  try {
    const hook = (window as unknown as Record<string, unknown>)[
      "__feedbackmonkServiceLog"
    ];
    if (typeof hook === "string") return hook || undefined;
    if (typeof hook === "function") {
      const v = (hook as () => unknown)();
      return typeof v === "string" && v ? v : undefined;
    }
  } catch {
    // ignore — service log is best-effort.
  }
  return undefined;
}

// ---------------------------------------------------------------------------
// Attach UI controller
// ---------------------------------------------------------------------------

export interface AttachmentsController {
  element: HTMLDivElement; // field container to insert into the modal
  getInputs(): AttachmentInput[];
  count(): number;
  destroy(): void; // revoke all object URLs
}

interface Item {
  id: string;
  file: Blob;
  name: string;
  url: string;
  redacted: boolean;
  li: HTMLLIElement;
  thumb: HTMLImageElement;
  nameEl: HTMLSpanElement;
  redactBtn: HTMLButtonElement;
}

function humanSize(bytes: number): string {
  if (bytes < 1024) return bytes + " B";
  if (bytes < 1024 * 1024) return Math.round(bytes / 1024) + " KB";
  return (bytes / (1024 * 1024)).toFixed(1) + " MB";
}

function baseName(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(0, dot) : name;
}

function validate(file: File): string | null {
  if (!ALLOWED_TYPES.includes(file.type)) {
    return "Only PNG, JPEG, or WebP images can be attached.";
  }
  if (file.size > MAX_BYTES) {
    return "Each image must be 5 MB or smaller.";
  }
  return null;
}

export function createAttachments(root: HTMLElement): AttachmentsController {
  const items: Item[] = [];

  const container = createElement("div", "fbm-field fbm-attach");
  const label = createElement("span", "fbm-attach-label", "Screenshots (optional)");

  const helpId = "fbm-attach-help-" + Math.random().toString(36).slice(2, 8);
  const help = createElement(
    "span",
    "fbm-attach-help",
    "PNG, JPEG, or WebP · up to 4 · 5 MB each. Redact sensitive areas before sending.",
  );
  help.id = helpId;

  const input = createElement("input", "fbm-sr-only");
  input.type = "file";
  input.accept = ALLOWED_TYPES.join(",");
  input.multiple = true;
  input.tabIndex = -1;
  input.setAttribute("aria-hidden", "true");

  const attachBtn = createElement(
    "button",
    "fbm-btn fbm-btn-secondary fbm-attach-btn",
    "Attach screenshot",
  );
  attachBtn.type = "button";
  attachBtn.setAttribute("aria-describedby", helpId);
  attachBtn.addEventListener("click", () => input.click());

  const errorRegion = createElement("div", "fbm-attach-error");
  errorRegion.setAttribute("role", "alert");
  errorRegion.setAttribute("aria-live", "polite");
  errorRegion.hidden = true;

  const list = createElement("ul", "fbm-attach-list");
  list.setAttribute("aria-label", "Attached screenshots");

  container.append(label, help, attachBtn, input, errorRegion, list);

  function setError(msg: string | null): void {
    if (msg) {
      errorRegion.textContent = msg;
      errorRegion.hidden = false;
    } else {
      errorRegion.textContent = "";
      errorRegion.hidden = true;
    }
  }

  function refreshAttachState(): void {
    const full = items.length >= MAX_FILES;
    attachBtn.disabled = full;
    attachBtn.textContent = full
      ? "Maximum 4 screenshots"
      : "Attach screenshot";
  }

  function removeItem(it: Item): void {
    const idx = items.indexOf(it);
    if (idx === -1) return;
    URL.revokeObjectURL(it.url);
    it.li.remove();
    items.splice(idx, 1);
    refreshAttachState();
    setError(null);
    attachBtn.focus();
  }

  async function redactItem(it: Item): Promise<void> {
    it.redactBtn.disabled = true;
    const prev = it.redactBtn.textContent;
    it.redactBtn.textContent = "Loading…";
    try {
      // Same-origin code-split chunk — only fetched on first redact.
      const mod = await import("./redact.js");
      const result = await mod.redactImage(it.file, root);
      if (result) {
        URL.revokeObjectURL(it.url);
        it.file = result;
        it.name = baseName(it.name) + ".redacted.png";
        it.url = URL.createObjectURL(result);
        it.thumb.src = it.url;
        it.redacted = true;
        it.nameEl.textContent = it.name + " · redacted";
        it.redactBtn.setAttribute("aria-label", "Re-redact " + it.name);
      }
    } catch {
      setError("The redaction tool could not be loaded. You can still send the image as-is.");
    } finally {
      it.redactBtn.disabled = false;
      it.redactBtn.textContent = prev || "Redact";
    }
  }

  function addItem(file: File): void {
    const id = "att-" + Math.random().toString(36).slice(2, 10);
    const url = URL.createObjectURL(file);

    const li = createElement("li", "fbm-attach-item");

    const thumb = createElement("img", "fbm-thumb");
    thumb.src = url;
    thumb.alt = "Preview of " + file.name;

    const meta = createElement("div", "fbm-attach-meta");
    const nameEl = createElement(
      "span",
      "fbm-attach-name",
      file.name + " (" + humanSize(file.size) + ")",
    );
    meta.appendChild(nameEl);

    const controls = createElement("div", "fbm-attach-controls");
    const redactBtn = createElement(
      "button",
      "fbm-btn fbm-btn-secondary fbm-attach-mini",
      "Redact",
    );
    redactBtn.type = "button";
    redactBtn.setAttribute("aria-label", "Redact " + file.name);

    const removeBtn = createElement(
      "button",
      "fbm-btn fbm-btn-secondary fbm-attach-mini",
      "Remove",
    );
    removeBtn.type = "button";
    removeBtn.setAttribute("aria-label", "Remove " + file.name);

    controls.append(redactBtn, removeBtn);
    li.append(thumb, meta, controls);
    list.appendChild(li);

    const it: Item = {
      id,
      file,
      name: file.name,
      url,
      redacted: false,
      li,
      thumb,
      nameEl,
      redactBtn,
    };
    redactBtn.addEventListener("click", () => void redactItem(it));
    removeBtn.addEventListener("click", () => removeItem(it));
    items.push(it);
  }

  function addFiles(files: FileList | null): void {
    if (!files || files.length === 0) return;
    setError(null);
    for (const file of Array.from(files)) {
      if (items.length >= MAX_FILES) {
        setError("You can attach at most 4 screenshots.");
        break;
      }
      const err = validate(file);
      if (err) {
        setError(err);
        continue;
      }
      addItem(file);
    }
    refreshAttachState();
    input.value = ""; // allow re-selecting the same file
  }

  input.addEventListener("change", () => addFiles(input.files));

  return {
    element: container,
    getInputs: () => items.map((i) => ({ file: i.file, name: i.name })),
    count: () => items.length,
    destroy: () => {
      for (const i of items) URL.revokeObjectURL(i.url);
    },
  };
}

// ---------------------------------------------------------------------------
// Upload (multipart). One request, AFTER the feedback row exists.
// ---------------------------------------------------------------------------

export async function uploadAttachments(
  projectId: string,
  feedbackId: string,
  inputs: AttachmentInput[],
  logs: CapturedLogs,
  opts: MountOptions,
): Promise<void> {
  const hasFiles = inputs.length > 0;
  const hasLogs = !!(logs.service_log || logs.console_log);
  if (!hasFiles && !hasLogs) return;

  const form = new FormData();
  for (const input of inputs) {
    form.append("files[]", input.file, input.name);
  }
  if (logs.service_log) form.append("service_log", logs.service_log);
  if (logs.console_log) form.append("console_log", logs.console_log);

  const url =
    resolveApiBase(opts) +
    "/api/v1/projects/" +
    encodeURIComponent(projectId) +
    "/feedback/" +
    encodeURIComponent(feedbackId) +
    "/attachments";

  // No explicit Content-Type — the browser sets the multipart boundary.
  const headers: Record<string, string> = {};
  if (opts.jwt) headers["Authorization"] = "Bearer " + opts.jwt;

  const response = await fetch(url, {
    method: "POST",
    headers,
    body: form,
    credentials: opts.jwt ? "omit" : "include",
  });
  if (!response.ok) {
    const err: ApiError = await readError(response);
    throw err;
  }
}
