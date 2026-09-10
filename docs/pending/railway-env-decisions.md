# Two Railway env-var changes, unblocked and awaiting a decision

Both were blocked by the deploy outage (DEFER-009, resolved 2026-09-10). Deploys work again and
`feedback.gitcellar.com` runs `0.4.0`, so these are decisions, not blockers. Live-deployment record:
`docs/planning/feedbackmonk-deploy-state.md` § Stage F/G.

## 1. `FEEDBACKMONK_TRANSLATION_PROVIDER`

Currently unset, so it resolves to `off` and the FR-FBR-30 translate-after-accept worker never
spawns. GitCellar's non-English feedback is clustered, searched and sentiment-scored **untranslated**
today. Needs a provider and a key: `deepl` + `FEEDBACKMONK_TRANSLATION_DEEPL_API_KEY`, or
`libretranslate` + a URL (`docs/operations/SELFHOST_ENV.md` § Translation Provider). After setting
it, `POST /api/v1/ops/translation/backfill` stamps pre-existing rows so the worker picks them up;
call it until `stamped` is 0. The UI-localization spec (FR-FBR-34..41) assumes this is running.

## 2. `FEEDBACKMONK_STORAGE_BACKEND=s3`

Currently unset, so attachments use the ephemeral local backend and would die on each redeploy.
Harmless today: the `attachments` table has 0 rows. **Not a one-variable change** — `storage.rs`
also requires `FEEDBACKMONK_S3_BUCKET`, `FEEDBACKMONK_S3_ACCESS_KEY_ID` and
`FEEDBACKMONK_S3_SECRET_ACCESS_KEY`, plus `FEEDBACKMONK_S3_ENDPOINT` for R2; setting the backend
alone fails startup. No production bucket exists, and the only object-storage credentials on this
machine are `gitcellar-r2-eu-test-*`, a **test** key — using it for production is the owner's call.
Do this before anything starts using Phase A attachments.

## Done, recorded here so it is not re-raised

Rotating `FEEDBACKMONK_SESSION_SECRET` and `FEEDBACKMONK_OPS_TOKEN` was the third item in this set.
**Done 2026-09-10** and verified live — § Stage G. Neither value is mirrored in any credential store;
both live in Railway alone.

## When you do these

Each `variableUpsert` triggers its own deploy. Set every variable first, then deploy once, or you
race deployments whose failures look exactly like the September outage.
`docs/dev-notes/railway-deploy-diagnosis.md` has the detail.
