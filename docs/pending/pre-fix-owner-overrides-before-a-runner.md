# Check pre-fix work-order overrides before any runner connects

## What to do on the trigger

**Trigger:** an autopilot runner is connected to a feedbackmonk instance, meaning a runner token
is registered and a runner process polls `/runner/...`. None is connected today. The GitCellar
Railway deploy runs no runner; `docs/operations/RAILWAY_GITCELLAR.md` names only the migration
runner. Observable: a row in `runner_tokens` that has not been revoked, or a runner service in the
deploy.

Before the runner claims its first order, list every recommendation-grounded work order that is
not finished and whose owner overrides carry text. Run a read-only query on the instance.

There is deliberately no date filter. Every state change resets `updated_at`, and the saved
overrides carry over unchanged, so a date would miss exactly the risky rows. The real cutoff is
also not a commit date: it is when the fixed console was deployed on that instance, which is
unknown. The owner reviews every row, so extra matches cost nothing.

```sql
SELECT id, tenant_id, state, updated_at, owner_overrides
FROM work_orders
WHERE recommendation_id IS NOT NULL
  AND owner_overrides ?| array['title','instructions']
  AND state NOT IN ('completed','cancelled');
-- `failed` and `reported` are included on purpose: retry (failed -> approved) and
-- request-changes (reported -> building) both send an order back to a runner with
-- its saved overrides.
```

For each row, the owner does one of two things:
- re-approves with override text they type themselves, in the console's empty fields;
- or clears the override fields.

## Why it waits

Until `f77a048`, two console dialogs pre-filled the override fields with the model-written
recommendation text: "Tweak & approve" (`RecommendationCard.tsx`) and "request changes"
(`WorkOrderDetail.tsx`). If the owner edited anything, the whole field was saved as
`owner_overrides`. The runner's prompt treats overrides as trusted: DEC-FBR-IMPL-33 and its
amendment. So an override saved that way may carry model text, possibly including an injection
from public feedback, into the trusted layer. The console fix stops new cases, but it does not
clean rows already saved. That matters only once a runner exists to execute them.

## State as of 2026-09-30

- No runner is deployed on the known live instance (`feedback.gitcellar.com`).
- I did not query the live database, so whether any such rows exist is unknown.
- Found by the critic's re-review of `f77a048`, recorded in
  `docs/planning/project-checks-review-2026-09.md`.

## Removal condition

Delete this entry, together with its stub in `CLAUDE.md`, once either of these is true:
- the query above has been run on every instance that is about to get a runner, and it returned
  no rows or every row it returned was re-approved or cleared;
- or every order the query would match is `completed` or `cancelled`. A `failed` order does not
  count as finished, because it can be retried.
