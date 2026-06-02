<!--
Agent Context Header (ULADP):
- Purpose: Reusable presentational + interaction components for the feedbackmonk
  admin UI. Pages (admin-ui/src/pages/) compose these; components hold no routing
  or data-fetching responsibility beyond the props/callbacks they declare.
- Owner module: admin-ui/src/components/
- Read first: this README + admin-ui/README.md
-->

# components/ — admin UI reusable components

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
| `StatusBadge.tsx` | Renders a `FeedbackStatus` as an icon + label pair. Color never carries meaning alone (WCAG 1.4.1) — the icon/label pair is the accessible signal. |
| `StatusControls.tsx` | Status-transition control. `LEGAL_TRANSITIONS[currentStatus]` is the sole source of which transitions the UI offers; backend 409 (Contract C7 `TransitionError`) is the fallback guard. |
| `Toast.tsx` | `ToastProvider` context + toast queue for transient success/error notifications. |
| `SearchBox.tsx` | **GitCellar parity gap #3.** Debounced full-text search box for the admin feedback list. Exports `SEARCH_DEBOUNCE_MS` (250ms). Commits the trimmed query after the debounce settles; the page mirrors it to the URL `q` param and calls `GET /api/v1/admin/feedback/search`. |
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

- **Debounce over throttle for `SearchBox` (250ms).** Full-text search fires on
  the trailing edge after typing settles, not at a fixed rate. Throttling would
  issue requests mid-keystroke, causing result flicker and wasted backend FTS
  queries; debounce issues exactly one query per typing pause. 250ms is below
  the perceptible-lag threshold while comfortably coalescing fast typing.
- **Components are presentation-only; pages own data.** Keeps the component
  layer trivially unit-testable (no network/router mocks) and lets pages remain
  the single place where server state and URL state reconcile.
