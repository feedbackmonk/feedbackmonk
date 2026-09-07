<!--
Agent Context Header (ULADP):
- Purpose: Reusable presentational + interaction components for the feedbackmonk
  admin UI. Pages (admin-ui/src/pages/) compose these; components hold no routing
  or data-fetching responsibility beyond the props/callbacks they declare.
- Owner module: admin-ui/src/components/
- Read first: this README + admin-ui/README.md
-->

# components/ — admin UI reusable components

## Synopsis

Reusable presentational + small-interaction React components shared across admin pages (`ReplyComposer`, `StatusBadge`, `StatusControls`, `Toast`, `SearchBox`). Each is self-contained — receives data/callbacks via props, emits intent upward, and owns no routing or data-fetching. WCAG affordances are encoded directly (color never carries meaning alone). Open it to find a building block before composing a page.

## 1. Purpose & Responsibilities

Presentational and small-interaction React components shared across admin
pages. Each component is self-contained: it receives data and callbacks via
props and emits intent upward — it does not fetch, route, or own server
state. Accessibility is load-bearing here (the admin UI is the operator
surface); components encode WCAG affordances directly rather than relying on
page-level wrappers.

## 2. File Index

| File | Purpose |
|---|---|
| `ReplyComposer.tsx` | Plain-text reply editor for the feedback detail view. Exports `REPLY_MIN`/`REPLY_MAX` (1..16384, mirrors Contract C7's validator so the UI rejects locally first). Plain-text-only by design (no rich-text toolbar — P1 deferred decision). |
| `StatusBadge.tsx` | Renders a `FeedbackStatus` as an icon + label pair, label from `useLabels().status()` (Stage 2 / W-D). Color never carries meaning alone (WCAG 1.4.1) — the icon/label pair is the accessible signal. |
| `StatusControls.tsx` | Status-transition control. `LEGAL_TRANSITIONS[currentStatus]` is the sole source of which transitions the UI offers; backend 409 (Contract C7 `TransitionError`) is the fallback guard. Every string, including the transition labels, is localized (`useTranslation("admin")` + `useLabels()`). |
| `Toast.tsx` | `ToastProvider` context + toast queue for transient success/error notifications. Renders whatever localized message the caller passes; carries no strings of its own. |
| `SearchBox.tsx` | **GitCellar parity gap #3.** Debounced full-text search box for the admin feedback list. Exports `SEARCH_DEBOUNCE_MS` (250ms) and `SEARCH_FOCUS_KEY` (`/`). Commits the trimmed query after the debounce settles; the page mirrors it to the URL `q` param and calls `GET /api/v1/admin/feedback/search`. Layout is reflow-free: label + inline "Clear search" link share one fixed-height row, an always-rendered syntax hint (`aria-describedby`) sits under the field. `Esc` clears; `/` anywhere on the page (outside another editable control) focuses the field (`focusKey` prop; `null` disables). Match highlighting for the result excerpts lives in `shared/highlight.tsx`. `label`/`placeholder` default to the localized catalog value, resolved inside the component body (not a JS default-param literal) once no caller overrides them. |
| `SentimentBadge.tsx` | **P5a**: Renders a `Sentiment` value (`Positive | Neutral | Negative`) as an icon + label + accessible color cue, label from `useLabels().sentiment()`. Mirrors `StatusBadge` pattern — color + icon + label, never color alone (WCAG 1.4.1). |
| `SentimentTrendChart.tsx` | **P5a**: Line chart visualizing sentiment distribution over a rolling time window (7/30/90 days). Consumes `GET /api/v1/admin/feedback/sentiment-trend` timeseries data. WCAG-compliant chart (data table alternative available, color-blind palette). |
| `*.test.tsx` | Vitest unit tests colocated per component. |
| `README.md` | This file. |

## 3. Public API & Usage

Components are imported directly by pages:

```tsx
import { SearchBox, SEARCH_DEBOUNCE_MS } from "../components/SearchBox";

<SearchBox value={q} onCommit={setQuery} />
```

Each component's props interface is declared inline in its `.tsx`; consult the
file for the authoritative shape.

## 4. Constraints & Business Rules

1. **Accessibility is not optional.** `StatusBadge` never signals via color
   alone; interactive components are keyboard-operable and labelled. The widget
   surface has an axe-core gate; the admin UI relies on these component-level
   invariants plus component tests.
2. **No data fetching in components.** Components receive data + callbacks via
   props. Server interaction (search requests, transition PATCHes) is owned by
   the consuming page. This keeps components testable without a network mock.
3. **Local validation mirrors backend contracts.** Where a component gates
   input (`ReplyComposer` length, `StatusControls` legal transitions), the
   bounds mirror the backend contract so the UI rejects before the request —
   but the backend remains the authoritative validator.

## 5. Relationships & Dependencies

- **Consumed by**: `admin-ui/src/pages/` (FeedbackList, feedback detail,
  roadmap, settings).
- **Depends on**: `admin-ui/src/shared/` (ApiClient, types), React.

## 6. Decision Log

- **Stage 2 (W-D) localization.** Every literal string in this directory now
  comes from `i18n/locales/en/admin.json` via `useTranslation("admin")`, and
  every wire-enum label via `i18n/useLabels.ts` / `i18n/useAdminLabels.ts`.
  `SearchBox`'s `label`/`placeholder` props still default sensibly with no
  caller override — the default is now resolved with `t()` inside the
  component body rather than as a JS default-parameter string literal, since a
  hook cannot run in a parameter-default expression cleanly.
- **Debounce over throttle for `SearchBox` (250ms).** Full-text search fires on
  the trailing edge after typing settles, not at a fixed rate. Throttling would
  issue requests mid-keystroke, causing result flicker and wasted backend FTS
  queries; debounce issues exactly one query per typing pause. 250ms is below
  the perceptible-lag threshold while comfortably coalescing fast typing.
- **Components are presentation-only; pages own data.** Keeps the component
  layer trivially unit-testable (no network/router mocks) and lets pages remain
  the single place where server state and URL state reconcile.
