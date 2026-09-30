# feedback-erasure-completeness

## Summary

Proves from code that both right-to-erasure paths (`DELETE …/me/feedback/{id}` and the "forget me"
`DELETE …/me`) leave nothing of the submitter behind: no object bytes, no un-cascaded child rows, no
end-user-keyed table, no derived prose, and no row outside the caller's own (tenant, project, sub).
A "the row is gone" test cannot see any of these. Come here when a probe is red, or before touching
`handlers/me_feedback.rs`, the erasure functions in `feedbackmonk-repository`, or a migration that
adds a table referencing `feedback` or keyed by an end-user identity.

## Probes

Run `python oracle.py` (static, well under a second of work) or `python oracle.py --full`, which also
runs `cargo test -p feedbackmonk-api --test me_feedback_delete` against a real database. Exit 0 PASS,
1 FAIL, 2 environment error. `--root <path>` inspects another tree (used by the self-test below).

| Probe | Where | What must hold |
|---|---|---|
| A | `handlers/me_feedback.rs`, `repository/attachments.rs` | Both handlers: `authenticate_parts` → list keys → `storage.delete(..)?` inside a `for` over those keys → row delete keyed to `claims.sub`. Per-item: `get_for_end_user(.., claims.sub, ..).await?` runs before any listing or purge (`resolve_feedback_uuid` is not sub-scoped). Bulk: `list_storage_keys_for_end_user(.., claims.sub)`, whose SQL binds `end_user_sub`, `tenant_id` and `project_id` to the caller. |
| B | `migrations/*.sql`, parsed into a final schema | Every FK to `feedback` (with or without `(id)`, inline, table-level or `ALTER … ADD CONSTRAINT`) is `ON DELETE CASCADE` or `SET NULL`; the raw count of `REFERENCES feedback` equals the parsed FKs (an unreadable form fails, not passes); every `*feedback_id` / `*feedback_uuid` column outside `feedback` has an FK to it. |
| C | `repository/feedback.rs` | Every `DELETE` in `delete_for_end_user` / `erase_all_for_end_user` binds, **in its own WHERE** (SQL comments stripped, no top-level `OR`), `tenant_id` to `scope.tenant_id()`, `project_id` to `scope.project_id()` and an end-user key to `end_user_sub` — or is `DELETE FROM feedback WHERE id = $n` with `$n` = `<row>.id`, `<row>` bound by an earlier `SELECT … FROM feedback` scoped the same way. |
| D | `repository/feedback.rs` | `scrub_cluster_derived_text` clears `feedback_clusters.summary`, `recommendations.body`, `work_orders.instructions`, `analysis_sweeps.digest_summary`; both erasure fns call it. |
| E | migrations × `erase_all_for_end_user` | Every table carrying an end-user identity column (`end_user_*`, `*_sub`, `voter_id`, `submitter_id`) is erased by a DELETE scoped as in C and keyed on that column — or is listed in `CASCADE_COVERED` (in `oracle.py`, with a rationale) **and** verified to hang off `feedback` by a `NOT NULL … ON DELETE CASCADE` FK. Today: `feedback`, `feedback_solicitations`, `roadmap_votes`, `feedback_board_votes` by DELETE; `submit_idempotency` by cascade. |
| --full | `tests/me_feedback_delete.rs` | The behavioral test passes; a missing test file is a FAIL. |

There is no allowlist. A new end-user-keyed table fails E until `erase_all_for_end_user` erases it.

## Adversarial self-test

Harness: a scratch copy of `me_feedback.rs`, `feedback.rs`, `attachments.rs`, the delete test and
`migrations/`, one mutation at a time, run with `--root`. Last run 2026-09-30; the unmutated copy
exited 0 before and after.

| # | Mutation | Exit |
|---|---|---|
| C1 | owner key moved out of the bulk `DELETE FROM feedback` WHERE into a trailing Rust comment | 1 |
| C2 | `end_user_sub = $4` dropped from the `SELECT … FOR UPDATE` the per-item delete-by-id relies on | 1 |
| C3 | per-item `DELETE … WHERE id = $1` bound to an id not from the scoped SELECT | 1 |
| C4 | top-level `OR cluster_id IS NULL` added to the bulk DELETE | 1 |
| C5 | owner key moved into a SQL `--` comment inside the DELETE literal | 1 |
| B1 | new table: `parent UUID NOT NULL REFERENCES feedback` (no column, default NO ACTION) beside a CASCADE FK | 1 |
| B2 | `REFERENCES feedback ON DELETE RESTRICT` (no column) | 1 |
| B3 | a `feedback_id UUID NOT NULL` column with no FK | 1 |
| B4 | `REFERENCES feedback(id),` followed by a CASCADE FK on the next line (the old 60-char window passed this) | 1 |
| B5 | `ALTER TABLE attachments ADD CONSTRAINT … FOREIGN KEY (feedback_id) REFERENCES feedback (id)` | 1 |
| A1 | bulk handler: `erase_all_for_end_user` moved before the key listing and purge | 1 |
| A2 | per-item handler: ownership check moved after the byte purge | 1 |
| A3 | per-item handler: ownership check result discarded (`let _ = … .await;`) | 1 |
| A4 | bulk handler: purge error swallowed (`let _ = state.storage.delete(key).await;`) | 1 |
| A5 | `list_storage_keys_for_end_user` loses `f.end_user_sub = $3` | 1 |
| E1 | `erase_all_for_end_user` stops deleting `feedback_board_votes` | 1 |
| E2 | `roadmap_votes` deleted by `item_id::text = $3` instead of `voter_id` | 1 |
| E3 | new migration adds `mut_prefs(tenant_id, project_id, end_user_sub)` that nothing erases | 1 |
| E4 | `submit_idempotency.feedback_id` becomes nullable `ON DELETE SET NULL` (CASCADE_COVERED no longer holds) | 1 |
| D1 | `recommendations.body = ''` replaced by `body = body /* body = '' */` | 1 |
| F1 | `--full` with `tests/me_feedback_delete.rs` deleted | 1 |

D1 first came back 0: SQL comments inside the string literals were not stripped. The oracle now
strips `--` and `/* */` from every SQL literal before reading it, and C5 was added to hold that.
