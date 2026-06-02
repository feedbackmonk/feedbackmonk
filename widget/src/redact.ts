// Canvas redaction tool for the feedbackmonk widget. CODE-SPLIT CHUNK —
// dynamically imported by attachments.ts only when the user clicks "Redact",
// so it is fetched at runtime only if actually used.
//
// SELF-CONTAINED on purpose: it imports nothing from the base modules so
// Rollup keeps the entry (`widget.js`) whole instead of hoisting a shared
// chunk + stub. The two tiny helpers below are the only duplication.
//
// The user draws opaque rectangles over sensitive regions; "Apply" exports a
// flattened PNG with those regions permanently blacked out. CSP-safe (pure
// canvas 2D + DOM; no eval/Function/inline). A11y: focus-trapped role="dialog",
// keyboard-operable controls, ESC cancels. Rectangle DRAWING is pointer-driven
// (fine-grained region selection is inherently a pointing gesture) — a
// documented limitation; every control around it is keyboard-reachable.

interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

function ce<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  cls?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  if (cls) el.className = cls;
  if (text !== undefined) el.textContent = text;
  return el;
}

function focusables(c: HTMLElement): HTMLElement[] {
  return Array.from(
    c.querySelectorAll<HTMLElement>("button:not([disabled])"),
  ).filter((el) => !el.hidden);
}

function btn(cls: string, text: string): HTMLButtonElement {
  const b = ce("button", "fbm-btn " + cls, text);
  b.type = "button";
  return b;
}

export function redactImage(
  file: Blob,
  root: HTMLElement,
): Promise<Blob | null> {
  return new Promise((resolve) => {
    const objectUrl = URL.createObjectURL(file);
    const img = new Image();

    img.onerror = () => {
      URL.revokeObjectURL(objectUrl);
      resolve(null);
    };

    img.onload = () => {
      const rects: Rect[] = [];

      // Reuse the base scrim/modal styling; appended last so DOM order stacks
      // it above the feedback modal (no explicit z-index needed).
      const scrim = ce("div", "fbm-scrim");
      scrim.setAttribute("role", "presentation");
      const panel = ce("div", "fbm-modal fbm-redact-panel");
      panel.setAttribute("role", "dialog");
      panel.setAttribute("aria-modal", "true");
      const titleId = "fbm-rt-" + Math.random().toString(36).slice(2, 8);
      panel.setAttribute("aria-labelledby", titleId);

      const title = ce("h2", "fbm-title", "Redact screenshot");
      title.id = titleId;
      const hint = ce(
        "p",
        "fbm-redact-hint",
        "Drag across the image to black out sensitive areas, then apply.",
      );

      const canvas = ce("canvas", "fbm-redact-canvas");
      canvas.width = img.naturalWidth;
      canvas.height = img.naturalHeight;
      canvas.tabIndex = 0;
      canvas.setAttribute("role", "img");
      canvas.setAttribute(
        "aria-label",
        "Screenshot redaction surface. Drag with a pointer to add black-out boxes.",
      );
      const ctx = canvas.getContext("2d");

      function redraw(preview?: Rect): void {
        if (!ctx) return;
        ctx.clearRect(0, 0, canvas.width, canvas.height);
        ctx.drawImage(img, 0, 0);
        ctx.fillStyle = "#000";
        for (const r of rects) ctx.fillRect(r.x, r.y, r.w, r.h);
        if (preview) ctx.fillRect(preview.x, preview.y, preview.w, preview.h);
      }
      redraw();

      let drawing = false;
      let sx = 0;
      let sy = 0;
      function pt(e: PointerEvent): { x: number; y: number } {
        const r = canvas.getBoundingClientRect();
        return {
          x: ((e.clientX - r.left) * canvas.width) / r.width,
          y: ((e.clientY - r.top) * canvas.height) / r.height,
        };
      }
      function norm(ax: number, ay: number, bx: number, by: number): Rect {
        return {
          x: Math.min(ax, bx),
          y: Math.min(ay, by),
          w: Math.abs(bx - ax),
          h: Math.abs(by - ay),
        };
      }
      canvas.addEventListener("pointerdown", (e) => {
        drawing = true;
        const p = pt(e);
        sx = p.x;
        sy = p.y;
        canvas.setPointerCapture(e.pointerId);
      });
      canvas.addEventListener("pointermove", (e) => {
        if (drawing) {
          const p = pt(e);
          redraw(norm(sx, sy, p.x, p.y));
        }
      });
      function endDraw(e: PointerEvent): void {
        if (!drawing) return;
        drawing = false;
        const p = pt(e);
        const r = norm(sx, sy, p.x, p.y);
        if (r.w > 2 && r.h > 2) rects.push(r);
        redraw();
        undoBtn.disabled = rects.length === 0;
      }
      canvas.addEventListener("pointerup", endDraw);
      canvas.addEventListener("pointercancel", endDraw);

      const actions = ce("div", "fbm-actions fbm-redact-actions");
      const undoBtn = btn("fbm-btn-secondary", "Undo");
      undoBtn.disabled = true;
      undoBtn.addEventListener("click", () => {
        rects.pop();
        redraw();
        undoBtn.disabled = rects.length === 0;
        canvas.focus();
      });
      const cancelBtn = btn("fbm-btn-secondary", "Cancel");
      const applyBtn = btn("fbm-btn-primary", "Apply redaction");
      actions.append(undoBtn, cancelBtn, applyBtn);

      panel.append(title, hint, canvas, actions);
      scrim.appendChild(panel);
      root.appendChild(scrim);

      const previousFocus = document.activeElement as HTMLElement | null;
      const onKey = (e: KeyboardEvent): void => {
        if (e.key === "Escape") {
          e.preventDefault();
          teardown(null);
        } else if (e.key === "Tab") {
          const f = focusables(panel);
          if (!f.length) return;
          const first = f[0];
          const last = f[f.length - 1];
          const a = document.activeElement as HTMLElement | null;
          if (e.shiftKey && (a === first || !panel.contains(a))) {
            e.preventDefault();
            last.focus();
          } else if (!e.shiftKey && a === last) {
            e.preventDefault();
            first.focus();
          }
        }
      };
      function teardown(result: Blob | null): void {
        document.removeEventListener("keydown", onKey, true);
        scrim.remove();
        URL.revokeObjectURL(objectUrl);
        if (previousFocus && document.body.contains(previousFocus)) {
          previousFocus.focus();
        }
        resolve(result);
      }
      cancelBtn.addEventListener("click", () => teardown(null));
      applyBtn.addEventListener("click", () => {
        if (canvas.toBlob) {
          canvas.toBlob((b) => teardown(b), "image/png");
        } else {
          teardown(null);
        }
      });
      document.addEventListener("keydown", onKey, true);
      window.setTimeout(() => canvas.focus(), 0);
    };

    img.src = objectUrl;
  });
}
