# approval-gate-enforcement

## Summary

Defends Contract C22 invariant 1 (FR-FBR-25a / FR-FBR-22): no work order reaches an execution
state (`dispatched` and after) without a prior owner-authored `approved` event in the
`work_order_events` ledger. This is the boundary between public feedback and code execution, and a
happy-path test cannot prove the absence of a bypass. Come here when you touch the work-order
transition handler, the work-order repository, or any SQL that mentions `work_orders`.

## Probes

Static probes read comment-stripped source (a gate that is commented out does not count) and
exclude `#[cfg(test)]` items. `--root <path>` points the oracle at another tree (used by the
self-test below). Exit 0 PASS, 1 FAIL, 2 environment error.

- **A — the gate.** In `crates/feedbackmonk-api/src/handlers/work_orders.rs`, `fn
  transition_work_order` must contain, at the top level of its body and before the
  `transition_in_executor` write, `if to.is_execution_state() && !state.work_order_events
  .has_approved_event(scope, work_order_id).await? { return Err(... ApprovalRequired ...) }`;
  `to` must not be rebound between the gate and the write, and the write must record
  `to_state: to`. The pattern is deliberately exact: a refactor of the gate must update this
  oracle in the same change.
- **B — the sole writer of `work_orders.state`.** Across `crates/*/src`: SQL that sets
  `work_orders.state` appears only in the repository's `transition_in_executor` and
  `update_state_in_executor`; no `UPDATE work_orders` SQL at all outside the repository crate; no
  `INSERT INTO work_orders` names `state` (orders are born `draft` by the column default,
  migration 00014); `.transition_in_executor(` is called only from `transition_work_order`; and
  `.update_state_in_executor(` (the unpaired setter kept for the bypass-resistance test corpus) is
  called from no production code. If either expected writer disappears the probe fails, because
  its model of the code is stale.
- **C — behaviour (`--full`).** Runs `cargo test -p feedbackmonk-api --test
  work_order_state_machine`. A missing test file is a FAIL.

The state-machine table itself (`is_execution_state`, `Approved` as the only predecessor of
`Dispatched`) is proven by the core unit tests `execution_states_are_exactly_dispatch_and_after`
and `approval_is_the_only_gate_into_execution` (`crates/feedbackmonk-core/src/work_order.rs`), so
this oracle no longer duplicates them. The earlier PENDING branches, for a handler or test that
did not exist yet, are gone: both exist, so their absence fails.

## Adversarial self-test

Run 2026-09-30 against a copy of `crates/*/src` plus the state-machine test, via `--root`. Each
mutation was applied alone; every one exited **1**, and the restored tree exited **0**.

| # | Mutation | Caught by |
|---|---|---|
| M1 | gate removed from `transition_work_order` | A — no live gate |
| M2 | gate commented out with `//` | A — no live gate |
| M3 | gate wrapped in `/* */` | A — no live gate |
| M4 | `&&` weakened to `\|\|` | A — no live gate |
| M5 | gate nested under `if false { }` | A — nested (depth 2) |
| M6 | gate moved after `tx.commit()` | A — write precedes gate |
| M7 | `let to = WorkOrderState::Dispatched;` after the gate | A — `to` rebound |
| M8 | `UPDATE work_orders SET state = 'dispatched'` added in `handlers/admin_ops.rs` | B — SQL outside repository |
| M9 | second repository fn `force_dispatch` with `SET updated_at = now(), state = ...` | B — unapproved writer |
| M10 | `state` added to the `INSERT INTO work_orders` column list | B — insert names state |
| M11 | handler calls `.update_state_in_executor(` instead | A and B |
| M12 | `transition_in_executor` called from another handler fn | B — caller not the gate |
| M13 | handler file deleted | A — missing file |
| M14 | `--full` with `tests/work_order_state_machine.rs` deleted | C — missing file |

Default static run: 0.7–1.1 s.
