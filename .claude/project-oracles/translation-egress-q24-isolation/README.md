# translation-egress-q24-isolation

## Summary

This oracle defends two parts of the FR-FBR-30 translation pipeline, and it checks both from the code:

- **Privacy posture (DEC-FBR-IMPL-26).** The translation provider defaults to `off`, so sending text to a translation service is something the operator must opt into.
- **Q24 read isolation (DEC-FBR-IMPL-25).** No public, end-user or board surface ever reads `body_translated`.

Come here when you touch the provider constructor in `main.rs`, any SQL or struct that names `body_translated`, the handler of a public router, or the `body_tsv` migration.

## Probes

Default mode is static and takes about 1 s. It exits 0 on PASS, 1 on FAIL and 2 on an environment error. `--root <path>` scans another tree.

| Probe | What must hold |
|---|---|
| A | In `build_translation_provider`: `FEEDBACKMONK_TRANSLATION_PROVIDER` defaults to `"off"` at the env read, and the arm carrying `"off"` is `=> Ok(None)`. Any catch-all arm (`_` / `other`) returns an error. Outside that fn, no shipped source constructs `DeepLTranslator::new` or `LibreTranslateTranslator::new`. Only the balanced `#[cfg(test)] mod` bodies are exempt. |
| B | Every occurrence of `body_translated` sits inside an allow-listed fn or an allow-listed struct. Occurrences are found in every crate's `src/`, with comments stripped. <br>Allowed fns: `set_translation`, `list_member_bodies_for_cluster`, `get_translation_for_admin` and `get_admin_feedback`.<br>Allowed structs: `AdminTranslationView` and `FeedbackDetailResponse`.<br>The check is occurrence-based, so it also catches a module-level `const` SQL string, a new struct field, or a generic fn. |
| C | Only `set_translation` writes the column (`SET body_translated`). No `INSERT INTO feedback` names it. `.set_translation(` is called only from `translation/worker.rs`. |
| D | The latest `body_tsv` generated column sources from `coalesce(body_translated, body)`. |
| E | A router is public if it is merged in `build_app` without a `bind_admin_routes` wrapper; this includes the base `health_router`. The file defining each public router must not name `body_translated`, an allow-listed translation struct, or a repository translation reader. Files are found through `pub fn`, or through a `use .. as` alias in `lib.rs`/`main.rs`. A public router with no definition the oracle can find is a FAIL. |
| `--full` | Runs `cargo test -p feedbackmonk-api --test translation_worker`. If that test file is missing, the result is FAIL (it used to be a vacuous PENDING). If `cargo` is missing, the exit code is 2. |

What changed on 2026-09-30:

- **Probe A's off-arm check was fixed.** It was `no Ok(None) arm AND '"off"' not in body`. The default string `"off"` is always in the body, so the check could never fail.
- **Probe A was tightened.** It now anchors the default to the env read. Previously a DOTALL fallback matched any later `unwrap_or_else(|_| "off")`. It also checks the catch-all arm and looks for provider constructors made outside the fn.
- **Probe B was rewritten.** It went from scanning fn bodies to checking every occurrence.
- **Probe E was added.**
- **The PENDING branch for `--full` was removed.**
- **The comment stripper was replaced.** The new one understands string literals and preserves length.

## Adversarial self-test

Run on 2026-09-30. The shipped `src/` of every crate, `tests/translation_worker.rs` and `migrations/` were copied into `scratchpad/mut2/translation-egress-q24-isolation/tree`. Each mutation below was applied there and the oracle was run with `--root`. The pre-change oracle was run on the same tree for comparison. After each mutation the file was restored and the oracle re-run.

| # | Mutation | New oracle | Old oracle |
|---|---|---|---|
| M1 | `"" \| "off" => Ok(None)` changed to `=> Ok(Some(Arc::new(DeepLTranslator::new(..)?)))` | exit 1 (A) | exit 0 (missed) |
| M2 | Default `"off"` changed to `"deepl"` | exit 1 (A) | exit 1 |
| M3 | Added a `_ => Ok(Some(DeepLTranslator..))` arm ahead of `"libretranslate"` | exit 1 (A) | exit 0 (missed) |
| M4 | `DeepLTranslator::new(..)` constructed in a new fn in `email/send.rs`, placed after that file's test module | exit 1 (A) | exit 0 (missed) |
| M5 | `pub body_translated: Option<String>` added to the public `BoardItemResponse` | exit 1 (B, E) | exit 0 (missed) |
| M6 | Module-level `const BOARD_SQL = "SELECT coalesce(body_translated, body) .."` in repository `feedback.rs` | exit 1 (B) | exit 0 (missed) |
| M7 | Public `board.rs` handler calls `state.feedback.get_translation_for_admin(..)` | exit 1 (E) | exit 0 (missed) |
| M8 | New repository fn containing `UPDATE feedback SET body_translated = $1` | exit 1 (B, C) | exit 1 |
| M9 | Generic `fn leak<'t>(..)` referencing `body_translated` | exit 1 (B) | exit 1 |
| M10 | `tests/translation_worker.rs` deleted, then `--full` | exit 1 | exit 0 (PENDING) |

After every restore, the new oracle returned exit 0 on the scratch tree. A `diff -rq` showed the tree byte-identical to the repo. `--root` pointing at a directory with no `crates/` exits 2.
