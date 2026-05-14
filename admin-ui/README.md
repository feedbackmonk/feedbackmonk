# admin-ui

> Feedbackr tenant-admin web UI — triage, transition, and reply.
> React 18 + Vite 5 + TypeScript 5 + TanStack Query.

## Purpose & Responsibilities

Browser-facing UI that consumes Contracts C7 (transition + reply) and C8 (list + detail) from the `feedbackr-api` Rust backend. Lets tenant admins:

1. Log in (cookie issued by backend; `withCredentials: true`).
2. Browse a status-filtered, paginated feedback list.
3. Open a drawer for one feedback with body + status history + replies.
4. Reply (public or internal).
5. Transition status — only `LEGAL_TRANSITIONS[currentStatus]` are clickable.

FR-FBR-07. P1 Stage 2.

## File Index

| Path | What it is |
|---|---|
| `package.json` | npm package + scripts (`dev`, `build`, `test`, `test:e2e`). |
| `vite.config.ts` | Vite + Vitest config. Port **14204** with `strictPort: true` (Dev Port Registry); `/api` → `http://localhost:14304`. |
| `playwright.config.ts` | Playwright config; spawns the Vite dev server as `webServer`. |
| `tsconfig.json` / `tsconfig.node.json` | Strict TS config. |
| `index.html` | SPA shell. |
| `src/main.tsx` | Entry: QueryClient + Router + ToastProvider + `<App />`. |
| `src/App.tsx` | Route table (login / feedback list / drawer overlay). |
| `src/shared/types.gen.ts` | **Canonical TS mirror** of backend C7/C8 shapes. Source: `docs/planning/handoffs/p1-stage1-to-stage2.md` §TypeScript type mirror. |
| `src/shared/ApiClient.ts` | Axios instance (`withCredentials: true`) + typed wrappers + 401 → `/login` redirect interceptor. |
| `src/shared/router.tsx` | Minimal history-API router + `useSearchParams` hook. |
| `src/shared/format.ts` | Relative/absolute time formatters. |
| `src/pages/Login.tsx` | `POST /api/v1/auth/login` form. |
| `src/pages/FeedbackList.tsx` | Status filter pills (URL state) + paginated table. |
| `src/pages/FeedbackDrawer.tsx` | Body + history + replies + composer + controls. |
| `src/components/StatusBadge.tsx` | Status pill (icon + label — never color-only, WCAG 1.4.1). |
| `src/components/StatusControls.tsx` | State-machine-aware transition buttons. |
| `src/components/ReplyComposer.tsx` | 1..16384-char plain-text reply. |
| `src/components/Toast.tsx` | `aria-live="polite"` toast region + provider. |
| `src/styles/index.css` | Single stylesheet; light + dark color schemes. |
| `src/test/setup.ts` | Vitest setup (`@testing-library/jest-dom`). |
| `src/test/testUtils.tsx` | `renderWithClient` helper. |
| `src/**/*.test.tsx` | Vitest unit tests (StatusControls, ReplyComposer, FeedbackList). |
| `e2e/a11y.spec.ts` | Playwright + `@axe-core/playwright` smoke (login → list → drawer → reply → transition). |

## Public API & Usage

Scripts (run from `admin-ui/`):

```sh
npm install
npm run dev        # Vite dev server on http://localhost:14204
npm run build      # tsc -b && vite build → dist/
npm test           # Vitest unit suite (13 tests)
npm run test:e2e   # Playwright a11y smoke — default fake-API mode
```

Environment toggles:

- `PLAYWRIGHT_FAKE_API=0` — run Playwright against a real backend on `localhost:14304` (requires a seeded admin; current spec marks that path `test.skip`).

## Constraints & Business Rules

1. **Vite must bind 14204 with `strictPort: true`.** Silent fallback was the 2026-04-26 SessionHelm/WinLocksmith incident root cause. Failing loud beats rendering one project's frontend inside another's WebView.
2. **`src/shared/types.gen.ts` is the only place that defines backend response shapes.** Never hand-roll alternative TypeScript types for C7/C8. If CLAUDE-A widens a shape per pre-authorized widenings, mirror here.
3. **No `dangerouslySetInnerHTML`, anywhere.** Submitter-provided bodies render as plain text (`<p>` with `white-space: pre-wrap` in CSS). Stored-XSS defense per handoff doc Contract C8 invariant.
4. **No third-party trackers, ever.** No Segment, Mixpanel, GA, Intercom, Hotjar, Sentry-browser-SDK, PostHog. DEC-FBR-02 brand promise.
5. **No JWT, no `localStorage`/`sessionStorage` for auth.** Auth = the HttpOnly `feedbackr_session` cookie; the browser ships it via `withCredentials: true`. End-user JWTs are a different auth scheme that never appears in the admin UI.
6. **State-machine UI invariant** (`StatusControls.tsx`): `LEGAL_TRANSITIONS[currentStatus]` is the only source of which transition buttons render. Backend 409 fallback is belt-and-braces.
7. **Reply body length 1..16384 chars.** Mirrors backend validation; UI rejects locally before submit.
8. **Color never carries meaning alone.** `StatusBadge` always pairs a glyph + text label with status color (WCAG 1.4.1).
9. **CORS is NOT this app's responsibility.** Vite dev proxy makes `/api` same-origin in dev; prod serves both backend and built `dist/` from `feedbackr-api`. Deferred to P1 Stage 3 e2e integration per the P1 plan.

## Relationships & Dependencies

- **Depends on**: `feedbackr-api` (Contracts C7/C8/C11) running at `http://localhost:14304` for the live dev proxy.
- **Frozen contract source**: `docs/planning/handoffs/p1-stage1-to-stage2.md` (Stage 1 → Stage 2 handoff doc).
- **Spec authority**: `docs/specs/SPECIFICATION.md` FR-FBR-07.
- **No cross-module imports** within the repo; this is a self-contained npm project peer to `crates/`.

## Decision Log

- **Why a hand-rolled router, not react-router-dom**: only two top-level routes (`/login`, `/feedback`) plus a drawer overlay. Adding the dep + its surface area was premature abstraction. Swap in if route count grows past ~5.
- **Why axios over fetch**: typed instance with default `withCredentials`, response interceptors for the 401 → `/login` redirect, and well-shaped error narrowing via `axios.isAxiosError`. Lower-line-count than re-implementing with fetch wrappers.
- **Why plain CSS modules (single file), not Tailwind**: GitCellar's admin-ui used vanilla CSS; matching its idiom keeps the visual pass uniform without dragging in a build dependency. Token variables + light/dark schemes are enough at this scope.
- **Why Vitest, not Jest**: matches Vite's transform pipeline natively; no separate Babel/TS config to drift.
- **Why a "fake API" mode in Playwright**: Stage 3 hasn't seeded an admin in `e2e-p1-curl.sh` yet. Mock-mode unblocks CI now; flipping `PLAYWRIGHT_FAKE_API=0` is a one-flag switch when the real fixture lands.
- **Why `aria-pressed` on status filter pills, not `aria-selected`**: pills are toggle buttons (one active at a time), not list-items in a listbox. axe's role checking accepts this without violation.
