# widget/ — feedbackmonk embeddable widget

## Synopsis

Greenfield vanilla-TS+CSS embeddable widget that customer sites load via a `<script type="module" data-project-id="...">` tag. Mounts a launcher button + accessible modal that fetches widget-config (Contract C12) once on mount, then POSTs feedback to the submission endpoint. Speaks 31 languages: the English catalog is inlined and every other locale is a lazily fetched per-locale chunk under `dist/locales/` (FR-FBR-35). Hard-capped at 30 KiB for the English page-load set, 4 KiB per locale chunk, and zero third-party trackers — all enforced as code-level invariants by the `widget-bundle-size` Verification Oracle (FR-FBR-04, DEC-FBR-02, C42).

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
- The whole of the above in the **host page's language** (FR-FBR-35): the
  widget follows `data-locale` → the host's `<html lang>` → the browser, sets
  its own `lang`/`dir`, renders its own error copy instead of the server's, and
  sends the resolved locale with the submission (Contract C37).

This module is **independent of `crates/`** — it is a separate npm-managed
TypeScript project with its own toolchain. The only cross-module
dependency is the runtime HTTP wire format documented in Contract C12 +
the P0 submission shape.

## File Index

| File | Role |
|---|---|
| `package.json` | Toolchain: vite + terser + TS + Playwright + axe-core. No React, no UI framework. AGPL-3.0-or-later. `build` = `vite build` **then** an explicit `terser` minify pass (see Decision Log — vite's integrated terser was not minifying JS whitespace in this env). |
| `tsconfig.json` | Strict ES2020 + DOM lib; bundler-mode resolution; `noEmit: true` (vite handles emission). |
| `vite.config.ts` | Lib mode → `dist/widget.js` (ESM entry) + `dist/redact.js` (lazy chunk) + `dist/locales/<code>.js` (31-locale catalog chunks) + `dist/widget.css`. Stable unhashed names; `chunkFileNames` routes anything from `src/locales/` under `dist/locales/`. CSS minified by vite; JS minified by the post-build terser pass. |
| `scripts/slice-locales.mjs` | **Build step** (runs first in `npm run build`). Slices `i18n/locales/<code>/widget.json` into `src/catalog-en.ts` (inlined English), `src/locales/<code>.ts` (one per other locale) and `src/locale-chunks.ts` (the loader map + RTL list + resolver override tables projected from `i18n/locales.json`). All four are generated and gitignored. |
| `src/i18n.ts` | Runtime i18n, no library and no dependency (DEC-FBR-IMPL-30): `t(key, args?)` with `{{name}}` interpolation and CLDR plurals, `resolveLocale()` (Contract C34), `loadLocale()` (the lazy chunk), `dir()`, per-key fallback to English. |
| `src/i18n.test.ts` | Vitest: `t()` semantics, plural selection, per-key fallback — and every case in `i18n/resolution-fixtures.json`, read at run time so the widget and the Rust resolver stay one contract. |
| `src/ui.test.ts` | Vitest: the English-rendering snapshot (the guard that catalog extraction changed no visible text), `lang`/`dir` on the root, and the error-code → copy mapping. |
| `src/plural-partition.test.ts` | Vitest: the DIFFERENTIAL between the platform's CLDR (`Intl.PluralRules`) and the hand-rolled Rust plural table carried in `i18n/plural-fixtures.json`, plus the digest proving this file's transcription of that table is still faithful. |
| `vitest.config.ts` | jsdom environment for the three unit suites; `e2e/` is excluded (Playwright owns it). |
| `src/attachments.ts` | Screenshot attach UI (≤4, ≤5MB, PNG/JPEG/WebP), opt-in console/service log capture, and the multipart upload (`POST …/feedback/:fb/attachments`). Static import; dynamically imports `redact.ts` on first redact. |
| `src/redact.ts` | **Code-split chunk** (`dist/redact.js`): canvas redaction editor — draw opaque rectangles, export flattened PNG. Self-contained (imports nothing from base) so the entry stays a single `widget.js`. Fetched lazily only when the user redacts. |
| `playwright.config.ts` | Vite preview on port `14206` (`strictPort: true`, deconflicted from admin-ui's 14204 dev / 14205 e2e preview and api's 14304). |
| `.gitignore` | Standard, but `dist/` is INTENTIONALLY tracked — see Decision Log below. |
| `src/widget.ts` | Entry: `mountFeedbackMonk(opts?)` + auto-mount-on-script-load. Owns lifecycle, focus-trap install/teardown, theme resolution (`data-theme` → brand default → `auto`), launcher-less mode (`data-fbm-no-auto-mount`), `[data-feedback-open]` delegated-click wiring, and the `window.feedbackmonk.open()` / handle `.open()`/`.destroy()` API (DEC-FBR-IMPL-12/13). |
| `src/types.ts` | Mirror of Contract C12 + `SubmitFeedbackRequest`/`Response`. Adds `WidgetTheme` (`auto`\|`light`\|`dark`), nullable `primary_color`/`footer_url`/`theme` brand fields, and the `theme`/`noLauncher` mount options + handle `.open()`/`.destroy()` (DEC-FBR-IMPL-12/13). Authored by Worker A on both sides. |
| `src/ui.ts` | DOM construction. CSP-safe (no `innerHTML` with user data; single static literal for the launcher SVG). Applies resolved theme + per-tenant accent; supports launcher-less render. |
| `src/api.ts` | `fetchWidgetConfig` + `submitFeedback`. JWT bearer if supplied; otherwise anonymous (credentials: include for anon cookie). |
| `src/styles.css` | Custom-prop-driven theme (`--fbm-primary`). Light/dark/auto theme variables (DEC-FBR-IMPL-12). Inline-style-free; cached separately by embedders. |
| `e2e/fixture.html` | Host page used by the Playwright harness — loads built bundle, has a host-page launcher to verify focus return. |
| `e2e/widget-a11y.spec.ts` | Playwright + axe-core: modal-closed clean, modal-open clean, Tab cycles inside dialog, ESC closes + focus return; plus launcher-less + dark-theme and `window.feedbackmonk.open()`/`destroy()` coverage (DEC-FBR-IMPL-12/13). |
| `e2e/fixture-capture.html` | Fixture variant with `data-capture-console` — exercises console-log capture + the consent checkbox + the `console_log` multipart part. |
| `e2e/widget-locale.spec.ts` | Playwright: the en/de/fa locale matrix (`lang`/`dir`, the chunk request, axe clean), the C36 precedence cases, the CSP fixture, and the RTL grep guard over `styles.css`. |
| `e2e/fixture-locale.html` | No `lang`, no `data-locale` — puts `navigator.languages` in charge so the matrix can drive it with Playwright's `locale`. |
| `e2e/fixture-locale-attr.html` | `data-locale="fa"` against an English host page and a German browser — proves embed-wins precedence and is the RTL case. |
| `e2e/fixture-csp.html` | `script-src 'self'` fixture (TGF-…-01): proves the lazy locale chunk loads under a real embedder's policy with no CSP violation logged. |
| `e2e/fixture-no-launcher.html` | Fixture variant with `data-fbm-no-auto-mount` + a `[data-feedback-open]` host trigger — exercises launcher-less / embedder-trigger mode (DEC-FBR-IMPL-13). |
| `dist/widget.js` | Built ES2020 minified entry bundle (vite + terser). Committed for oracle inspection. |
| `dist/redact.js` | Built minified lazy redaction chunk. Committed for oracle inspection (Probe A counts it toward the cap). |
| `dist/widget.css` | Built minified styles. Committed for oracle inspection. |
| `dist/locales/<code>.js` | Built per-locale catalog chunks, one per non-English locale (Contract C42). Committed like the rest of `dist/`; each is capped at 4,096 B by Probe C. |

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
loaded).

### Theme (DEC-FBR-IMPL-12)

Add `data-theme="auto" | "light" | "dark"` to the script tag to force a theme.
Precedence: `data-theme` (or the `theme` mount option) → the per-tenant
`brand.theme` default from widget-config → `"auto"` (follows
`prefers-color-scheme`). The accent is driven by the per-tenant
`primary_color`; when the tenant sets none, the widget's WCAG-AA-safe
`#2563eb` CSS default wins (`primary_color` is nullable end-to-end).

### Language (FR-FBR-35 / Contract C36)

Add `data-locale="<code>"` to the script tag to tell the widget what language
the surrounding page is in:

```html
<script
  type="module"
  src="https://cdn.feedbackmonk.com/widget.js"
  data-project-id="..."
  data-locale="de"          <!-- optional; any of the 31 codes in i18n/locales.json -->
></script>
```

Precedence: `data-locale` (or the `locale` mount option) → the host page's
`<html lang>` → `navigator.languages` → `en`. Tags are resolved through the
C34 table, so `de-AT` → `de`, `pt` → `pt-BR`, `zh-Hant-CN` → `zh-TW`, and
anything unshipped falls through to the next candidate and finally to English —
an unknown value degrades, it never errors.

**The host owns the decision.** The embedder already knows its user's language
(a cookie, an account setting, its own routing); the widget only follows. That
is also why an unshipped `data-locale` is not an error: shipping a page in a
language feedbackmonk has not translated yet must never break the feedback
button.

The widget sets `lang` and `dir` on its own root (`.fbm-root`), so a German
widget on an English page is announced correctly by a screen reader and a
Persian one lays out right-to-left without the host doing anything.

**Attribute reference**

| Attribute | Meaning |
|---|---|
| `data-project-id` | **Required.** The project the feedback belongs to. |
| `data-jwt` | Optional. End-user identity (auth mode); anonymous without it. |
| `data-api-base` | Optional. Backend origin; defaults to `https://api.feedbackmonk.com`. |
| `data-locale` | Optional. UI language (this section). |
| `data-theme` | Optional. `auto` \| `light` \| `dark`. |
| `data-capture-console` | Optional, opt-in. Capture `console.*` for diagnostics. |
| `data-fbm-no-auto-mount` | Optional. Launcher-less mode. |
| `data-feedback-open` | On any element on the page: clicking it opens the modal. |

### Vendoring `dist/` (read this before copying the widget into another repo)

`dist/` is no longer three files. It is:

```
dist/
  widget.js         entry
  widget.css        styles
  redact.js         lazy chunk (screenshot redaction)
  locales/          31 lazy chunks, one per non-English locale  <-- NEW
    de.js  fr.js  fa.js  …
```

**Copy the whole `dist/` tree, directory structure included.** A vendored copy
that takes only the top-level files still works — every locale silently falls
back to English, because a missing chunk is caught and ignored by design — which
is precisely why it needs saying here: the failure is invisible. GitCellar
vendors this widget by copy in two places, and both need the `locales/`
directory alongside `widget.js`.

Serving requirement: the chunks are fetched with a same-origin dynamic
`import()` relative to `widget.js`, so they must be served from the same origin
under `locales/`, with a JavaScript MIME type. No CSP change is needed —
`script-src 'self'` is sufficient, and `e2e/fixture-csp.html` proves it on every
commit.

### Launcher-less / embedder-trigger mode (DEC-FBR-IMPL-13)

Add `data-fbm-no-auto-mount` to the script tag to initialize **launcher-less**:
the widget mounts but renders no floating launcher. The embedder opens the
modal with either an `[data-feedback-open]` element (auto-wired via a single
document-level delegated click listener) or `window.feedbackmonk.open()`. The
mount handle also exposes `.open()` and `.destroy()` (removes modal + launcher
+ listeners + root). Note: `data-fbm-no-auto-mount` no longer means "do not
mount" — the old meaning produced a dead, un-openable widget, so this redefinition
is safe.

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
  (`{ kind, subject, body, email?, locale }`). `locale` (Contract C37) is the
  resolved C34 code — always one of the 31, never the raw attribute value. It
  records how to write BACK to this person; it says nothing about what language
  the body is in. The server stores an unknown value as NULL and never rejects
  the submission for it.
- `GET <widget.js origin>/locales/<code>.js` → the catalog chunk for a
  non-English locale, fetched once at mount. English is inlined and fetches
  nothing (Contract C42).
- `POST /api/v1/projects/{project_id}/feedback/{feedback_id}/attachments`
  → multipart (GUIDE §6 frozen contract): `files[]` (≤4 images) + optional
  `service_log` / `console_log` text parts. Fired only when attachments/logs
  are present, AFTER the feedback row exists. Attachment-upload failure is a
  **soft failure** — the feedback itself is never lost.

That's it. No telemetry. No callbacks to customer auth. No third-party
scripts loaded at runtime.

## Constraints & Business Rules

### Hard invariants (oracle-enforced)

1. **English page-load set ≤ 30720 bytes**. Sum of the **top-level**
   `dist/*.{js,mjs,css}` byte counts — `widget.js` + `widget.css` +
   `redact.js`. `widget-bundle-size` Probe A. FR-FBR-04. Current: **29,836 B
   used / 884 B headroom** (widget.js 21,601 + widget.css 5,390 + redact.js
   2,845).

   Localization redefined the measured SET, never the cap: before FR-FBR-35,
   "everything under `dist/`" and "what a page load fetches" were the same
   number; with 30 locale chunks they are not, and summing all 31 would measure
   a page load nobody performs while letting one oversized chunk hide inside
   the total. `SIZE_CAP_BYTES` is the same 30720 it has always been, and
   Probe C (below) is a new, additional ceiling.

   **If a future string does not fit**: the next lever is dropping the
   redundant `widget.` prefix from the inlined catalog and the `t()` call sites
   (the widget only ever reads its own namespace) — a mechanical **~735 B**,
   deliberately left unspent so the code and the catalog files share one key
   vocabulary. Raising the cap is not on the list.

1b. **Each locale chunk ≤ 4096 bytes**, `dist/locales/*.js`.
   `widget-bundle-size` Probe C, Contract C42. A chunk is a flat map of ~59
   short strings; one that trips this is carrying something that is not
   strings. Current largest: 53 B (the catalogs are untranslated skeletons
   until the owner runs `/1-translate`).
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
- **Verification Oracle**: `.claude/project-oracles/widget-bundle-size/` — built
  BEFORE any source file in this directory landed (Task Zero discipline).
- **No npm workspace integration**. The widget has its own lockfile so
  admin-ui's React deps cannot accidentally leak into the embedder's
  bundle.
- **Consumes the catalog tree**: `i18n/locales/<code>/widget.json` and
  `i18n/locales.json` (Contracts C34/C35), read at BUILD time by
  `scripts/slice-locales.mjs`. The widget's runtime never reads them directly,
  and this module writes only `i18n/locales/en/widget.json` — the other 30 are
  written by `scripts/i18n/translate.py` and nobody else (DEC-FBR-17).
- **Build**: `npm run build` = `node scripts/slice-locales.mjs` (catalog slices
  + loader map) then `vite build` (rollup under the hood) + terser minify →
  `dist/`. The slice step runs FIRST because `src/catalog-en.ts`,
  `src/locale-chunks.ts` and `src/locales/*.ts` are generated and gitignored —
  a fresh clone has none of them, so `tsc -b` and `vitest` also run it first
  (`npm run build:check`, `npm test`). **Windows gotcha**: if the build dies with *"Cannot find
  module @rollup/rollup-win32-x64-msvc … npm has a bug related to optional
  dependencies"*, it is **not** npm bug 4828 — the culprit is a machine-global
  `~/.npmrc` `os=linux`, which makes npm skip the Windows-native rollup binary
  on every install (including a clean `npm ci`). Fix on this machine:
  `npm install --os=win32` (CLI overrides `~/.npmrc`), then `npm run build`.
  Linux CI is unaffected. `dist/` is a **vendored artifact** — rebuild it in
  the same commit as any `src/` change so `widget-bundle-size` doesn't pass on
  a stale bundle.

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
  customer-help widget pattern. Customers who want a launcher-less,
  embedder-driven trigger use `data-fbm-no-auto-mount` (mounts without a
  floating launcher; open via `[data-feedback-open]` or
  `window.feedbackmonk.open()` — DEC-FBR-IMPL-13).
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

- **The widget follows the host page's language; it never guesses on its own
  behalf** (FR-FBR-35 / C36). `data-locale` → `<html lang>` →
  `navigator.languages` → `en`. The embedder is the only party that knows what
  language it is rendering, and a GitCellar `/de/pricing` page whose shell is
  still `lang="en"` is exactly the case `data-locale` exists for. Rejected:
  reading the language from the JWT (couples the UI language to identity, and
  anonymous submitters have no JWT) and asking the backend (a round trip before
  the first paint).
- **An unshipped locale is never an error.** An unknown `data-locale` falls
  through to the next candidate and finally to English; a chunk that 404s or is
  blocked leaves the widget in English; a key missing from a translated catalog
  renders its English source. Every degradation path lands on a working widget,
  because the alternative — a feedback button that breaks when a page ships in
  a language we have not translated yet — is worse than an English dialog.
- **English is inlined, every other locale is a lazy chunk** (C42). An English
  visitor fetches exactly what they fetched before this feature existed, and
  the inlined map doubles as the per-key fallback, so the fallback path costs
  no extra network. 31 inlined catalogs would be ~60 KB, i.e. twice the cap.
- **The server's error message is never rendered.** `showError` takes a CODE,
  not an `ApiError`, so the rule is enforced by the signature rather than by
  everyone remembering it. Two reasons: server text is English (a localized
  widget would speak two languages in one dialog), and it is server-authored
  text landing in the user's face. Note the API currently emits
  `{"error": "..."}` while `readError()` expects `{code, message}`, so every
  API error already arrives as `http_<status>` — which is why the mapping is
  keyed on status classes.
- **`redactImage` takes `t` as a parameter instead of importing it.** An
  `import` of `./i18n.js` from `redact.ts` would make Rollup hoist the i18n
  module into a chunk shared with the entry, adding a network round trip to
  every page load to save nothing. Same reasoning that keeps `redact.ts`
  self-contained in the first place; the `import type` for the signature is
  erased at compile time and creates no such edge.
- **`src/locale-chunks.ts` is generated instead of importing
  `src/locales.gen.ts`.** That generated table carries every locale's endonym
  and DeepL target — ~1.9 KB the widget cannot use (it renders no language
  switcher) and cannot tree-shake away, because its `LOCALE_CODES` / `BY_CODE`
  initializers call `.map`, which Rollup must assume is impure. Importing any
  symbol from it drags the whole table in and puts the bundle 815 B OVER the
  cap. The projection is generated from the same `i18n/locales.json` on every
  build, so the two cannot drift.
- **The lazy chunks are loaded through a generated map of literal
  `import()` calls**, not `import(`./locales/${code}.js`)`. Vite's lib mode
  does not chunk a template-literal import at all — it compiles to a runtime
  path-guessing helper and emits nothing under `dist/locales/` (measured
  2026-09-06). The literal map also means no computed specifier ever reaches
  `import()`: the set of loadable chunks is a whitelist by construction.

## Lineage

- **FR-FBR-04** — Embeddable widget (<30KB, a11y-clean, CSP-safe)
- **DEC-FBR-02** — No third-party trackers in the widget, ever (brand promise)
- **DEC-FBR-04** — JWT is the only identity feedbackmonk ever has
- **DEC-FBR-IMPL-04** — Dev Port Registry + `strictPort: true`
- **Contract C12** — `GET /api/v1/projects/{project_id}/widget-config`
- **P2 plan §Worker A** — Task list + exit gate
- **PODS session** — `collab-20260514-035703` (CLAUDE-A worker)
