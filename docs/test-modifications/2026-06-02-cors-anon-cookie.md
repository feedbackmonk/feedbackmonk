---
schema: test-modification-justification/v1
commit: ""
session_id: "single-session"
authored_at: 2026-06-02T00:00:00Z
authored_by: "primary"
tests_modified:
  - path: crates/feedbackmonk-api/tests/cors_preflight.rs
    change_type: extend
    lines_changed: 178
    description: New integration test file — preflight-not-405, echo-origin + credentials, disallowed-origin rejection, empty-allowlist-blocks-all.
  - path: crates/feedbackmonk-api/src/handlers/feedback.rs
    change_type: rewrite
    lines_changed: 6
    description: resolve_anon_cookie_mints_when_absent now asserts SameSite=None + Secure (was SameSite=Lax) — matches the intentional cross-site cookie change.
  - path: crates/feedbackmonk-api/src/cors.rs
    change_type: extend
    lines_changed: 30
    description: New unit tests for parse_origins + non-panicking layer construction (inline, ship with the new module).
code_modified:
  - path: crates/feedbackmonk-api/src/cors.rs
    change_type: new-behavior
    lines_changed: 130
    description: New credentialed CORS layer (echo-origin allowlist from FEEDBACKMONK_CORS_ORIGINS) for the public widget endpoints.
  - path: crates/feedbackmonk-api/src/main.rs
    change_type: new-behavior
    lines_changed: 30
    description: Read FEEDBACKMONK_CORS_ORIGINS; apply the CORS layer to the submission + attachments routers.
  - path: crates/feedbackmonk-api/src/handlers/feedback.rs
    change_type: new-behavior
    lines_changed: 3
    description: Anon-dedup cookie SameSite=Lax -> SameSite=None; Secure so it survives the cross-site credentialed submit.
rationale_summary: "New CORS layer + cross-site anon cookie unblock the GitCellar widget embed; cookie test updated to the new (intentional) attributes."
hypothesis_ledger_ref: ""
spec_change_ref: "DEC-FBR-IMPL-09"
---

# Test Modification Justification

## Why both tests and code changed in this commit

### 1. What behavior changed?

Two new contract-level behaviors were introduced (DEC-FBR-IMPL-09):

1. The public credentialed widget endpoints (`POST …/feedback` and `POST …/feedback/{id}/attachments`)
   now carry a **CORS policy**. Previously they had none: a browser preflight `OPTIONS` hit a POST-only
   route and was answered `405` with no `Access-Control-*` headers, so the browser blocked every
   cross-origin submission. They now answer the preflight and decorate responses with an echo-origin
   `Access-Control-Allow-Origin` (never `*`) + `Access-Control-Allow-Credentials: true`, gated by the
   `FEEDBACKMONK_CORS_ORIGINS` allowlist. This is the long-planned implementation of DEC-FBR-04's
   "Domain allowlist for widget embed (CORS …)".
2. The anonymous-dedup cookie changed from `SameSite=Lax` to **`SameSite=None; Secure`**. The anonymous
   submit path uses `fetch(credentials: "include")`; a `SameSite=Lax` cookie is dropped by the browser on
   a cross-site request, so it would never travel — silently disabling per-cookie dedup (FR-FBR-06).
   `SameSite=None` requires `Secure`.

Both are deliberate behavior changes, recorded in `docs/specs/DECISIONS.md` → DEC-FBR-IMPL-09
(`spec_change_ref`).

### 2. Why was the existing test outdated, incorrect, or incomplete?

Only one existing test was modified: `resolve_anon_cookie_mints_when_absent` in `feedback.rs`. It asserted
`SameSite=Lax` — the *old* cookie attribute. Item 2 above intentionally changed that attribute to
`SameSite=None; Secure` because `Lax` is wrong for the cross-site embed the widget is built for (the cookie
would be dropped). The test asserted the now-superseded attribute, so leaving it unchanged would assert the
*old, incorrect-for-cross-site* behavior. The test was updated to assert the new attributes **and** to add a
negative assertion (`!contains("SameSite=Lax")`) so a regression back to `Lax` is caught.

### 3. Why is the new test correct?

- `feedback.rs` cookie test: now asserts the cookie string contains `SameSite=None` **and** `Secure` **and
  not** `SameSite=Lax`. This is the exact attribute set required for a cross-site credentialed cookie; the
  invariant is "the minted anon cookie is storable/sendable in a third-party context", which is strictly
  stronger and more correct than the prior "cookie is Lax" assertion.
- `tests/cors_preflight.rs` (new, not a rewrite of anything): asserts the *contract*, not the
  implementation — (a) a preflight from an allowed origin is **not** `405` and carries the CORS headers;
  (b) a credentialed response echoes the **specific** origin and never `*`, with `Allow-Credentials: true`;
  (c) a **disallowed** origin receives **no** `Access-Control-Allow-Origin` (so the browser blocks it); and
  (d) an **empty** allowlist (the unset-env default) blocks all cross-origin. These are independent of how
  the layer is built — they would pass for any correct CORS implementation and fail for the pre-fix `405`
  behavior. The layer was verified to fail-then-pass: the disallowed/empty cases prove the allowlist is
  actually enforced, not vacuously open.
- `cors.rs` unit tests: assert `parse_origins` trims/drops empties and that `public_cors_layer` does not
  panic for empty or non-empty allowlists (the `tower_http` credentials+wildcard panic guard).

### 5. Adversarial check

`tests/cors_preflight.rs::preflight_from_disallowed_origin_gets_no_allow_origin` and
`empty_allowlist_blocks_all_cross_origin` are the adversarial cases: they assert the allowlist actually
*rejects* (no `Access-Control-Allow-Origin` emitted) for an origin not on the list and when the list is
empty. A reward-hacking "make CORS pass" implementation that returned `*` unconditionally would fail both —
and would also be invalid with credentials. The `credentialed_request_echoes_specific_origin_never_wildcard`
test explicitly asserts `acao != "*"`, catching exactly that shortcut.
