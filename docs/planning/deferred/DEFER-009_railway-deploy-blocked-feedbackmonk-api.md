---
id: DEFER-009
title: Railway cannot create containers for feedbackmonk-api — the WontFix white-screen fix is built, migrated and staged but cannot ship
status: RESOLVED
origin: defer-local
source-project: feedbackmonk
source-session-id: interactive-20260902T010351Z-feedbackmonk-c1
injected-at: 2026-09-03T02:30:00Z
autonomy-hint: autopilot
suggested-entry-point: implement
scope-estimate: single-session
content-hash: fbm-railway-create-container-blocked-v1
---

# DEFER-009: Railway cannot create containers for `feedbackmonk-api`

> **RESOLVED 2026-09-10.** Railway started creating containers for this service again on its own:
> a plain `deploy` of the already-pinned `0.2.0` (deployment `e775c7b3-8b9c-4871-ac9b-793497b74a30`,
> 2026-09-08 20:12 UTC, no creator recorded — the dashboard or Railway's side, not this machine)
> reached SUCCESS and its container took over serving. On that evidence the finish below was run on
> 2026-09-10 16:48–16:50 UTC: `feedbackmonk-api` → `0.4.0` (deployment
> `480ad320-5e67-4fb2-87f2-a72f1e836ba6`, SUCCESS), then `feedbackmonk-admin-ui` → `0.1.3`
> (deployment `4a6807f1-c8d6-4634-9d69-9b14054834e1`, SUCCESS). Graded on curl and a real browser,
> not on Railway's status: `/api/v1/capabilities` reports `0.4.0` with 15 capabilities; the served
> admin bundle carries the "Won't Fix" label; logged in at `triage.gitcellar.com`, hard-reloaded,
> the list shows a "Won't Fix" pill on every won't-fix row, and opening `FB-3S8XGA` renders the
> drawer (status pill, status history, transition table) instead of a blank page, with zero console
> errors after sign-in. Full record: `docs/planning/feedbackmonk-deploy-state.md` § Stage F.
>
> The root cause on Railway's side was never learned: the support thread had no reply readable from
> this machine (it needs the dashboard login), and no setting on the service was changed between the
> last failure (2026-09-02 14:43 UTC) and the first success (2026-09-08). The one visible difference
> in the successful deployment's manifest is `multiRegionConfig: europe-west4-drams3a`, the same
> region as every other service in the project. The "DO NOT" block and the workaround below are kept
> as history; neither applies any more. The three deferred sub-items are now unblocked and are
> listed in `CLAUDE.md` § Pending Follow-Ups as owner decisions.
>
> Everything below this line is the pre-resolution resume brief, unchanged.


> **RESUME POINT.** Everything below was measured, not assumed. Full evidence:
> `docs/planning/feedbackmonk-deploy-state.md` § Stage E.

## The one-line state

The production white-screen fix is **written, tested, merged, and staged**; the production
database is **already migrated**; both container images are **already in the registry**.
The *only* thing missing is that **Railway cannot start a new container for this one service**,
for a reason on their side.

## What is broken for users, right now

`triage.gitcellar.com` goes blank when an admin opens any won't-fix feedback row, and won't-fix
rows show an empty status pill. **78 of the 79 production feedback rows are `wontfix`**, so the
triage inbox is ~99% unusable. Root cause was `FeedbackStatus::WontFix` serialising as `wont-fix`
while the DB and all clients use `wontfix` — fixed in `d7dca56` / `c065b60`.

## DO NOT DO THIS

**Do not change any environment variable, setting, or image pin on the `feedbackmonk-api`
Railway service until deploys are working again.**

The container currently serving `feedback.gitcellar.com` is the last one Railway successfully
started (June 2026). It is healthy. It is also **irreplaceable while this bug persists** — any
change that restarts it risks it never coming back, because starting a replacement is precisely
what is broken. That turns "one admin page is broken" into "the service is offline indefinitely".

This specifically blocks the otherwise-sensible `FEEDBACKMONK_STORAGE_BACKEND=s3` change (see
§ Deferred sub-item below). Leave it alone until a deploy succeeds.

## Evidence that this is NOT ours

Four deployment attempts across 11.5 hours, all FAILED at `Deploy › Create container` in 2–4s,
with **empty build logs, empty deploy logs**, and in-dashboard Diagnosis failing:

`c9200c3e-9764-4014-9171-ffeb23049e1c`, `a641150b-541f-4e5b-aaf5-1147154daf73`,
`2e5a6af2-50dc-4983-a052-de6bb42a9d4f`, `e12b3923-951a-4397-98c1-07831681700d`

- **The decisive one**: attempt 3 redeployed `feedbackmonk-api:0.2.0` — *the image already
  running in production* — and failed identically. Not our image, not our code, not the migration.
- Registry auth verified: manifests fetch 200 authenticated / 401 anonymous.
- **Image integrity verified**: every layer blob of `feedbackmonk-api:0.4.0` (10/10) and
  `feedbackmonk-admin-ui:0.1.3` (11/11) is present in the registry, HTTP 200. A Docker daemon
  restart at ~03:14Z did not corrupt the push.
- `0.4.0` and `0.2.0` have byte-identical `Entrypoint` / `Cmd` / `User` / `WorkingDir` /
  `ExposedPorts`, both `linux/amd64`.
- Registry credentials also passed explicitly via `ServiceInstanceUpdateInput.registryCredentials`.
- No staged changes (`environmentPatchCommitStaged` → "No patch to apply").
- Billing current: $3.12 of $20 included, card on file, Pro plan.
- **This is not the volume-contention case** from the us-west2 community thread — this service
  mounts no volume at all.
- The other 10 services in the project are online and unaffected.

## Current state — safe, consistent, leave as is

- Image pin **reverted to `feedbackmonk-api:0.2.0`**, matching the container actually running, so
  a restart cannot land on a tag Railway refuses.
- `feedback.gitcellar.com/health/ready` 200; `/api/v1/capabilities` reports `0.2.0`;
  `triage.gitcellar.com` 200.
- **DB schema is deliberately ahead of the running code**: migrations `00019`–`00030` are applied
  (max=30, 0 failures) while the service runs 0.2.0. This is supported and was the point of
  migrating first — every pending migration is backward-compatible with 0.2.0. Verified serving
  for 24h+.
- Pre-migration backup: `S:\_fbm-deploy-backups\feedbackmonk-20260902T024909Z.sql.gz`
  (gzip-verified, 23 tables). **Keep until the deploy lands.** Note GitCellar's nightly
  `gitcellar-pg-backup` cron targets the `railway` DB, NOT `feedbackmonk`.

## Open support thread

Private Railway thread ("Technical Help"), filed 2026-09-02 ~14:50 UTC:
**"Deploys fail instantly at CREATE_CONTAINER, no logs, no volume — old container still serving"**
Status **Awaiting Railway Response**, **0 replies as of 2026-09-03 02:15 UTC (11h)**.
It carries all four deployment IDs and the retry evidence, and explicitly asks that it not be
converted to a community bounty.

> **Where this thread actually lives** (established 2026-09-10, after two sessions called it "the
> dashboard login" and got that wrong). Railway has **no in-dashboard ticket system**. Private
> Technical Help threads are on **Central Station, `https://station.railway.com/`**, signed in as the
> account that filed it (`admin@gitcellar.com`, password in the owner's password manager). The
> project dashboard `https://railway.com/project/fab620f1-2392-4a3c-9eb6-5cc7a60bc06e` is a different
> place and does not carry it. `railway.com/help` 301-redirects to Central Station's **public**
> community area, which is explicitly *not* where a private thread appears.
>
> **The thread's own URL was never recorded**, so it can only be found by signing in and locating it
> among your own threads; public threads are `station.railway.com/questions/<slug>-<hash>` but a
> private slug cannot be derived. **Whoever files the next support thread: paste its URL here.**
>
> Also useful context for why it sat unanswered: Pro support targets ~72 hours within business hours
> (Mon-Fri, 9am-9pm Pacific). It was filed Wednesday ~14:50 UTC and judged unanswered ~11 hours later.


## On resume — do this

**1. Check whether deploys work again.** Re-run one deploy. If it succeeds, finish immediately
(step 2). If it fails the same way, check the support thread for a reply.

**2. If deploys work — finishing takes ~1 minute. No rebuild, no migration, no code change.**
Both images are already in the registry. Repoint + deploy, header **`Project-Access-Token`**
(NOT `Authorization: Bearer` — that returns `"Not Authorized"` at HTTP 200), env
`15941208-6a62-4f8a-ad18-013d67c76df5`:
- `feedbackmonk-api` `50e4291d-b411-4388-a6e5-1f9d47ec8623` → `registry.gitcellar.com/feedbackmonk-api:0.4.0`
- `feedbackmonk-admin-ui` `48918bae-fd9f-459a-b280-f2cc9290e640` → `registry.gitcellar.com/feedbackmonk-admin-ui:0.1.3`

Pass `registryCredentials` (`gitcellar-push` + WCM `gitcellar-registry-push`) in the same
`serviceInstanceUpdate` call. Then `serviceInstanceDeployV2`. **Grade on curl, never on Railway's
status** — neither service defines a `healthcheckPath`, so Railway reports SUCCESS the moment a
container starts.

Verify: `/api/v1/capabilities` → `0.4.0` with 15 capabilities; then **in a real browser** (curl
cannot see an SPA crash) log into `triage.gitcellar.com`, hard-reload, open a won't-fix row, and
confirm a "Won't Fix" pill renders instead of a blank page.

**3. If Railway is still stuck and waiting is no longer acceptable — the replacement-service
workaround.** In the community thread "Every deploy of one service fails instantly at
CREATE_CONTAINER", the reporter observed that a *brand-new* service in the same project deployed
fine; the corruption is pinned to the one service. So: create a NEW Railway service pointed at
`feedbackmonk-api:0.4.0`, copy all 14 environment variables across, verify it healthy on its
Railway-generated URL, then move the `feedback.gitcellar.com` custom domain over and delete the
old service.

Trade-offs, stated honestly: it means copying secrets by hand; there is a short downtime window
while the domain moves; and if the new service also refuses to start, the old one is still
untouched, so it is recoverable. **Get the owner's approval before moving the domain.**

## Deferred sub-items (do NOT action before a deploy succeeds)

- **`FEEDBACKMONK_TRANSLATION_PROVIDER`** — currently unset, so it resolves to `off` and the
  FR-FBR-30 translate-to-English worker never spawns. Measured 2026-09-06 (D-FBR-32): the running
  `0.2.0` predates FR-FBR-30 anyway, so GitCellar's non-English feedback is clustered, searched and
  sentiment-scored **untranslated** today. Once a deploy path works, set the provider (`deepl` +
  `FEEDBACKMONK_TRANSLATION_DEEPL_API_KEY`, or `libretranslate` + URL — `SELFHOST_ENV.md` L110-113)
  in the same change as the redeploy, then `POST /api/v1/ops/translation/backfill` for the
  pre-existing rows. The UI-localization spec (FR-FBR-34..41) assumes this pipeline is running.

- **`FEEDBACKMONK_STORAGE_BACKEND=s3`** — currently unset, so attachments would use the ephemeral
  local backend and die on each redeploy. Harmless today: the `attachments` table has **0 rows**.
  It is **not a one-variable change**: `storage.rs` requires `FEEDBACKMONK_S3_BUCKET`,
  `FEEDBACKMONK_S3_ACCESS_KEY_ID` and `FEEDBACKMONK_S3_SECRET_ACCESS_KEY` (plus
  `FEEDBACKMONK_S3_ENDPOINT` for R2). Setting the backend alone would fail startup. No production
  bucket exists; the only object-storage credentials on the machine are `gitcellar-r2-eu-test-*`
  (a **test** key — using it for production is the owner's call). Do this only once a working
  deploy path exists to roll back with, and before anything starts using Phase A attachments.
- **Rotate `FEEDBACKMONK_SESSION_SECRET` and `FEEDBACKMONK_OPS_TOKEN`** — both were visible in a
  screenshot shared during debugging. Requires a working deploy to take effect.
