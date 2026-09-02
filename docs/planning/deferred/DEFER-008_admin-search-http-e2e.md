---
id: DEFER-008
title: "HTTP end-to-end test for GET /admin/feedback/search?status=… (needs an admin-session fixture for feedback routes)"
status: PROPOSED
origin: local
source-session-id: session-20260901-search-ux
created-at: 2026-09-02
autonomy-hint: autopilot
suggested-entry-point: implement
scope-estimate: single-session
---

# DEFER-008: HTTP end-to-end test for the admin search `status` filter

## Idea

Add an integration test in `crates/feedbackmonk-api/tests/` that logs in as an admin, seeds
feedback in more than one status (including `wontfix`), and asserts that
`GET /api/v1/admin/feedback/search?q=…&status=wontfix` returns only the won't-fix hits with the
correct `total`. It is the one leg no existing test covers: the wire spelling of the status
(`wontfix`, serde) must deserialize through `SearchParams::status` and then bind through
`FeedbackStatus::as_db_str` on the way to the query.

## Why it is deferred

- The API test suite has **no admin-session fixture for the feedback routes** today — nothing under
  `crates/feedbackmonk-api/tests/` hits `/api/v1/admin/feedback`. Building one (tenant + verified
  admin login + session cookie plumbing) is a bigger job than the ten-line repository test that
  landed with the feature, and it is reusable well beyond this endpoint, so it deserves its own
  slot rather than being bolted onto the search commit.
- The two halves are each pinned separately already:
  - JSON⇔DB round-trip for all six variants: `crates/feedbackmonk-core/src/status.rs` (the
    2026-09-01 `#[serde(rename = "wontfix")]` fix, commit `d7dca56`).
  - DB-side binding of `WontFix` through the search filter:
    `crates/feedbackmonk-repository/tests/admin_search_status_filter.rs`
    (`status_filter_binds_the_db_spelling_for_wontfix`).
  What is missing is only the seam between them at the HTTP boundary.

## Originating context

The search UX rework (commit `8b3c2a7`, 2026-09-01) made the status pills compose with a live
search for the first time; before it the search endpoint ignored `status` entirely. That made the
"Won't Fix" pill + search reachable from the UI — the exact variant whose wire form had been broken
(`wont-fix` vs `wontfix`) until `d7dca56`. Flagged by the sibling session's review of `8b3c2a7`.

## Acceptance

- New fixture helper (admin sign-up/verify/login → authenticated `reqwest`/tower client) lives in
  a shared `tests/common/` module so later admin-route tests reuse it.
- Test covers: `status` absent (all FTS hits), `status=wontfix`, `status=triaged`, and an unknown
  status value → 400 (serde rejection), each asserting `items` + `total`.
- `bash scripts/ci-local.sh --tests` green; `.sqlx/` re-prepared with `-- --all-targets` if the
  test uses `sqlx::query!`.
