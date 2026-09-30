---
schema: judge/1
verdict: VOUCH
vouched_by: judge
judge_model: claude-opus-5-5
judge_seat: same-model
ruled_at: 2026-09-30T20:37:26Z
working_session: 875e346e-f50a-4eb2-bfda-6d482ce62811
diff_form: staged
test_diff_sha256: bfec4289d698de14721b2cf9a002640e22430b3a55055e04ecdc28ffecbb18b0
code_diff_sha256: 76908aae1855fe5567947a87cb806d50fa6a324eacf5f67bfb55f3d802d30816
tests: ["crates/feedbackmonk-runner/src/prompt.rs", "crates/feedbackmonk-api/tests/feedback_injection_corpus.rs", "crates/feedbackmonk-api/tests/admin_crash_detail.rs", "admin-ui/src/components/CrashBanner.test.tsx"]
---

## Reason

In prompt.rs `assemble_keeps_feedback_out_of_the_trusted_layer` and corpus case (g) assertion (3), the positive check "trusted layer contains the order's instructions" is replaced by three stricter checks: the trusted layer contains DERIVED_TASK, does not contain the copied title/instructions, and the envelope does contain them. The code diff to `assemble` implements exactly this split, which DEC-FBR-IMPL-33 records as the owner's decision before any failure. The change narrows what reaches the trusted layer, so the test is stricter, not weaker. The owner-authored path keeps its unchanged test asserting the instructions stay trusted with an empty envelope, and all the other assertions (DEC-84, steering absent, delimiters, render order, overrides) are unchanged. admin_crash_detail.rs and CrashBanner.test.tsx are new files that only add coverage for the DEC-FBR-IMPL-32 endpoint and banner, with concrete assertions and no skips.

## Rationale submitted

The owner decided two product changes today.

DEC-FBR-IMPL-33 changes the runner prompt's trust rule. A recommendation-grounded work order's title and instructions are copied from the model-written recommendation, and the owner cannot edit them. So they now go inside the untrusted envelope, and the trusted layer carries a fixed DERIVED_TASK statement instead. Owner-authored orders (recommendation: None) are unchanged. Two existing assertions encoded the old rule, and they now assert the opposite:
- prompt.rs `assemble_keeps_feedback_out_of_the_trusted_layer` used to assert the trusted layer contains the order's instructions.
- corpus case (g) assertion (3) used to assert the trusted layer carries "the owner-approved instructions".

In both, the trusted layer must now contain DERIVED_TASK and must NOT contain the copied text, and the copied text must be in the envelope. Every other assertion is unchanged: DEC-84 present, steering absent from the trusted layer, envelope delimiters, and render order. A new unit test, `injection_surviving_into_the_recommendation_title_stays_data`, covers an injection in the copied title and instructions.

DEC-FBR-IMPL-32 wires crash correlation.
- The new admin_crash_detail.rs covers the endpoint's linked, unconfigured, down, not-found and none states, plus 401 without a session and 404 for another tenant (with the tracker not called).
- The new CrashBanner.test.tsx covers rendering, the safe-URL rule and the degrade paths.

All pass locally: runner 63, the corpus 10, admin_crash_detail 5, and CrashBanner 6. The drawer test is unchanged and passes.
