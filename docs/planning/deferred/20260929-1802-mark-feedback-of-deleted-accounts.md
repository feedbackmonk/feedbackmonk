---
status: open
commissioned_by: GitCellar, lead session af03f35f (pre-launch sweep, collab-20260929-203844, lane CLOUDFIX)
origin: GitCellar legal-review item B1, opt-in erasure of feedback at account deletion
harm: inferred
beneficiary: product
deferred_because: another-project
---

GitCellar now lets a user opt in to erasing their feedback when they delete their account. The
Cloud stores `account_deletions.erase_feedback`, and when it is true the erasure job calls
FeedbackMonk to erase that user's submissions; the job postpones and retries if FeedbackMonk fails.
The client half is `libs/feedbackmonk-client` in GitCellar: `erase_all`, authenticated with the
tenant JWT.

What the legal review also asked for is the FeedbackMonk side, which GitCellar cannot build:
- When the user **did not** opt in, the feedback of a deleted account stays. The admin UI should
  mark those submissions "account deleted", so a triager does not try to reply to a gone account and
  can tell retained feedback apart. The submitting identity should also be reduced to that marker.
- Confirm that an `erase_all` for a subject removes every attachment, screenshot and service log
  too, and say so in FeedbackMonk's own retention docs.

To try: the tenant API accepts a "subject deleted" notification (GitCellar can send it from the same
job when `erase_feedback` is false). It marks the subject's submissions and strips the identity
fields, keeping the message.
