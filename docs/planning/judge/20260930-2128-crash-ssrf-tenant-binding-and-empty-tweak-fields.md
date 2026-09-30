---
schema: judge/1
verdict: VOUCH
vouched_by: judge
judge_model: claude-opus-5-5
judge_seat: same-model
ruled_at: 2026-09-30T21:28:02Z
working_session: 875e346e-f50a-4eb2-bfda-6d482ce62811
diff_form: staged
test_diff_sha256: f76079105d6c8e2a596e397a5d0f77eb2616224ceea56a96e4ecfe5d3d63c309
code_diff_sha256: f40c4854c52dcb145486bfd9a37cb1645bd0e3b0486268f149db26fd85a70423
tests: ["crates/feedbackmonk-api/src/crash_correlation.rs", "crates/feedbackmonk-api/tests/admin_crash_detail.rs", "admin-ui/src/pages/autopilot/__tests__/RecommendationCard.test.tsx"]
---

## Reason

No existing assertion was weakened or removed. The one changed assertion, in glitchtip_event_url_is_sentry_compatible, compares the same expected URL string through .unwrap().as_str(), because the code diff changes event_url to return Option<reqwest::Url>. The unwrap also makes the test stricter, since it now fails if a valid id is rejected. In admin_crash_detail.rs, each existing test only gains a third argument, its own admin's &tscope, at the app() call. That follows from the code diff adding CrashState.tenant_id and a handler gate (Some(c) if ours), and every existing status and call-count assertion is kept exactly. Everything else is new coverage that directly exercises the new code paths: the event-id and path-segment SSRF gate with its no-I/O proof, the cross-tenant tracker refusal, and the empty tweak fields with overrides that carry only typed text. These changes would be correct even if every old test had been passing.

## Rationale submitted

(1) crash_correlation.rs (SSRF fix). `event_url` now returns `Option<reqwest::Url>`, built from encoded path segments, and is `None` for anything that is not a plain event id. The existing test `glitchtip_event_url_is_sentry_compatible` compares against the same expected URL string, now through `.unwrap().as_str()`; that is a type adaptation, and the assertion is unchanged. Three tests are new: `event_ids_are_plain_tokens_only`: path, query, fragment, space, percent-escape and over-length ids are rejected; `event_url_stays_inside_the_events_route`; `a_path_shaped_id_is_never_requested`: this proves no I/O happens, because it answers NotFound where a real request to 127.0.0.1:9 would answer Unavailable. (2) admin_crash_detail.rs. `CrashState` gained `tenant_id`, because the tracker now serves only the tenant that owns it. The helper `app()` takes the owning tenant scope, and every existing test passes its own admin's scope, so their assertions are unchanged. The new test `the_tracker_serves_only_the_tenant_that_owns_it` shows another tenant's own crash-linked row answers unavailable, with the tracker never called. (3) RecommendationCard.test.tsx. The Tweak dialog's override fields now start empty rather than pre-filled with the model-written recommendation. The existing test `sends authoritative owner_overrides when the owner tweaks` is unchanged and still passes. Two tests are new: the fields start empty and only typed text is sent; tweaking nothing sends no overrides. All pass: crash_correlation 11, admin_crash_detail 6, and 39 autopilot tests.
