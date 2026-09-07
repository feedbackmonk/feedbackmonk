<!--
Agent Context Header (ULADP):
- Purpose: Route-level page components for the feedbackmonk admin UI —
  the top-level feedback triage surface (list/drawer/reply/transition) and
  the operator login. Feature-scoped pages live in their own subdirectories
  (autopilot/, board/, moderation/, public/, roadmap/, settings/), each with
  its own README; this file covers the module root plus those subdirectories
  as one index, since they compose one routed surface under `App.tsx`.
- Owner module: admin-ui/src/pages/
- Read first: this README, then the subdirectory README for the feature
  you're touching (autopilot/README.md, moderation/README.md,
  settings/README.md, board/README.md).
-->

# `pages/` — admin UI routed pages

## Synopsis

Every route `App.tsx` mounts lives under here: the top-level feedback triage flow (`FeedbackList` → `FeedbackDrawer` → reply/transition), `Login`, and six feature subdirectories (`autopilot/`, `board/`, `moderation/`, `public/`, `roadmap/`, `settings/`) each with their own README. This directory had no root README before Stage 2 (W-D) — it is authored now because that stage rewrites nearly every file here (FR-FBR-38 localization) and an agent orienting from `pages/` had nowhere to start.

## Purpose & Responsibilities

- **`FeedbackList.tsx`** — the admin feedback table: status-filtered, full-text-searchable (`components/SearchBox`), paginated, with an expandable satisfaction-trend panel (`components/SentimentTrendChart`).
- **`FeedbackDrawer.tsx`** — the per-feedback overlay: metadata, the FR-FBR-30 machine-translation toggle, status history, replies (public/internal tabs), and the reply/transition/promote controls (`components/ReplyComposer`, `components/StatusControls`, `roadmap/PromoteButton`).
- **`Login.tsx`** — the operator sign-in form. The only unauthenticated admin route besides the public surfaces.
- **Subdirectories** — each is a self-contained feature slice with its own README: `autopilot/` (P5a review & approval), `board/` + `roadmap/` (public voting surfaces, mostly out of Stage 2's scope — see their own READMEs), `moderation/` (Public Feedback Board moderation queue), `public/` (host-rooted tenant landing), `settings/` (the five `/admin/settings/*` pages).

## File Index

| File | Role |
|---|---|
| `FeedbackList.tsx` | `/feedback` — the admin feedback table |
| `FeedbackList.test.tsx` | Vitest suite — search+filter composition, pagination |
| `FeedbackDrawer.tsx` | `/feedback/:id` overlay — detail, history, replies, actions |
| `FeedbackDrawer.test.tsx` | Vitest suite — the FR-FBR-30 translation toggle |
| `Login.tsx` | `/login` — operator sign-in |
| `autopilot/` | P5a review & approval surface — see `autopilot/README.md` |
| `board/` | Public feedback board (end-user, no admin chrome) — see `board/README.md` |
| `moderation/` | Owner moderation queue for the public board — see `moderation/README.md` |
| `public/` | Host-rooted tenant landing (FR-FBR-32) |
| `roadmap/` | Admin + public roadmap pages, and the feedback→roadmap promote flow |
| `settings/` | The five `/admin/settings/*` pages — see `settings/README.md` |

## Public API & Usage

Every page here is mounted by hand-rolled route matching in `admin-ui/src/App.tsx` (no router library — see that file's own routing comment block for the full path table). Pages are plain default-export-free named exports, imported directly:

```tsx
import { FeedbackList } from "./pages/FeedbackList";
import { FeedbackDrawer } from "./pages/FeedbackDrawer";
```

## Constraints & Business Rules

- **Submitter-provided content is always untrusted.** `body`, `body_excerpt`, reply bodies, cluster/recommendation text — rendered as escaped React text nodes only, never `dangerouslySetInnerHTML` (Contract C8 invariant; C24 corpus for the autopilot surface).
- **No content translation on any surface here.** FR-FBR-38 localizes CHROME (labels, buttons, headings) via `useTranslation("admin")`; feedback bodies and the FR-FBR-30 translation toggle are a *different* axis entirely (Q24 / DEC-FBR-15) — never conflate the two.
- **Every wire enum renders through a labels hook**, never a hardcoded string map: `useLabels()` (the four SPA+Rust shared families) or `useAdminLabels()` (admin-only families, `i18n/useAdminLabels.ts`).
- **Every date/number is locale-aware.** `shared/format.ts`'s `formatRelative`/`formatAbsolute` take a required `locale` argument (Stage 2 / W-D, D-FBR-31) — always thread `useLocale().locale` through, never call bare.

## Relationships & Dependencies

- **Consumes**: `shared/ApiClient.ts` (feedback CRUD), `components/*` (badges, controls, search, toasts), `i18n/*` (`useTranslation`, `useLabels`, `useAdminLabels`, `useLocale`).
- **Consumed by**: `App.tsx`'s route matcher.
- **Sibling module**: `components/README.md` documents the presentational pieces these pages compose; `i18n/README.md` documents the localization runtime every page here depends on.

## Decision Log

- **Root README authored at Stage 2 (W-D), not earlier.** The directory's four subdirectories each already had one; the root did not, and Stage 2 rewrites nearly every file directly under it (FR-FBR-38 extraction) — exactly the moment a missing README costs the most. ULADP requires the module README exist in the same commit as the change that would otherwise leave it silently stale.
- **One README for the module root, feature subdirectories keep their own.** `autopilot/`, `moderation/`, `settings/`, `board/` are each a large enough feature slice to warrant independent documentation; folding them into one file here would just relocate the staleness risk rather than remove it.
