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
- `api`'s response interceptor redirects to `/login` on 401, so the admin-only clients
  (`boardModerationApi.ts`, `hostingApi.ts`, `localeApi.ts`) must never be called from a public
  page.

## File Index

| File | What it is |
|---|---|
| `ApiClient.ts` | Shared axios instance (`withCredentials`, 401 → `/login`) + typed wrappers for feedback, search, sentiment trend, transitions, replies, login, projects, roadmap + promote, tier status, clusters, recommendations, work orders, runner and signing keys, and the public board; `extractTierCapExceeded`. |
| `types.gen.ts` | Hand-kept mirror of the backend response shapes (feedback, status workflow + `LEGAL_TRANSITIONS`, sentiment, roadmap, board, autopilot). |
| `boardModerationApi.ts` | Admin client + types for the moderation queue and `moderate` action (Contract C28) and per-project board settings. |
| `hostingApi.ts` | Admin client + types for tenant subdomain and custom-domain claim/release (Contract C33) and the public-site read. |
| `localeApi.ts` | Admin client + types for the tenant language setting (Contract C38). |
| `router.tsx` | Minimal history-API router: `Router`, `useRouter`, `useSearchParams`, `Link`. |
| `format.ts` | `formatRelative` / `formatAbsolute` date formatters; the locale is a required argument. |
| `highlight.tsx` | `extractSearchTerms` + `highlightMatches`: best-effort `<mark>` highlighting of search terms in result excerpts. |
| `highlight.test.tsx` | Vitest: quoted phrases, exclusions, operators and stopwords in term extraction, and the highlighted render. |
