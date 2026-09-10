---
status: resolved
source: GitCellar, session 49c7c32d-0a3e-403b-acfc-627d23e145d7 (2026-09-10)
origin: a GitCellar session auditing whether recent FeedbackMonk work had broken its integration
harm: witnessed
beneficiary: product
---

# The GitCellar adoption contract still documents `wont-fix`, and the change that broke it went unrecorded

> **RESOLVED 2026-09-10.** §6.1 corrected to `wontfix` (the only hyphenated occurrence in the file); a 2026-09-01 change-log entry added, marked as a wire-form change to an existing field and naming the send/read asymmetry of the alias; the `status.rs` alias comment now states the two-part delete condition (GitCellar flips its smoke-script literal, no other adopter documented as sending the hyphen) instead of an open invitation. The redeploy that unblocks GitCellar's flip landed the same day (DEFER-009).

## What is wrong

`docs/integrations/gitcellar-adoption.md:383` documents the §6.1 status union as:

```
"status": "submitted" | "triaged" | "in-progress" | "shipped" | "wont-fix" | "duplicate",
```

Since `d7dca56` (2026-09-01, *"serialize FeedbackStatus::WontFix as `wontfix`, not `wont-fix`"*) the
API has emitted `wontfix`. `MeFeedbackItem`, `MeThreadResponse` and `ExportFeedbackItem` all carry
`status: FeedbackStatus`, so serde's rename governs every consumer-facing projection. The contract
line has been wrong for nine days.

The `## Change log` in that same file has entries for 2026-09-06 and 2026-09-07 (both correctly
marked ADDITIVE) but **no entry for 2026-09-01** — the one change in that window that was *not*
additive for a consumer.

## What was witnessed

GitCellar's `libs/feedbackmonk-client/src/mapping.rs` had pinned the documented spelling in three
functions and in a table test asserting `status_to_ui_status("wont-fix") == "wontfix"`. Against a
FeedbackMonk serving the current code, `wontfix` matched no arm and fell through every `_` fallback:
bucketed `open`, reported non-terminal, badged `submitted`. A closed feedback item would have
rendered in the Desktop UI as an active one — no error, no log line, nothing to notice.

It has not bitten a user, because `feedback.gitcellar.com` still serves `"version":"0.2.0"`
(verified 2026-09-10) and the fix is not deployed. GitCellar has since made its client accept both
spellings, so the incident is closed on that side. The doc is still wrong for the next consumer.

## What to do

1. Correct line 383 to `wontfix`, and grep the file for any other hyphenated occurrence.
2. Add a `## Change log` entry dated 2026-09-01 that says plainly this was a **wire-form change to
   an existing field**, not an additive one, and names the input alias (`#[serde(alias =
   "wont-fix")]`) that keeps old *writers* working — the alias protects callers who **send** the
   status, and does nothing for callers who **read** it. That asymmetry is the whole trap and is
   worth one sentence.
3. Consider whether `status.rs`'s "Safe to delete once no client is known to send `wont-fix`" note
   should also record that the doc, not just clients, was a consumer of the old spelling.

## Do not drop the input alias yet — GitCellar is the client that comment is about

`status.rs` says the `#[serde(alias = "wont-fix")]` is "Safe to delete once no client is known to
send `wont-fix`." **A client is known: this one.**
`scripts/prod-smoke/prod-feedback-smoke.ps1:492` (GitCellar) POSTs
`{ to_status = 'wont-fix' }` to the admin transition endpoint as its cleanup step, and
`tests/e2e/test-e2e-solicitation.ps1` instructs an operator to triage its synthetic rows to won't-fix
by hand. The smoke script even carries a comment pinning the hyphen as "the WIRE status value".

GitCellar cannot simply switch that literal to `wontfix` today: the live
`feedback.gitcellar.com` is still `0.2.0`, which pre-dates `d7dca56` and therefore accepts **only**
the hyphenated form. Flipping it now breaks the smoke test until the redeploy; flipping it after the
redeploy is a one-word change. So the ordering is: redeploy, then GitCellar switches the literal,
then the alias is genuinely unused. Deleting the alias before that middle step 422s a live GitCellar
cleanup path.

Worth adding that ordering to the alias comment, so the next reader of "safe to delete" has the
condition rather than the invitation.

## Ruled out

Not a GitCellar bug. Not a deployment question — the correction is right regardless of when the
Railway redeploy (DEFER-009) or the SaaS cutover lands, and doing it now is what stops the *next*
adopter reading a stale union and pinning it exactly as GitCellar did.
