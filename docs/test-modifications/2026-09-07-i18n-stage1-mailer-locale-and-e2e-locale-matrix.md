---
schema: test-modification-justification/v1
vouched_by: judge
judge_model_id: "claude-opus-5[1m]"
judge_verdict: VOUCH
judge_ruled_at: "2026-09-07T01:08:47Z"
judged_test_diff_sha256: "06cbc51506e5a51c22d3479d9e9d2bcf68c56760414ccfbe2b5d1dbc2bc81471"
judged_code_diff_sha256: "899905890d7dc74193473695069befd7276cdbbc8ec4ded432d17bde1ed0810f"
working_session_id: "collab-20260906-215851"
commit: ""
session_id: "collab-20260906-215851"
authored_at: "2026-09-07T01:08:47Z"
authored_by: judge
tests_modified:
  - path: "crates/feedbackmonk-api/tests/account_recovery.rs"
    change_type: extend
    lines_changed: 4
    description: "RecordingMailer's two Mailer methods take the new required `_locale` parameter; recorded assertions untouched."
  - path: "crates/feedbackmonk-api/tests/board_moderation_gate.rs"
    change_type: extend
    lines_changed: 4
    description: "StubMailer signature-only compile fix for the Mailer trait's new `locale` parameter."
  - path: "crates/feedbackmonk-api/tests/board_privacy_isolation.rs"
    change_type: extend
    lines_changed: 4
    description: "StubMailer signature-only compile fix for the Mailer trait's new `locale` parameter."
  - path: "crates/feedbackmonk-api/tests/board_vote.rs"
    change_type: extend
    lines_changed: 4
    description: "StubMailer signature-only compile fix for the Mailer trait's new `locale` parameter."
  - path: "crates/feedbackmonk-api/tests/board_vote_moderation_gate.rs"
    change_type: extend
    lines_changed: 4
    description: "StubMailer signature-only compile fix for the Mailer trait's new `locale` parameter."
  - path: "crates/feedbackmonk-api/tests/email_integration.rs"
    change_type: extend
    lines_changed: 28
    description: "Additive only (+28/-0): four new TenantRepo stub methods returning the untouched-tenant values (None/false) and `submitter_locale: None` on the EmailContext literal, so the existing English mailpit body assertions still bind unchanged."
  - path: "crates/feedbackmonk-api/tests/feedback_injection_corpus.rs"
    change_type: extend
    lines_changed: 4
    description: "StubMailer signature-only compile fix for the Mailer trait's new `locale` parameter."
  - path: "crates/feedbackmonk-api/tests/handlers.rs"
    change_type: extend
    lines_changed: 4
    description: "RecordingMailer signature-only compile fix; the (to, link) recording and its assertions are unchanged."
  - path: "crates/feedbackmonk-api/tests/host_tenant_binding.rs"
    change_type: extend
    lines_changed: 4
    description: "StubMailer signature-only compile fix for the Mailer trait's new `locale` parameter."
  - path: "crates/feedbackmonk-api/tests/me_feedback_delete.rs"
    change_type: extend
    lines_changed: 4
    description: "StubMailer signature-only compile fix for the Mailer trait's new `locale` parameter."
  - path: "crates/feedbackmonk-api/tests/me_feedback_export.rs"
    change_type: extend
    lines_changed: 4
    description: "StubMailer signature-only compile fix for the Mailer trait's new `locale` parameter."
  - path: "crates/feedbackmonk-api/tests/me_feedback_isolation.rs"
    change_type: extend
    lines_changed: 4
    description: "StubMailer signature-only compile fix for the Mailer trait's new `locale` parameter."
  - path: "crates/feedbackmonk-api/tests/me_feedback_reply_state.rs"
    change_type: extend
    lines_changed: 4
    description: "StubMailer signature-only compile fix for the Mailer trait's new `locale` parameter."
  - path: "crates/feedbackmonk-api/tests/router_submission_integration.rs"
    change_type: extend
    lines_changed: 4
    description: "StubMailer signature-only compile fix for the Mailer trait's new `locale` parameter."
  - path: "crates/feedbackmonk-api/tests/runner_e2e.rs"
    change_type: extend
    lines_changed: 4
    description: "StubMailer signature-only compile fix for the Mailer trait's new `locale` parameter."
  - path: "crates/feedbackmonk-api/tests/solicitation_integration.rs"
    change_type: extend
    lines_changed: 4
    description: "StubMailer signature-only compile fix for the Mailer trait's new `locale` parameter."
  - path: "crates/feedbackmonk-api/tests/submit_idempotency.rs"
    change_type: extend
    lines_changed: 4
    description: "StubMailer signature-only compile fix for the Mailer trait's new `locale` parameter."
  - path: "crates/feedbackmonk-api/tests/tier_enforcement_smoke.rs"
    change_type: extend
    lines_changed: 4
    description: "StubMailer signature-only compile fix for the Mailer trait's new `locale` parameter."
  - path: "crates/feedbackmonk-api/tests/work_order_routing.rs"
    change_type: extend
    lines_changed: 4
    description: "StubMailer signature-only compile fix for the Mailer trait's new `locale` parameter."
  - path: "crates/feedbackmonk-api/tests/work_order_state_machine.rs"
    change_type: extend
    lines_changed: 4
    description: "StubMailer signature-only compile fix for the Mailer trait's new `locale` parameter."
  - path: "admin-ui/e2e/public-board-a11y.spec.ts"
    change_type: extend
    lines_changed: 103
    description: "Pure insertion (+103/-0): en-US/de-DE/fa-IR locale matrix asserting <html lang>/dir, the language switcher and axe-cleanliness, plus ?lang= precedence, hostile-?lang= rejection and switcher persistence. Pre-existing a11y-smoke describe untouched."
  - path: "admin-ui/e2e/public-roadmap-a11y.spec.ts"
    change_type: extend
    lines_changed: 53
    description: "Pure insertion (+53/-0): the same locale matrix and a ?lang= precedence/no-persistence spec. Pre-existing a11y-smoke describe untouched."
code_modified:
  - path: "crates/feedbackmonk-api/src/email/mod.rs"
    change_type: api-change
    lines_changed: 26
    description: "FR-FBR-37: the `Mailer` trait's `send_verify_email` / `send_password_reset_email` gain a REQUIRED `locale: Locale` parameter (deliberately not Option-with-default); re-exports `resolve_account_locale` / `resolve_recipient_locale`."
  - path: "crates/feedbackmonk-api/src/email/mailpit.rs"
    change_type: new-behavior
    lines_changed: 188
    description: "Account-email bodies move to catalog-driven renderers keyed by locale; adds four NEW English byte-identity insta snapshots plus two locale-independent structural tests for a path that previously had no test at all."
  - path: "crates/feedbackmonk-api/src/email/env_smtp.rs"
    change_type: api-change
    lines_changed: 15
    description: "EnvSmtpMailer implements the new Mailer signature."
  - path: "crates/feedbackmonk-api/src/email/send.rs"
    change_type: new-behavior
    lines_changed: 90
    description: "EmailContext gains `submitter_locale: Option<String>`; adds the submitter -> tenant -> English locale ladder (`resolve_recipient_locale`) and two new unit tests pinning it."
  - path: "crates/feedbackmonk-api/src/email/templates.rs"
    change_type: new-behavior
    lines_changed: 174
    description: "Renderers take an explicit `Locale` and source literals from the catalog; the six pre-existing insta snapshots are byte-unchanged, call sites gain only `Locale::EN`."
  - path: "crates/feedbackmonk-repository/src/tenants.rs"
    change_type: api-change
    lines_changed: 86
    description: "FR-FBR-38: TenantRepo gains get_locale/set_locale/get_translate_outbound/set_translate_outbound (migration 00032) — the trait extension the email_integration.rs stub additions satisfy."
  - path: "crates/feedbackmonk-repository/src/feedback.rs"
    change_type: new-behavior
    lines_changed: 60
    description: "Captures and reads `feedback.submitter_locale` (C37)."
  - path: "crates/feedbackmonk-api/src/handlers/feedback.rs"
    change_type: new-behavior
    lines_changed: 55
    description: "Submit path captures the submitter's UI locale."
  - path: "crates/feedbackmonk-api/src/handlers/signup.rs, account_recovery.rs, admin_feedback.rs, promote.rs, admin_ops.rs, admin_tier.rs, mod.rs, capabilities.rs, main.rs, lib.rs"
    change_type: api-change
    lines_changed: 152
    description: "Call sites pass the resolved recipient locale into the mailer; new tenant_settings handler and capability advertisement."
  - path: "crates/feedbackmonk-i18n/src/lib.rs"
    change_type: new-behavior
    lines_changed: 30
    description: "Locale type, `t()` lookup, catalogs and Accept-Language parsing (C34/C35)."
  - path: "admin-ui/src/pages/board/PublicBoard.tsx, roadmap/PublicRoadmap.tsx, public/TenantHostLanding.tsx, App.tsx, shared/format.ts, styles/index.css"
    change_type: new-behavior
    lines_changed: 235
    description: "Public surfaces resolve locale from navigator.languages / ?lang= / stored choice, set <html lang> and dir, and render the language switcher — the behaviour the two e2e specs newly assert."
rationale_summary: "Trait-signature-driven mechanical test-double fix plus purely additive locale e2e coverage; every removed test line is one of the two Mailer signatures."
hypothesis_ledger_ref: ""
spec_change_ref: "FR-FBR-36 / FR-FBR-37 / FR-FBR-38 (docs/specs/SPECIFICATION.md); DEC-FBR-15, DEC-FBR-17; Contracts C34/C35/C37/C38"
---

# Test Modification Justification — Independent Judge Vouch (ARHG-06)

> **What this artifact is**: an independent, fresh-context judge — which did **not** write the code under
> examination and has no stake in the test passing — examined the test diff, the code diff, and the working
> agent's rationale, and ruled this **legitimate test evolution consistent with the development change**.
> It is the Phase 0.5 justification artifact for that change, and the working agent proceeded on this ruling
> instead of interrupting the user (DEC-190, amending DEC-111).
>
> **Authored by the judge, never by the working agent.** The Flag Integrity Rule is amended, not repealed:
> the judge is vouching source 3, alongside plan/spec pre-grant (ARHG-05) and the user's explicit word.
> The agent being graded still never vouches for itself and never writes this file.
>
> **This file is the whole artifact (ARHG-14, DEC-430).** The framework's vouch artifact set is closed: one
> record per VOUCH under `docs/test-modifications/`, and no ledger, index or JSONL beside it. If your project
> keeps an additional local audit surface, **that surface is your project's to enforce** — the ARHG-01
> pre-commit gate checks this record's presence and nothing else, so a row omitted from a local ledger is
> omitted silently. Enforce it in the same commit that stages this record, not with an oracle run afterwards.
> (This project keeps no such ledger as of this ruling.)
>
> **Honesty caveat (DEC-131 tier)**: authenticity here is enforced by role and schema, not cryptographically.
> The authority chain is auditable — the record names the judge, its model, the verdict, and hashes of the
> exact diffs judged — but it is not tamper-proof. What makes a bad VOUCH recoverable is that nothing
> downstream was removed: the ARHG-01 pre-commit gate, Phase 0.5 detection, the DEC-84 subordinate deletion
> deferral, and the PODS critic all still run.

## Judge verdict

**VERDICT: VOUCH** — by `claude-opus-5[1m]` at `2026-09-07T01:08:47Z`.

## Judge's reason

Every content removal in the 22-file test diff — 38 lines, after excluding the 22 `--- a/…` headers — is one
of exactly two lines, `async fn send_verify_email(&self, …)` or `async fn send_password_reset_email(&self, …)`,
each re-added verbatim with `, _locale: feedbackmonk_i18n::Locale` appended; a filter for removed lines that
are not those two signatures returns empty, and no assertion, expected value, fixture constant, `#[test]` fn
or `test.describe` block was removed anywhere. The code diff independently compels this: `src/email/mod.rs`
adds a required `locale: Locale` parameter to both `Mailer` trait methods, which in Rust makes every `impl
Mailer` a hard compile error, so the doubles would have needed the identical edit even if the suite had been
green. The two non-mechanical test edits check out the same way — `tests/email_integration.rs` is +28/−0,
adding stubs for four `TenantRepo` methods newly declared in `crates/feedbackmonk-repository/src/tenants.rs`
and returning the untouched-tenant values `None`/`false`, so its existing English body assertions still bind;
the two Playwright specs are pure insertions (+103/−0 and +53/−0) ahead of untouched pre-existing describes,
and their `test.skip(!FAKE_API, …)` is the files' own idiom already present at HEAD, not a new disabling.
Nothing was re-baselined and verification strictly increases: no snapshot file is modified or deleted in the
tree, the six pre-existing `insta` snapshots in `src/email/templates.rs` appear as unchanged context with only
`Locale::EN` added at the call sites, and `src/email/mailpit.rs` adds four new English byte-identity snapshots
for account emails that previously had no test at all plus structural no-raw-key assertions across every
shipped locale.

## What the judge examined

- **Test diff** (`sha256: 06cbc51506e5a51c22d3479d9e9d2bcf68c56760414ccfbe2b5d1dbc2bc81471`): 551 lines over
  22 existing test files — 20 Rust integration-test files in `crates/feedbackmonk-api/tests/` (19 of them
  signature-only, `email_integration.rs` additive-only) and 2 Playwright a11y specs (additive-only). 60 `-`
  lines total, 22 of which are file headers.
- **Code diff** (`sha256: 899905890d7dc74193473695069befd7276cdbbc8ec4ded432d17bde1ed0810f`): the FR-FBR-36/37/38
  i18n Stage-1 change — the `Mailer` trait's required `locale` parameter, catalog-driven account-email and
  notification-email rendering, the submitter→tenant→English locale ladder, the `TenantRepo` language-settings
  methods, and the public admin-UI locale resolution/switcher the e2e specs assert. The supplied patch covers
  10 files; the judge additionally read the full working-tree code diff via `git diff` (29 further source
  files, incl. `crates/feedbackmonk-repository/src/tenants.rs` and `src/email/mod.rs`) to verify the trait
  and `TenantRepo` changes the test edits claim to follow from.
- **Working agent's stated rationale**: that every test-double edit is a mechanical compile fix forced by the
  trait signature change, that no assertion/expectation/fixture was weakened, and that the e2e specs were
  extended rather than altered — a claim examined against the diffs, not accepted on its own word. One
  imprecision found and immaterial: the rationale says the feedback-notification *send methods* gained a
  locale parameter, whereas they in fact receive it via the new `EmailContext.submitter_locale` field.

## Working agent's rationale (as submitted)

1. `crates/feedbackmonk-api/tests/*.rs` (20 files, CLAUDE-C): the `Mailer` trait gained a required
   `locale: feedbackmonk_i18n::Locale` parameter on `send_verify_email` / `send_password_reset_email` (and the
   feedback-notification send methods) so every email renders in the recipient's language (FR-FBR-37). Every
   test double implementing `Mailer` was patched with an unused `_locale` parameter — a mechanical compile fix
   from the trait signature change. The worker states: no assertion, expectation or fixture value was
   weakened; only new tests were added (`tests/submit_locale.rs`, `tests/email_locale.rs`,
   `tests/tenant_locale_settings.rs`) and English byte-identity snapshots were kept unchanged (six pre-existing
   insta snapshots pass unchanged; four new snapshots added for previously untested account emails).
2. `admin-ui/e2e/public-board-a11y.spec.ts` and `public-roadmap-a11y.spec.ts` (CLAUDE-B): parametrised over a
   locale matrix (en-US / de-DE / fa-IR) and extended with assertions on `<html lang>`/`dir`, the language
   switcher, `?lang=` precedence and hostile-`?lang=` rejection. Existing assertions kept; 13 pre-existing
   specs still pass; 22 specs added (35 total).

---

**Requirement**: ARHG-06. **Decision**: DEC-190. **Judge agent**: `~/.claude/agents/test-mod-judge.md`.
**Consumer**: `~/.claude/segments/-finalize/phase0.5-test-mod-gate.md` § 0.5.16.
