# `admin-ui/src/shared/` — API clients, wire types and small shared helpers

## Summary

Everything the admin console and the public pages share that is not a component: the axios
client and its typed wrappers, the hand-kept TypeScript mirror of the backend's response
shapes, the per-surface API clients, the minimal router, and formatting / highlighting helpers.
Come here when a backend wire shape changes (the mirror and its client live here) or when a
page needs a request it cannot find.

## Notes

- `types.gen.ts` is **hand-rolled** despite its name — it is the canonical TS mirror of the Rust
  shapes, not generator output. Never define a backend response shape anywhere else.
- `api`'s response interceptor redirects to `/login` on 401 — except on a `/public/…` page and
  for a board's `401 {"error":"SignInRequired"}` (Contract C43), which asks the END USER to sign
  in to that board and is rendered by the board pages as a sign-in prompt. The admin-only clients
  (`boardModerationApi.ts`, `hostingApi.ts`, `localeApi.ts`) must still never be called from a
  public page.
- The board session (FR-FBR-44) is an HttpOnly cookie riding on `withCredentials`; no client
  function ever reads or stores it, and the end-user JWT travels only in `exchangeBoardSession`'s
  POST body.

## File Index

| File | What it is |
|---|---|
| `ApiClient.ts` | Shared axios instance (`withCredentials`, 401 → `/login`) + typed wrappers for feedback, search, sentiment trend, transitions, replies, login, projects, roadmap + promote, tier status, clusters, recommendations, work orders, runner and signing keys, and the public board — list, viewer-aware list with kind filter, item detail, votes, not-needed, the whole-list priority read/save and the board session (Contract C43); `extractTierCapExceeded`. |
| `types.gen.ts` | Hand-kept mirror of the backend response shapes (feedback, status workflow + `LEGAL_TRANSITIONS`, sentiment, roadmap, board and its C43 viewer / detail / priority-list / session shapes with `isSignInRequired`, autopilot). |
| `boardModerationApi.ts` | Admin client + types for the moderation queue and `moderate` action (Contract C28) and per-project board settings. |
| `hostingApi.ts` | Admin client + types for tenant subdomain and custom-domain claim/release (Contract C33) and the public-site read. |
| `localeApi.ts` | Admin client + types for the tenant language setting (Contract C38). |
| `router.tsx` | Minimal history-API router: `Router`, `useRouter`, `useSearchParams`, `Link`. |
| `format.ts` | `formatRelative` / `formatAbsolute` date formatters; the locale is a required argument. |
| `highlight.tsx` | `extractSearchTerms` + `highlightMatches`: best-effort `<mark>` highlighting of search terms in result excerpts. |
| `highlight.test.tsx` | Vitest: quoted phrases, exclusions, operators and stopwords in term extraction, and the highlighted render. |
