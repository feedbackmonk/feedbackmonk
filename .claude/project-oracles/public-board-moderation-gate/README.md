# public-board-moderation-gate

## Summary

Proves from code that no public-board endpoint (read or vote) can return or act on a
feedback row whose `moderation_status` is not `approved`, and that the board wire shape
carries no submitter PII (C29 inv. 1 and 3, plan D2). The moderation gate is the trust
boundary between public submission and public exposure. Come here when you touch
`handlers/board.rs`, a `*board*` repository read, or the board wire shape.

## Probes

Run: `python .claude/project-oracles/public-board-moderation-gate/oracle.py [--full] [--root <repo>]`.
Exit 0 PASS, 1 FAIL, 2 environment error. `--root` points the oracle at another tree
(default: this repository) and exists for the self-test below.

**Probe B (static, default run, ~0.3 s).** Scope: `crates/feedbackmonk-api/src/handlers/board.rs`
plus every repository fn named `*board*` whose body queries `FROM feedback`.

- **(0)** board.rs and at least one board read fn must exist. A missing file is a FAIL; there
  is no PENDING state.
- **(0b)** board.rs may reach the feedback repository only through `BOARD_SAFE_READS` in
  `oracle.py` (`list_public_board`, `get_public_board_item`, `resolve_approved_board_feedback_id`).
  Any other `.feedback.<method>(` call (whitespace/multi-line tolerant), an aliased
  `.feedback` handle, or raw `sqlx` / `.pool` / `PgPool` / `query*(` in board.rs fails. Each
  allowlisted method must itself be a discovered board read fn, so check (1) pins its SQL.
- **(1)** Every SQL string literal in each board read fn must carry the literal
  `moderation_status = 'approved'` at least once per `FROM/JOIN feedback` it contains.
  Rust comments and SQL `--` / `/* */` comments are stripped first, so a filter present only in
  a comment does not count. A bound parameter does not count.
- **(1b)** no `'pending'` / `'rejected'` literal in scope. **(2)** no submitter-PII column in
  scope. **(3)** no `feedback_replies` in scope.
- **(4)** every board.rs handler writing through `.board_votes` calls `ensure_board_enabled`
  and `resolve_approved_board_*` before the write.

**Probe C (`--full`).** `cargo test -p feedbackmonk-api --test board_moderation_gate --test
board_privacy_isolation --test board_vote_moderation_gate` against a real database. A missing
test file is a FAIL; a missing `cargo` is exit 2.

**Retired: former Probe A** (`is_publicly_visible` classifies only `Approved`). It duplicated
the core unit test `feedbackmonk-core/src/moderation.rs::only_approved_is_publicly_visible`
(line ~165), which `cargo test` runs on every push.

## Adversarial self-test

Run 2026-09-30 by the scratch harness `mut.py` (session scratchpad
`mut2/public-board-moderation-gate/`), which copies `handlers/board.rs`, the repository
`src/` and the `board_*` tests into a scratch tree, applies one mutation, and runs
`oracle.py --root <scratch>`. The "old" column is the pre-hardening oracle (HEAD `4f1b88d`)
against the same mutation.

| Mutation | New | Old |
|---|---|---|
| M1 drop the approved filter from `list_public_board`'s second (count) query | exit 1 | exit 0 |
| M2 that filter present only as a SQL `--` comment | exit 1 | exit 0 |
| M3 that filter present only as a Rust `//` comment beside the query | exit 1 | exit 0 |
| M4 board.rs adds `state\n.feedback\n.list_for_end_user(..)` (multi-line, not allowlisted) | exit 1 | exit 0 |
| M5 board.rs aliases the handle: `let repo = &state.feedback; repo.list_public_board(..)` | exit 1 | exit 0 |
| M6 `UNION ALL` an unfiltered `FROM feedback` into `get_public_board_item`'s one literal | exit 1 | exit 0 |
| M7 vote-cast handler resolves the approved target after the `.board_votes` write | exit 1 | exit 1 |
| M8 board.rs deleted | exit 1 | exit 0 (PENDING) |
| M9 raw `sqlx::query_scalar(..).fetch_one(&state.pool)` in board.rs | exit 1 | exit 0 |
| restored (unmutated copy) | exit 0 | exit 0 |

Known static boundary: C29 inv. 2 (board-disabled project returns 404 on the read path) is
covered by Probe C (`board_moderation_gate.rs::board_disabled_project_404s`), not statically.
