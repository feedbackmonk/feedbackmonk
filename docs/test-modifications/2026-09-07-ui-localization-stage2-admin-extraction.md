---
schema: test-modification-justification/v1
vouched_by: judge
judge_model_id: "claude-opus-5[1m]"
judge_verdict: VOUCH
judge_ruled_at: "2026-09-07T07:31:03Z"
judged_test_diff_sha256: "59aa681b1580abd482fb4db04f123290be88a5468c5990ab3ef8789c2cc31861"
judged_code_diff_sha256: "6a8b35193f7c3c5319f4dc6454d68e15f6f84c71d49aa8a1555a46ef88f42a3f"
working_session_id: "CLAUDE-D"
commit: ""
session_id: "CLAUDE-D"
authored_at: "2026-09-07T07:31:03Z"
authored_by: judge
tests_modified:
  - path: "admin-ui/src/components/SentimentBadge.test.tsx"
    change_type: rewrite
    lines_changed: 17
    description: "Deleted SENTIMENT_LABELS import replaced by a local EXPECTED_LABELS literal sourced from i18n/locales/en/status.json; matcher and case set unchanged."
  - path: "admin-ui/e2e/a11y.spec.ts"
    change_type: extend
    lines_changed: 129
    description: "en-US/de-DE locale matrix around the existing flow, plus an added html[lang] assertion and a new additive fa-IR RTL smoke block."
  - path: "admin-ui/e2e/autopilot-a11y.spec.ts"
    change_type: extend
    lines_changed: 136
    description: "en-US/de-DE locale loop; four existing tests re-indented with identical assertions."
  - path: "admin-ui/e2e/board-kanban-a11y.spec.ts"
    change_type: extend
    lines_changed: 66
    description: "en-US/de-DE locale loop; one existing test re-indented with identical assertions."
  - path: "admin-ui/e2e/hosting-settings-a11y.spec.ts"
    change_type: extend
    lines_changed: 98
    description: "en-US/de-DE locale loop; three existing tests re-indented with identical assertions."
  - path: "admin-ui/e2e/moderation-a11y.spec.ts"
    change_type: extend
    lines_changed: 98
    description: "en-US/de-DE locale loop; two existing tests re-indented with identical assertions."
  - path: "admin-ui/e2e/tier-settings-a11y.spec.ts"
    change_type: extend
    lines_changed: 64
    description: "en-US/de-DE locale loop wrapping the existing per-tier loop; assertions identical."
  - path: "admin-ui/src/i18n/useAdminLabels.test.tsx"
    change_type: extend
    lines_changed: 52
    description: "New file mirroring useLabels.test.tsx's four-test shape for the new useAdminLabels hook; expectations verified against i18n/locales/en/admin.json."
  - path: "widget/src/ui.test.ts"
    change_type: rewrite
    lines_changed: 6
    description: "invalid_input expected copy updated to match i18n/locales/en/widget.json after error.rs made the code server-reachable; all 14 cases and exact-equality matchers retained."
  - path: "crates/feedbackmonk-i18n/tests/plural_partition.rs"
    change_type: rewrite
    lines_changed: 2
    description: "Doc-comment count 8 -> 6 following removal of fa/tr from the pinned English-fallback ledger; assertion untouched and now stricter."
  - path: "scripts/i18n/tests/test_plural_partition.py"
    change_type: rewrite
    lines_changed: 4
    description: "Docstring count 8 -> 6 mirroring the same ledger shrink; assertion untouched."
  - path: "crates/feedbackmonk-api/tests/email_locale.rs"
    change_type: rewrite
    lines_changed: 4
    description: "Added translated_reason_note: None required by the FR-FBR-40 struct change; the value that preserves pre-existing rendering. No assertion touched."
  - path: "crates/feedbackmonk-api/tests/tenant_locale_settings.rs"
    change_type: rewrite
    lines_changed: 6
    description: "Module doc-comment refresh noting the FR-FBR-40 setting is no longer inert. No assertion touched."
code_modified:
  - path: "admin-ui/src/shared/types.gen.ts"
    change_type: api-change
    lines_changed: 123
    description: "Deleted 14 *_LABELS English-constant exports once every consumer migrated to the i18n hooks."
  - path: "admin-ui/src/i18n/useAdminLabels.ts"
    change_type: new-behavior
    lines_changed: 105
    description: "New hook resolving admin-only wire enums from admin.json admin.enum.*, with per-key fallback to the wire value."
  - path: "admin-ui/src/components/SentimentBadge.tsx"
    change_type: refactor
    lines_changed: 6
    description: "Reads its label from useLabels().sentiment() instead of the deleted SENTIMENT_LABELS constant."
  - path: "i18n/locales/en/admin.json"
    change_type: new-behavior
    lines_changed: 664
    description: "New English admin catalog holding every string extracted from admin-ui/src."
  - path: "crates/feedbackmonk-api/src/error.rs"
    change_type: api-change
    lines_changed: 139
    description: "Additive ApiError::code() + message wire fields; BadRequest(_) maps to invalid_input, making that code server-reachable by the widget for the first time."
  - path: "i18n/locales/en/widget.json"
    change_type: bugfix
    lines_changed: 2
    description: "invalid_input copy generalized because the code now covers server 400s beyond a missing subject/message."
  - path: "scripts/i18n/_catalog.py"
    change_type: bugfix
    lines_changed: 13
    description: "Removed fa and tr from OTHER_ONLY; CLDR gives each {one, other}, so their catalogs must carry a _one form."
  - path: "i18n/literal-baseline.json"
    change_type: refactor
    lines_changed: 254
    description: "Literal ratchet driven from 191 literals across 30 files to empty."
rationale_summary: "Every assertion change tracks a verified code change; net effect is stricter, with zero deletions or skips."
hypothesis_ledger_ref: ""
spec_change_ref: "FR-FBR-38 (UI localization Stage 2); DEC-FBR-17; DEC-FBR-15; Contract C35 rule 6"
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
>
> **Honesty caveat (DEC-131 tier)**: authenticity here is enforced by role and schema, not cryptographically.
> The authority chain is auditable — the record names the judge, its model, the verdict, and hashes of the
> exact diffs judged — but it is not tamper-proof. What makes a bad VOUCH recoverable is that nothing
> downstream was removed: the ARHG-01 pre-commit gate, Phase 0.5 detection, the DEC-84 subordinate deletion
> deferral, and the PODS critic all still run.

## Judge verdict

**VERDICT: VOUCH** — by `claude-opus-5[1m]` at `2026-09-07T07:31:03Z`.

## Judge's reason

Every assertion change in this batch is anchored to a verifiable code change, and the batch's net effect is
**stricter**, not laxer. `SentimentBadge.test.tsx` swapped `SENTIMENT_LABELS[sentiment]` for a local
`EXPECTED_LABELS` literal only because that export is genuinely gone — I confirmed zero remaining references
to `SENTIMENT_LABELS` in `admin-ui/src` and 14 `*_LABELS` exports deleted from `types.gen.ts`, with the
component now calling `useLabels().sentiment(sentiment)`; `i18n/locales/en/status.json` is unmodified and its
`sentiment.*` values are byte-identical to the new literals, and the matcher, the `it.each(SENTIMENT_ORDER)`
case set and `toBeInTheDocument()` are all unchanged — the new form is in fact stronger, since the old test
read the same constant the component read and would have passed on a wrong label. The six e2e specs are pure
`test.use({ locale })` parametrization: assertion-line counts are identical in five of six and *rose* 10→15 in
`a11y.spec.ts` (a new `lang` attribute assertion plus an additive `fa-IR` RTL block), test declarations only
grew, and all 21 `test.skip` calls are the pre-existing `!FAKE_API` guards re-indented, with no `.only`, no
unconditional skip, and no file deleted or renamed anywhere in the tree. The one genuine expected-value change
— `widget/src/ui.test.ts`'s `invalid_input` string — is justified upstream of any failure by `error.rs` adding
`ApiError::code()` (`BadRequest(_) => "invalid_input"`) plus a `message` field, which makes the widget's
`readError` accept the server's code for the first time and therefore makes `invalid_input` reachable from
400s that have nothing to do with a missing subject or message (verified at `feedback.rs:573`, "email is not a
valid address"), so the old subject/message-specific copy became wrong on its own merits.

Two caveats that do not change the verdict, both disclosed rather than waved through: the brief named 7 test
files but the working tree modifies **12 plus one new one**, and I judged all 13 independently rather than
accepting the framing; and `plural-fixtures.json` shrinks a pinned violator ledger 8→6, which I checked is a
*tightening* backed by a real CLDR correctness fix in `scripts/i18n/_catalog.py` (`fa`/`tr` wrongly listed as
`OTHER_ONLY`; both actually carry `{one, other}`).

Two quality nits recorded but not disqualifying: `useAdminLabels.test.tsx` uses `toBeTruthy()` for
`workOrderState("wontfix" as never)` and `keyClass("runner")`, weaker than the file's own `toBe(...)` standard
and — since `"wontfix"` is not a `workOrderState` key — asserting only that the fallback returned something
non-empty; and the `widget/src/ui.test.ts` comment cites "body too long" as an `invalid_input` case when that
path returns 413/`payload_too_large` (`feedback.rs:545/551`), overstating one example while the other cited
example holds.

## What the judge examined

- **Test diff** (`sha256: 59aa681b1580abd482fb4db04f123290be88a5468c5990ab3ef8789c2cc31861`): 12 modified
  test files (7 named in the brief, 5 discovered by the judge) plus the new untracked
  `admin-ui/src/i18n/useAdminLabels.test.tsx` (`sha256: 0218ef77a5a53af293ff933b22f37691b8dce4e3bbf37176a3e4d464dcaf4b01`),
  against base `f13a862441ac924f4f7069b81895bfc032e49297`.
- **Code diff** (`sha256: 6a8b35193f7c3c5319f4dc6454d68e15f6f84c71d49aa8a1555a46ef88f42a3f`): UI-localization
  Stage 2 admin-console string extraction into `i18n/locales/en/admin.json` via `useTranslation("admin")` /
  `useLabels()` / the new `useAdminLabels()` hook, deletion of 14 `*_LABELS` exports from `types.gen.ts`,
  the additive `ApiError::code()`/`message` wire fields, and the `_catalog.py` CLDR plural-category fix.
- **Working agent's stated rationale**: that the test edits follow the deletion of the `*_LABELS` constants
  and the addition of a `de-DE` locale run, with no assertion weakened — a claim the judge treated as the
  claim under examination and verified independently against the diffs and catalog contents rather than
  accepting, including the five test files the rationale did not disclose.

## Working agent's rationale (as submitted)

> This is a BATCHED review covering all test-file changes from one worker's task. The task's own charter
> authorized extracting every hardcoded English string in `admin-ui/src` into an i18n catalog
> (`i18n/locales/en/admin.json`) via `useTranslation("admin")` / `useLabels()` / a new `useAdminLabels()`
> hook, and deleting 14 now-redundant `*_LABELS` English-constant exports from
> `admin-ui/src/shared/types.gen.ts` once every consumer was migrated to the hooks. Also required by the
> task: add a German (`de-DE`) locale run to six existing Playwright a11y specs, and add one `fa-IR` RTL
> smoke assertion to one of them.
>
> 1. `SentimentBadge.test.tsx` previously imported `SENTIMENT_LABELS` from `types.gen.ts` and asserted
>    `screen.getByText(SENTIMENT_LABELS[sentiment])`. That constant is deleted in this task's code changes
>    (the component now reads its label from `useLabels().sentiment()`, backed by
>    `i18n/locales/en/status.json`). The test now asserts against a new local `EXPECTED_LABELS` object
>    hardcoding `{negative: "Negative", neutral: "Neutral", positive: "Positive"}`, sourced by reading the
>    actual English values in `status.json` (unmodified in this task — Stage 1 content).
> 2. The six `e2e/*-a11y.spec.ts` files: each existing `test.describe` block was wrapped in a
>    `for (const browser of ["en-US", "de-DE"])` loop with `test.use({ locale: browser })`, so every existing
>    test now runs twice with IDENTICAL assertions — no assertion weakened, removed, or made conditional on
>    locale. `a11y.spec.ts` additionally gained one wholly new `test.describe("Admin UI RTL smoke (fa-IR)")`
>    block asserting `<html dir="rtl">`, `lang="fa"`, no horizontal scroll overflow, and zero axe violations
>    on the login page.
> 3. `useAdminLabels.test.tsx` (new file) mirrors `useLabels.test.tsx`'s four-test shape (English default,
>    active-locale override, per-key fallback, unknown-value fallback) for the new `useAdminLabels()` hook.

---

**Requirement**: ARHG-06. **Decision**: DEC-190. **Judge agent**: `~/.claude/agents/test-mod-judge.md`.
**Consumer**: `~/.claude/segments/-finalize/phase0.5-test-mod-gate.md` § 0.5.16.
