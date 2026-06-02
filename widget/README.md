# widget/ — feedbackmonk embeddable widget

<!-- Agent Context Header (ULADP) -->
**Synopsis (1-5 lines)**:
Greenfield vanilla-TS+CSS embeddable widget that customer sites load via a
`<script type="module" data-project-id="...">` tag. Mounts a launcher button
+ accessible modal that fetches widget-config (Contract C12) once on mount,
then POSTs feedback to the P0 submission endpoint. Hard-capped at 30 KiB
total bundle, zero third-party trackers (both enforced as code-level
invariants by the `widget-bundle-size` Verification Oracle).

## Purpose & Responsibilities

Ship the customer-facing end-user surface of feedbackmonk:

- A bottom-right launcher button rendered on customer pages.
- A keyboard-accessible modal (subject + body + kind + optional email).
- Submission via the existing `POST /api/v1/projects/{project_id}/feedback`
  P0 endpoint.
- Optional attachments: ≤4 screenshots with a canvas redaction tool, plus
  opt-in console/service-log capture, uploaded via the multipart attachments
  endpoint (GitCellar customer-#1 parity, Gap #1 widget half).
- "Powered by feedbackmonk" footer that the backend toggles per-tier
  (free-tier ON, paid-tier OFF) via Contract C12's `brand.footer_text`.

This module is **independent of `crates/`** — it is a separate npm-managed
TypeScript project with its own toolchain. The only cross-module
dependency is the runtime HTTP wire format documented in Contract C12 +
the P0 submission shape.

## File Index

| File | Role |
|---|---|
| `package.json` | Toolchain: vite + terser + TS + Playwright + axe-core. No React, no UI framework. AGPL-3.0-or-later. `build` = `vite build` **then** an explicit `terser` minify pass (see Decision Log — vite's integrated terser was not minifying JS whitespace in this env). |
| `tsconfig.json` | Strict ES2020 + DOM lib; bundler-mode resolution; `noEmit: true` (vite handles emission). |
| `vite.config.ts` | Lib mode → `dist/widget.js` (ESM entry) + `dist/redact.js` (lazy chunk) + `dist/widget.css`. Stable unhashed `entryFileNames`/`chunkFileNames`. CSS minified by vite; JS minified by the post-build terser pass. |
| `src/attachments.ts` | Screenshot attach UI (≤4, ≤5MB, PNG/JPEG/WebP), opt-in console/service log capture, and the multipart upload (`POST …/feedback/:fb/attachments`). Static import; dynamically imports `redact.ts` on first redact. |
| `src/redact.ts` | **Code-split chunk** (`dist/redact.js`): canvas redaction editor — draw opaque rectangles, export flattened PNG. Self-contained (imports nothing from base) so the entry stays a single `widget.js`. Fetched lazily only when the user redacts. |
| `playwright.config.ts` | Vite preview on port `14205` (`strictPort: true`, deconflicted from admin-ui's 14204 and api's 14304). |
| `.gitignore` | Standard, but `dist/` is INTENTIONALLY tracked — see Decision Log below. |
| `src/widget.ts` | Entry: `mountFeedbackMonk(opts?)` + auto-mount-on-script-load. Owns lifecycle, focus-trap installation/teardown. |
| `src/types.ts` | Mirror of Contract C12 + `SubmitFeedbackRequest`/`Response`. Authored by Worker A on both sides. |
| `src/ui.ts` | DOM construction. CSP-safe (no `innerHTML` with user data; single static literal for the launcher SVG). |
| `src/api.ts` | `fetchWidgetConfig` + `submitFeedback`. JWT bearer if supplied; otherwise anonymous (credentials: include for anon cookie). |
| `src/styles.css` | Custom-prop-driven theme (`--fbm-primary`). Inline-style-free; cached separately by embedders. |
| `e2e/fixture.html` | Host page used by the Playwright harness — loads built bundle, has a host-page launcher to verify focus return. |
| `e2e/widget-a11y.spec.ts` | Playwright + axe-core: modal-closed clean, modal-open clean, Tab cycles inside dialog, ESC closes + focus return. |
| `e2e/fixture-capture.html` | Fixture variant with `data-capture-console` — exercises console-log capture + the consent checkbox + the `console_log` multipart part. |
| `dist/widget.js` | Built ES2020 minified entry bundle (vite + terser). Committed for oracle inspection. |
| `dist/redact.js` | Built minified lazy redaction chunk. Committed for oracle inspection (Probe A counts it toward the cap). |
| `dist/widget.css` | Built minified styles. Committed for oracle inspection. |

## Public API & Usage

### Embedding (auto-mount)

```html
<script
  type="module"
  src="https://cdn.feedbackmonk.com/widget.js"
  data-project-id="00000000-0000-0000-0000-000000000001"
  data-jwt="eyJhbGciOiJIUzI1NiIs..."         <!-- optional; auth mode if present -->
  data-api-base="https://api.feedbackmonk.com" <!-- optional -->
></script>
```

The widget auto-mounts on `DOMContentLoaded` (or immediately if already
loaded). Use `data-fbm-no-auto-mount` to disable auto-mount.

### Programmatic mount

```js
import { mountFeedbackMonk } from "@feedbackmonk/widget";
await mountFeedbackMonk({ projectId: "...", jwt: "...", apiBase: "..." });
```

### Optional attachments + logs

```html
<script
  type="module"
  src="https://cdn.feedbackmonk.com/widget.js"
  data-project-id="..."
  data-capture-console      <!-- opt-in: capture console.* into a bounded
                                  ring buffer; default OFF (privacy-by-default,
                                  DEC-FBR-02). User still consents per-submission
                                  via a checkbox before any log is sent. -->
></script>
```

Users can attach up to 4 screenshots (PNG/JPEG/WebP, ≤5 MB each), black out
sensitive regions with a canvas redaction tool (lazy-loaded `redact.js`), and —
when the embedder opts in and the user consents — include captured console /
host-exposed service logs. Logs are sent **raw**; the backend
(`feedbackmonk-tracing` scrubber) removes PII server-side before persist.
Host pages may expose an app log via `window.__feedbackmonkServiceLog`
(a `string` or `() => string`).

### Wire contract

Outbound:
- `GET  /api/v1/projects/{project_id}/widget-config` → Contract C12 JSON.
- `POST /api/v1/projects/{project_id}/feedback` → P0 submission shape
  (`{ kind, subject, body, email? }`).
- `POST /api/v1/projects/{project_id}/feedback/{feedback_id}/attachments`
  → multipart (GUIDE §6 frozen contract): `files[]` (≤4 images) + optional
  `service_log` / `console_log` text parts. Fired only when attachments/logs
  are present, AFTER the feedback row exists. Attachment-upload failure is a
  **soft failure** — the feedback itself is never lost.

That's it. No telemetry. No callbacks to customer auth. No third-party
scripts loaded at runtime.

## Constraints & Business Rules

### Hard invariants (oracle-enforced)

1. **Bundle size ≤ 30720 bytes**. Sum of `dist/*.{js,mjs,css}` byte counts —
   this includes the lazy `redact.js` chunk (Probe A counts *all* dist files,
   so code-splitting saves runtime base-load weight, not oracle headroom).
   `widget-bundle-size` Probe A. FR-FBR-04. Current: ~21.2 KB used / ~9.5 KB
   headroom (widget.js 13.4 KB + redact.js 2.9 KB + widget.css 4.9 KB).
2. **Zero third-party tracker hostnames in built artifacts**.
   `widget-bundle-size` Probe B reads `expected-trackers.txt` and greps
   every `dist/*` file. DEC-FBR-02 brand promise.
3. **No CSP `unsafe-inline` / `unsafe-eval` required**. Vite terser config
   sets `compress.unsafe: false`; mangle `eval: false`; styles emitted to
   a sibling `.css` file (no `<style>` injection); SVG icon is a static
   string literal embedded in the bundle. Customers running strict CSP
   can embed without relaxing their policy.
4. **JWT bearer is the only identity**. DEC-FBR-04. No callbacks to
   customer auth providers; no long-lived bearer storage; the JWT is
   passed via `Authorization: Bearer` only if the embedder supplies one.

### A11y (load-bearing for FR-FBR-04 a11y gate)

- `role="dialog"` + `aria-modal="true"` + `aria-labelledby` + `aria-describedby`.
- Keyboard trap inside modal (Tab cycles; Shift+Tab reverses).
- ESC closes modal AND returns focus to the launcher (or previously-focused
  element).
- Visible focus indicators on all interactive elements.
- `aria-live="polite"` toast for success/failure; `role="alert"` error region.

### V1 defaults (carry-forwards)

- `brand.footer_text` default = `"powered by feedbackmonk"` on every project
  for v1. **P3 wires the tier-flag flip** — paid tiers will receive `null`
  (no footer). This is intentional carry-forward, not technical debt.
- `auth_modes` hardcoded to `["auth", "anonymous"]` for v1.
- `submission_kinds` hardcoded to `["bug", "feature", "question", "other"]`.
- `max_body_chars` hardcoded to `16384` (mirrors P0 schema CHECK constraint).

## Relationships & Dependencies

- **Consumes**: `crates/feedbackmonk-api/src/handlers/widget_config.rs` (C12) +
  the existing P0 submission endpoint (`handlers/feedback.rs`).
- **Repository surface**: `TenantRepo::get_widget_brand(&TenantScope)` —
  added to `crates/feedbackmonk-repository/src/tenants.rs` by Worker A.
- **Verification Oracle**: `.claude/oracles/widget-bundle-size/` — built
  BEFORE any source file in this directory landed (Task Zero discipline).
- **No npm workspace integration**. The widget has its own lockfile so
  admin-ui's React deps cannot accidentally leak into the embedder's
  bundle.

## Decision Log

- **Attachments = file attach, not page capture**: "screenshot" means the
  user attaches their own image files. Programmatic page capture needs
  `getDisplayMedia` (a screen-share prompt) or html2canvas (a third-party lib
  that fails the tracker scan + budget). Users attach images; the canvas tool
  redacts them. Redacted exports are PNG (`canvas.toBlob`, in the MIME
  allowlist) — universally supported, avoids WebP-encoder variance.
- **Redaction is a lazy code-split chunk** (`redact.js`): the canvas editor is
  the heaviest code and most users never open it, so it is dynamically
  imported on first use (same-origin import, resolved relative to `widget.js`
  — CSP-safe under the embedder's existing `script-src` for the CDN origin; no
  policy change). `redact.ts` imports nothing from the base modules so Rollup
  keeps the entry a single `widget.js` instead of emitting a shared-chunk stub.
  Note: the size oracle still counts the chunk — splitting is a *runtime*
  base-load win, not an oracle-budget win.
- **Console capture is embedder opt-in, default OFF** (`data-capture-console`
  / `captureConsole`): privacy-by-default per DEC-FBR-02. When enabled, a
  bounded ring buffer captures `console.*` from mount, and a per-submission
  consent checkbox (default on, user can opt out) gates whether anything is
  sent. Logs go up raw; PII scrubbing is server-side at the single canonical
  `feedbackmonk-tracing` chokepoint — the widget never builds a second scrub
  path. The console patch reads/assigns `console[m]` (never *calls* a console
  method), so terser `drop_console` can't strip the passthrough.
- **Attachment upload is a soft failure**: it fires AFTER the feedback row
  exists (it needs the `feedback_id`), so a failed upload never costs the user
  their feedback — they get a non-blocking "attachments couldn't be uploaded"
  notice. (The server's error body is `{error}`, not `{code,message}`; the
  widget soft-fails generically so the shape difference is immaterial.)
- **Explicit terser minify pass in `build`**: vite's integrated terser was
  **not** minifying JS whitespace in this environment (esbuild only minified
  identifiers; terser left output fully unminified — the previously-"committed"
  dist was never actually minified, and `dist/` was in fact untracked despite
  the `.gitignore` note). The `build` script now runs `vite build` then an
  explicit `terser … --module` pass per JS file (preserves ESM exports + the
  `import("./redact.js")` dynamic import). CSS is minified fine by vite. This
  recovered ~10 KB and is what keeps the feature under the 30 KB cap.
- **Default `--fbm-primary` is blue-600 (#2563eb), not blue-500**: white text
  on blue-500 (#3b82f6) is only 3.67:1 — a real WCAG-AA contrast failure on the
  launcher + primary buttons, surfaced the first time the e2e harness was made
  runnable (it required `vite preview --outDir .` to serve the project root;
  bare `vite preview` only serves `dist/`, so the fixture URL had always
  404'd). Customer brand colors still override at runtime — see the contrast
  caveat below.
- **Contrast caveat (customer colors)**: the widget renders
  `--fbm-on-primary` (white) text on the customer's `primary_color`. Colors
  below ~4.5:1 against white (like the old default) will fail AA on the
  launcher/primary buttons. v1 ships an AA-clean *default*; guaranteeing AA for
  *arbitrary* customer colors (auto-picking black/white on-primary text, or
  documenting a brand-color contrast requirement) is a recommended follow-up.
- **Why `dist/` is committed**: the `widget-bundle-size` Verification
  Oracle reads `dist/*` to verify Probe A (size) and Probe B (tracker
  scan). Without tracked `dist/`, the oracle has nothing to evaluate
  before `npm install && npm run build` runs, which would defeat the
  inner-loop closure the oracle exists to provide. Reviewing a minified
  diff is intentional friction that surfaces unintentional bundle growth.
- **No npm workspace; isolated lockfile**: prevents admin-ui's React
  + Tanstack-Query devDeps from being available to the widget bundler.
  Worker A pinned the same versions of `@playwright/test@1.48.2` +
  `@axe-core/playwright@4.10.0` to keep CI invocation identical.
- **`createElement` over `innerHTML`**: the only `innerHTML` write in
  the entire widget is the launcher SVG, and its content is a static
  string literal. All other DOM is built via `document.createElement` so
  embedder-supplied data is never interpreted as markup. Defense against
  user-controlled `display_name` being treated as HTML.
- **`fetch` with `credentials: "include"` in anonymous mode only**: the
  P0 anon endpoint reads the `X-Feedbackmonk-Anon-Cookie` header. JWT
  mode uses `credentials: "omit"` to keep the auth surface explicit.
- **Single ESM output, no UMD**: customers embed with `type="module"`
  per modern web standards. Dropping legacy UMD saves ~2KB.
- **No `data-jwt` storage**: the widget reads JWT from script-tag
  attribute or `mountFeedbackMonk({ jwt })` only. We never persist it.
- **Auto-mount via `DOMContentLoaded`**: matches the gitcellar
  customer-help widget pattern. Customers who want manual control opt
  out with `data-fbm-no-auto-mount`.
- **Port 14206 for vite preview**: registered with the project's Dev
  Port Registry. Deconflicted from admin-ui dev (14204), admin-ui e2e
  (14205, claimed by CLAUDE-C in this same PODS session at 04:32Z), and
  api (14304); `strictPort: true` prevents silent collision per
  DEC-FBR-IMPL-04.
- **No icon library**: the launcher uses a single inline SVG. Adding
  even a minified icon set would consume ~3-5KB of budget that's better
  spent on a11y wiring.
- **Q24-equivalent invariants surfaced via oracle, not test**: the
  widget has no equivalent to Worker C's byte-for-byte Q24 test because
  the "no third-party trackers" promise is more naturally enforced at
  artifact-scan time (Probe B) than at runtime — runtime tracking could
  be obfuscated, build-time tracking cannot.
- **Plan §Oracle Pre-Build Plan conformance**: this widget directory
  was created AFTER `widget-bundle-size` was LIVE and GREEN. The
  task-zero ordering is documented in `.claude/collaboration/collab-…/workers/CLAUDE-A/work-log.md`.

## Lineage

- **FR-FBR-04** — Embeddable widget (<30KB, a11y-clean, CSP-safe)
- **DEC-FBR-02** — No third-party trackers in the widget, ever (brand promise)
- **DEC-FBR-04** — JWT is the only identity feedbackmonk ever has
- **DEC-FBR-IMPL-04** — Dev Port Registry + `strictPort: true`
- **Contract C12** — `GET /api/v1/projects/{project_id}/widget-config`
- **P2 plan §Worker A** — Task list + exit gate
- **PODS session** — `collab-20260514-035703` (CLAUDE-A worker)
