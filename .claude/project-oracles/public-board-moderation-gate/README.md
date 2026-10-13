# public-board-moderation-gate

## Summary

Proves from code that no public-board endpoint (read or vote) can return or act on a
feedback row whose `moderation_status` is not `approved`, and that the board wire shape
carries no submitter PII (C29 inv. 1 and 3, plan D2). The moderation gate is the trust
boundary between public submission and public exposure. Come here when you touch
`handlers/board.rs`, a `*board*` repository read, a board signal/note repository method
(`board_signals.rs`, `board_notes.rs`), or the board wire shape.

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
  Calls through the board sub-handles, `.board_votes.signals()` / `.notes()` / `.sessions()`
  (the list and detail reads, not-needed set/clear, priority-list get/put; C43, FR-FBR-61),
  are seen too, and are verified rather than allowlisted. The oracle finds the impl that backs
  each handle from the trait its accessor returns (`fn signals(&self) -> &dyn BoardSignalRepo`),
  and checks the called method's body and every same-file helper it reaches. **(g1)** No SQL
  literal reads `FROM/JOIN feedback` without the literal approved filter once per reference.
  **(g2)** Every write, meaning an `INSERT`/`UPDATE`/`DELETE` literal that does not itself carry
  the approved filter or a call to a helper that makes one, comes after a call to
  `resolve_pool_board_items`. That gate fn must itself be a discovered board read fn, so check
  (1) pins its SQL. The one exception is a withdraw (`clear_not_needed`): every literal must be
  a `DELETE` with no `RETURNING`, it may call no helper, and the handler must discard the
  result (a bare `state.board_votes.<h>().<m>(..).await?;` statement). A response that cannot
  depend on whether a hidden row exists confirms nothing. Verified method bodies join the scope
  of checks (1b), (2) and (3).
- **(1)** Every SQL string literal in each board read fn must carry the literal
  `moderation_status = 'approved'` at least once per `FROM/JOIN feedback` it contains.
  Rust comments and SQL `--` / `/* */` comments are stripped first, so a filter present only in
  a comment does not count. A bound parameter does not count.
- **(1b)** no `'pending'` / `'rejected'` literal in scope. **(2)** no submitter-PII column in
  scope. **(3)** no `feedback_replies` in scope.
- **(4)** every board.rs handler using `.board_votes` calls `ensure_board_enabled` before its
  first use. Every direct `.board_votes.<method>(` write (vote cast and retract) is preceded by
  `resolve_approved_board_*`. A sub-handle call is gated by its repository method, verified in
  (0b). Any other `.board_votes` form, such as an aliased handle, fails.

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

**v1.2.0 widening (2026-10-13, end-user prioritization wave 2).** The v1.1.0 oracle
failed every handler that reaches `.board_votes` through a sub-handle, because those
handlers gate inside the repository rather than calling `resolve_approved_board_*`. v1.2.0
verifies that repository gate instead. The mutations were applied in place to the working
tree and reverted, and the oracle was run against the real tree each time:

| Mutation | v1.2.0 |
|---|---|
| M10 `board_signals.rs::set_not_needed` drops its `resolve_pool_board_items(..)` call (a fixed `PoolItem` stands in) | exit 1: `set_not_needed: writes before (or without) calling resolve_pool_board_items` |
| M11 `board.rs::public_board_vote_cast` skips `resolve_approved_board_feedback_id` (the id is parsed from the short code) | exit 1: `public_board_vote_cast: ... before the .board_votes.cast write` |
| restored | exit 0 |

**Priority-list write (2026-10-13, end-user prioritization wave 1, no oracle change).** Same
harness and method; the oracle unchanged at v1.2.0.

| Mutation | v1.2.0 |
|---|---|
| M12 `board_signals.rs::save_priority_list` drops its `resolve_pool_board_items(..)` call (the codes map to fixed `PoolItem`s) | exit 1: `board.rs:616 (public_board_priorities_put): .board_votes.signals().save_priority_list(..) ... writes before (or without) calling resolve_pool_board_items` |
| restored | exit 0 |

**Re-running the mutations.** M12 is recorded in `mutations.json` and replayed by
`mutations.py`, which copies `crates/` and `migrations/` from the source tree into a temporary
directory, checks the unmutated copy exits 0, applies each row alone to the copy, runs this
`oracle.py --root <copy>` and requires exit 1, then restores the copy. The working tree is only
read. From the repository root:

    python .claude/project-oracles/public-board-moderation-gate/mutations.py [--root <tree>] [--only M12 ...]

Exit 0 means the clean copy is green and every row red. M1–M11 predate the recorded harness and
are not in `mutations.json`.

Known static boundary: C29 inv. 2 (board-disabled project returns 404 on the read path) is
covered by Probe C (`board_moderation_gate.rs::board_disabled_project_404s`), not statically.
