---
schema: judge/1
verdict: VOUCH
vouched_by: judge
judge_model: claude-fable-5-1
judge_seat: distinct
ruled_at: 2026-10-13T17:12:00Z
working_session: 8c0335d2-b0cf-486b-9f49-21b7e0f44fc0
diff_form: a3224c23e893dc3fd3a8c19db448e4ca5f0361fb..20ade55e727590ee0cff96b1f6ca13403d12d541
test_diff_sha256: f3d812eee645e341379a255ed01d3d4c597549f3123a9715f92ef18c5fb64886
code_diff_sha256: 8974f18d8bf6d382071423f77b157f66e7b3b65400246cc1c69184481dcf8913
tests: ["crates/feedbackmonk-repository/src/board_sessions.rs", "crates/feedbackmonk-repository/src/board_signals.rs", "crates/feedbackmonk-repository/src/lib.rs", "crates/feedbackmonk-api/src/handlers/board_session.rs", "crates/feedbackmonk-core/src/board_signals.rs", "crates/feedbackmonk-repository/src/board_notes.rs"]
---

## Reason

No existing assertion changed: five of the six test paths are new files, and the sixth, `crates/feedbackmonk-repository/src/lib.rs`, has 14 added lines (three `pub mod` registrations, a `#[cfg(test)] mod test_support`, and re-exports) and no deletions. The pre-existing files the code diff touches (`board_votes.rs`, `feedback.rs`, `projects.rs`) have no hunk inside a `#[cfg(test)]` region, so no prior test was loosened, skipped or deleted. The 19 new tests assert the new contracts with real negative cases, each backed by code the same range adds: `NotFound` on pending/rejected/bug/shipped/foreign items before any not-needed or priority-list write (the resolve-before-write gate in `board_signals.rs`), `upvote_cleared` plus a zero vote count and an empty `feedback_not_needed` table (the transactional DELETE added to `BoardVoteRepo::cast` and `set_not_needed`), the 8-hour clamp against `BOARD_SESSION_MAX_AGE_SECS`, and per-table row counts after `erase_all_for_end_user` (the three DELETEs added to `feedback.rs`). Between the branch's two commits the only edit to a test path was a code hunk (the hex-encoder swap in `handlers/board_session.rs`); its test's expected value is the standard SHA-256 of "abc", not a value read off the implementation.

## Rationale submitted

this wave (the "contracts" piece of docs/planning/plans/20261013T145126-end-user-prioritization.md, implementing the data layer for docs/specs/end-user-prioritization.md) ADDED new tests for new tables and repository methods. It covers:
- the board kind filter;
- the approved-only plus ranking-pool gate before not-needed and priority-list writes;
- upvote and not-needed exclusivity in one transaction;
- the priority list saved whole;
- pool drop-out;
- board-session creation, expiry and the 8-hour cap;
- board notes, gated to approved items;
- erasure of the new user-keyed tables from `erase_all_for_end_user`.

The intent is that no existing test was removed or weakened. The lib.rs change is the shared erasure test or module registration. Verify that claim: an existing assertion loosened, deleted or skipped would be a weakening. An independent critic separately ran these tests on the merged tree: 21 passed, 0 failed.
