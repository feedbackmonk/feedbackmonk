# feedbackmonk — Deploy State / RESUME POINT

**Purpose**: single consolidated, durable record of the live deployment state so any fresh session can
re-orient in seconds. Reconstructed 2026-06-03 from **live verification** (curl probes against the
running instance) + the committed integration contract, after a prior session's deploy-state notes
were lost in a reboot (never persisted). Every "VERIFIED" fact below was confirmed live on 2026-06-03.

> **Why this file exists**: a 2026-06-03 session reported it had committed a deploy-state note +
> updated `PROJECT_TRAJECTORY.md` / `PENDING_FOLLOW_UPS.md`. None of it survived the reboot (working
> tree was clean, no stash, reflog ended at `28cb2d8`). This note re-establishes ground truth from
> durable + live sources so the loss can't repeat. The bulk of the deploy facts already survive
> independently in `docs/integrations/gitcellar-adoption.md` (committed) — this note consolidates them
> and adds live-verification evidence.

---

## TL;DR — current state (2026-06-03)

**The feedbackmonk-api for GitCellar (customer #1) is DEPLOYED, LIVE, HEALTHY, and CORS-correct.**
Nothing on the feedbackmonk backend is "reverted." No feedbackmonk feature or deploy work is pending.
Remaining items are GitCellar-side (their public-site embed) and SaaS (deferred), both out of scope
for the feedbackmonk backend.

> **2026-06-09 update — widget theming + footer/tier decoupling DEPLOYED ✅.** The three changes
> from the GitCellar dogfood intake (DEC-FBR-IMPL-11/12/13: per-tenant admin-ops-only footer override
> decoupled from tier + ops endpoint `PATCH /api/v1/ops/tenants/{id}` guarded by
> `FEEDBACKMONK_OPS_TOKEN`; widget dark/light/auto theme + per-tenant color/logo; launcher-less
> `[data-feedback-open]` / `window.feedbackmonk.open()` trigger) are now **LIVE on prod**:
> - **`feedbackmonk-api:0.1.3`** built from `697a7db` (offline sqlx, digest
>   `sha256:98b3336f86147421e6f5d9d87ca68f6e7e61089c5e89cf333a1f6cdbd751d7c6`) →
>   `registry.gitcellar.com` → Railway `serviceInstanceUpdate(source.image)` + `serviceInstanceDeployV2`
>   (deployment `a174369f-…` SUCCESS). **Migration 00012 applied** to the prod `feedbackmonk` DB
>   beforehand (additive/all-NULL; the api image does not auto-migrate).
> - **GitCellar tenant ops-flipped**: `tier=self_host`, `footer_text_override=""` (badge suppressed,
>   restore at launch), `theme=dark`, `primary_color=#8b5cf6`. ⚠️ The ops path param is the
>   **tenant_id** (`020c637c-…`), not the project_id. Ops token: **Railway only** — see Stage G; there is no `gitcellar-feedbackmonk-ops-token` entry in the credential store and there never was one on this machine.
> - **GitCellar-side**: widget re-synced + Forge embed flipped to launcher-less + dark (GitCellar
>   commit `bfa5562e23`); verified live on Cloud Forge :3222 — no launcher, navbar button opens dark modal.
>
> Authoritative operator log of this deploy: GitCellar repo
> `docs/planning/feedbackmonk-deploy-state.md` § Stage C. Original checklist:
> `docs/planning/followups/20260609-gitcellar-widget-theming-and-footer-decoupling-resync.md`.

---

## Stage K (2026-09-11) - the alert path is PROVEN to reach a human, end to end

Stage J wired the heartbeat and proved the *ping*. This proves the half that actually matters: that a
missed backup check reaches an inbox. It was tested by deliberately breaking it, not by reasoning.

The heartbeat's expected interval was briefly shortened so it registered a miss, then restored.

| Event | UTC |
|---|---|
| heartbeat registered the miss (incident `1013735896`) | 01:05:49 |
| **alert email delivered to `admin@gitcellar.com`** | **01:05:53** |
| verifier ran, pinged, incident auto-resolved | 01:07:18 |
| resolution email delivered | 01:07:22 |

Four seconds from failure to mail, both from `alerts@alerts.betterstack.com`. Confirmed by the owner
and **independently verified** by a read-only search of the `admin@gitcellar.com` mailbox. A
resolution notice is sent too, so a self-clearing alert says so rather than leaving an open question.
Afterwards: heartbeat `up`, incident Resolved, `cronSchedule` back to `30 6 * * *`, period/grace back
to 86400/3600.

**Historical corroboration:** the sibling heartbeat `gitcellar-backup-verify` has raised real
incidents before (`992388841` on 2026-07-20, open two days; `985778580` on 2026-07-02, resolved in a
minute), so this alerting path has production history and is not merely configured.

### Text alerting now matches the sibling

`feedbackmonk-backup-verify` was created with defaults, which gave email only, while
`gitcellar-backup-verify` has `sms=true`. Both are now `email=true, sms=true, call=false, push=false`.
**What that does today: nothing** - the Better Stack record states the free tier delivers email and
Slack only, and phone/SMS paging is paid. It is set because the flag is free, and because leaving the
two heartbeats divergent means that on any future plan upgrade GitCellar's backup would start paging
and this one silently would not. Email is the channel that actually works now.

### Two traps worth writing down

- **The on-call schedule lists no users.** Neither heartbeat has an escalation policy, so both fall to
  the default "Primary on-call schedule", whose `on_call_users` comes back empty from the API. It
  clearly does not prevent delivery - the test email arrived in four seconds - but do not read that
  empty list as "configured and healthy".
- **Zoho's search API needs a field qualifier.** A bare keyword (`betterstack`) returns *no matches*
  for mail that is definitely there; `entire:betterstack` finds it. An empty result from a bare
  keyword reads exactly like "the mail never arrived" and briefly did here. Anyone verifying mail
  delivery through `~/.claude/agent-tools/email-triage/lib/zoho.ps1` should qualify the search key.

> The full `1-email-triage` skill was **not** used for this. It is a whole-inbox agent that stages
> deletions and drafts replies; running it to confirm one message would be wildly disproportionate,
> and it belongs to the GitCellar project so it is not loadable from here anyway. One read-only
> `Search` call against its underlying library was the right-sized tool.

---

## Stage J (2026-09-11) - backup alerting is CLOSED; absence is now detected

The last gap from Stage I. The owner supplied a write-capable Better Stack token; everything below
was done with it and verified, not assumed.

**Heartbeat `feedbackmonk-backup-verify`, id `492293`** - period 86400 s (one ping a day), grace
3600 s, so silence alerts about an hour after a missed run. Those numbers deliberately match the
sibling `gitcellar-backup-verify` heartbeat (id `468794`), **measured from the API rather than taken
from the inventory** - see the correction below.

Its ping URL is set as `FBM_VERIFY_HEARTBEAT_URL` on the `feedbackmonk-backup-verify` service
(`717daf20-...`). Railway is its only home, exactly as GitCellar's equivalent is stored; it is a
secret-ish token and **must never enter this public tree**.

### Proven end to end

| Step | Evidence |
|---|---|
| before | heartbeat status `pending` - never pinged |
| one-shot run of the verify job | `FBM_VERIFY_PASS` then `heartbeat pinged` |
| after | heartbeat status **`up`** |
| schedule restored | `cronSchedule` back to `30 6 * * *` |

A `variableUpsert` alone did **not** run the job - with a cron set, the auto-triggered deploy only
reschedules. Proving the ping therefore needed the clear-cron / deploy / restore-cron dance, which is
the same recipe Stage H records for changing these jobs.

### The API token is now stored, and that closes a documented gap

`gitcellar-betterstack-uptime` (user `uptime-api`) now exists in Windows Credential Manager, read
back through the estate's own helper and **exercised against the API**, not merely round-tripped.
This was one of the nine entries the credential inventory claimed but did not have
(`docs/planning/deferred/credential-inventory-claims-absent-entries-20260910.md` in the GitCellar
tree). Its absence is precisely why this task stalled for a turn.

### Correction to the inventory, measured

The `backup-verify-heartbeat-url` row states GitCellar's heartbeat is "Period 86400s + grace 86400s,
so silence alerts by email after ~48h". **The live values are period 86400, grace 3600** - so it
alerts after roughly 25 hours, not 48. The real behaviour is better than documented, but the number
in the record is wrong and someone sizing an incident response would be misled by it.

### What is now covered, and what is not

Covered: the dump runs nightly and read-back-verifies its own upload; the verifier independently
re-checks the newest dump each morning and fails loudly naming the responsible service; **and if
either stops firing at all, the heartbeat goes silent and Better Stack raises an alert.** That was
the last hole.

Not covered, and still true: Tier 1 proves a well-formed encrypted artifact, **not restorability**.
Only a real decrypt-and-restore proves that, and the private key is GitCellar's. Their
`ci/pg-backup/verify-restore.sh` Tier 2 is the shape that would do it.

---

## Stage I (2026-09-10) - the feedbackmonk backup is now verified daily by a verifier of our own

On the owner's word, after they asked whether extending GitCellar's verifier was the right move.
**It was not**, and the recommendation was reversed before building - see the decision below.

**New Railway cron `feedbackmonk-backup-verify`** (`717daf20-603a-48b5-8977-99b8fc44d5b1`), image
`postgres:16`, schedule **`30 6 * * *`** UTC, restart policy NEVER. Body:
`deploy/backup/feedbackmonk-backup-verify.sh` in this repo, base64-wrapped into the start command.
Two hours after our backup, thirty minutes after GitCellar's verifier. The four backup services now
read: `gitcellar-pg-backup` 04:00, `feedbackmonk-pg-backup` 04:30, `gitcellar-backup-verify` 06:00,
`feedbackmonk-backup-verify` 06:30.

### Why NOT extend `gitcellar-backup-verify`, which is what was originally proposed

1. **It pings a single Better Stack heartbeat.** Folding our checks in means a feedbackmonk failure
   silences GitCellar's backup alarm and reads as *their* backup breaking - one product's noise
   degrading another product's signal. Keeping our heartbeat separate keeps both alarms meaningful.
2. **Its script is committed in the GitCellar repo** under an explicit "KEEP IN SYNC with the Railway
   service start command" warning. Changing the live job means either editing that tree, which
   DEC-FBR-07 forbids from here, or knowingly leaving drift in a critical backup verifier.

Railway's `notificationRule*` mutations were checked as a third option and are **not reachable with a
project-scoped token** (the input type introspects empty and the query 400s), so platform-native
deploy-failure alerting is not available to us.

### Exercised before deployment - all three paths, not just the happy one

A verifier that has only ever passed is worth little, and this project already has history here:
GitCellar's verifier was blind for two weeks in 2026 while reporting failures on healthy backups.

| Scenario | Result |
|---|---|
| real prefix, real dump | `FBM_VERIFY_PASS`, `age=0h size=27659`, `openpgp OK (keyid FF02A40CF17791EF)`, exit 0 |
| prefix with no dumps | `FAIL: no dumps at all ... check THAT service's logs, not this one's credentials`, exit 1 |
| bucket it cannot read | `FAIL: listing ... errored -- this verifier cannot see the bucket`, exit 1 |

The two failure messages were deliberately split, because an empty listing and a listing error point
at **different services** - the backup job versus this verifier's own credentials. The first draft
said "likely bad/stale R2 creds" for both, which is exactly the class of misleading error that cost
this project a week in September 2026, so it was fixed before deployment.

### The gap that is left, and the one action that closes it

`FBM_VERIFY_HEARTBEAT_URL` is **unset**, so the verifier prints a NOTE and pings nothing. Artifact
checking is covered; **absence is not**. A cron that stops firing produces no logs to fail in. The
ping code is written and gated on that variable, so closing this is: create a Better Stack heartbeat,
set the URL on the verify service, let its deploy stand. Body:
`docs/pending/feedbackmonk-backup-alerting.md`.

---

## Stage H (2026-09-10) - the `feedbackmonk` database now has a nightly backup of its own

Closes the gap Stage G surfaced. On the owner's word ("add feedbackmonk to the nightly backup").

**New Railway cron service `feedbackmonk-pg-backup`** (`27880cb2-8bce-4aae-a28b-43442483c896`),
image `postgres:16`, schedule **`30 4 * * *`** (UTC), restart policy NEVER. Its body is
`deploy/backup/feedbackmonk-pg-backup.sh` in this repo, base64-wrapped into the service start
command - the same deployment shape GitCellar uses for its own backup cron. **The repo copy is the
source of truth; changing it does not change the deployment.** Re-deploy by setting
`startCommand` to `bash -c "echo <base64 of the script> | base64 -d | bash"` via
`serviceInstanceUpdate`, then `serviceInstanceDeployV2`. To prove a change before trusting the
schedule, clear `cronSchedule` in that same update so the deploy runs immediately as a one-shot,
read the logs, then set the cron back.

### Why a separate service rather than a second dump in GitCellar's job

`gitcellar-backup-verify` verifies **the newest object under the `pg/` prefix**. Adding feedbackmonk
dumps to that prefix would have pointed GitCellar's verification at the wrong file on alternating
days - quietly weakening a guarantee that already worked. This job writes under **`fbm/`** instead,
so `pg/` and its verifier are untouched, and neither product's backup can break the other's. Both
GitCellar services were re-checked afterwards and are unchanged (`gitcellar-pg-backup` cron
`0 4 * * *`, `gitcellar-backup-verify` cron `0 6 * * *`, last deploys still from July).

### Verified, not assumed

| Evidence | Result |
|---|---|
| Job log | `FBM_BACKUP_COMPLETE fbm/feedbackmonk-20260910-184631.sql.gz.gpg (27659B, R2-EU, read-back verified)` |
| Object listed in R2 from this machine | present, 27,657 B and 27,659 B for the two proving runs |
| OpenPGP structure, checked locally | valid - `pubkey enc packet: version 3, algo 18` |
| Recipient key vs. GitCellar's dump | **both `FF02A40CF17791EF`** - one private key restores both |
| GitCellar's `pg/` prefix | untouched, daily cadence intact through 2026-09-10 |

Railway reported the run SUCCESS **before the dump had even been written** - this service defines no
`healthcheckPath` either - so every claim above is graded on logs and on the artifact, never on
Railway's status.

### A bug in the first version, worth keeping written down

The first proving run uploaded correctly but printed no completion line. Cause: the script used
`set -euo pipefail`, and `gpg --list-packets` **exits non-zero on a file you hold no secret key
for** - which is every file this job writes. So the read-back check killed the script silently: the
upload succeeded, nothing was verified, and a genuinely corrupt upload would have died just as
quietly. GitCellar's `verify-cron-inline.sh` omits `-e` for exactly this reason. Fixed by checking
every exit code explicitly and grading the gpg step on its packet listing rather than its status.
**The missing log line was the only symptom** - the deployment still said SUCCESS.

### Still open - no dead-man switch on the new prefix

Nothing alerts if this cron silently stops: `gitcellar-backup-verify` reads `pg/` only, and a cron
that never fires produces no logs to fail loudly in. That is the same failure class that sat
undetected for two weeks on the GitCellar side in 2026. Filed to GitCellar as
`docs/planning/deferred/feedbackmonk-backup-prefix-and-verification-20260910.md`; it needs either an
extension of their verify cron or a heartbeat of this job's own, and both are cross-product calls.

**Also inherited:** this job reuses GitCellar's `r2-backup-writer` credentials and bucket, so a
rotation of that token breaks this backup too, visibly only in these logs. And no retention or
lifecycle policy applies to `fbm/` - the objects accumulate until someone sets one.

---

## Stage G (2026-09-10) — secrets rotated; pre-migration backup pruned; one Railway gotcha learned

Follows Stage F the same day, on the owner's word ("rotate the session secret and ops token").

### Both secrets rotated and verified

`FEEDBACKMONK_SESSION_SECRET` and `FEEDBACKMONK_OPS_TOKEN` were both exposed in a screenshot shared
during the 2026-09-02 debugging. Both are now fresh 64-hex values generated with a CSPRNG, set via
`variableUpsert` on the `feedbackmonk-api` service and confirmed by read-back. The live proof, taken
against production after the container restarted:

| Probe | Result |
|---|---|
| `PATCH /api/v1/ops/tenants/<tenant>` with the **old** ops token | **401** — old token is dead |
| the same with the **new** ops token | **200** — new token authenticates |
| the same with **no** token | 401 — the ops surface is enabled, not silently disabled |
| fresh browser login at `triage.gitcellar.com`, then a reload | session survives — the new session secret signs and verifies cookies |

The ops probe used an empty `{}` body deliberately: `patch_tenant` validates, resolves the scope,
and only writes under `if let Some(...)` for tier and branding, so `{}` reads and writes nothing.
The response confirmed the tenant is untouched — `tier: self_host`, `footer_text_override: ""`,
`theme: dark`, `primary_color: #8b5cf6`, exactly as Stage C set it.

**Correction to an earlier claim in this file and in `CLAUDE.md`: the ops token was NOT mirrored in
the credential store.** Enumerating Windows Credential Manager shows only
`gitcellar-feedbackmonk-jwt-private` (the Ed25519 signing key) and
`gitcellar-feedbackmonk-ops-password` (the 35-character `triage@gitcellar.com` **login password**,
which is a different secret and was not rotated). No entry holds either 64-hex value. So this was a
one-place rotation with no GitCellar coordination, not the two-repo change previously recorded. Both
values remain readable back from Railway's `variables` query, which is their only home.

### The Railway gotcha this cost — `variableUpsert` auto-deploys

**Each `variableUpsert` call triggers its own deployment.** Two upserts followed by an explicit
`serviceInstanceDeployV2` produced **three** deployments within one second
(`42348781-…` 18:18:03 SUCCESS, `6ce1e4cc-…` and `43936e52-…` 18:18:04 both FAILED). The two
failures are Railway superseding overlapping deploys — **not** the Stage E create-container bug,
which they superficially resemble because their build and deployment logs are also empty. The
distinguishing evidence is that a sibling deployment created in the same second reached SUCCESS and
its container served correctly.

A follow-up `serviceInstanceDeployV2` with no variable change (`41d3bd42-…`, 18:20:24) returned the
service instance's `latestDeployment` to SUCCESS, so the next reader does not meet a red status that
means nothing. **When changing several variables, upsert them all and then deploy once — or expect
this race.** `docs/operations/RAILWAY_GITCELLAR.md` § 8 carries the same warning.

### Pre-migration backup pruned

`S:\_fbm-deploy-backups\` is **deleted**. Its own `_WHY.txt` named two delete conditions and both
were re-measured today rather than assumed: `0.4.0` is deployed and serving, and `_verify.sql` was
re-run against the production database through the TCP proxy:

| Check | Value |
|---|---|
| rows with body text | 44 |
| rows empty or null body | 35 |
| text-bearing rows with a NULL `body_tsv` | **0** |
| `_sqlx_migrations` max / failed | 30 / 0 |
| feedback rows | 79 |

Identical to the 2026-09-02 figures, so migration `00019`'s `body_tsv` rebuild is still sound.

> **Gap this exposes, unchanged by the deletion and worth an owner decision:** the `feedbackmonk`
> database has **no logical backup of its own**. GitCellar's nightly `gitcellar-pg-backup` cron
> dumps the `railway` database, not this one, so the only coverage is Railway's project-level
> Postgres PITR. The deleted dump was a one-off pre-migration snapshot, never a backup regime.

---

## Stage F (2026-09-10) — **DEPLOYED**: `feedbackmonk-api:0.4.0` + `feedbackmonk-admin-ui:0.1.3` LIVE; DEFER-009 resolved

> **Root cause, learned 2026-09-10 (after this stage was written):** a stale registry credential on
> the service, not a Railway fault. See DEFER-009 § ROOT CAUSE. The paragraph below correctly records
> that this machine did not cause the recovery; its implication that the cause was unknowable is
> superseded.

**What unblocked it**: nothing this machine did. Read-only inspection on 2026-09-10 found a
`feedbackmonk-api` deployment `e775c7b3-8b9c-4871-ac9b-793497b74a30` at **2026-09-08 20:12 UTC**,
status SUCCESS, `reason: deploy`, image `0.2.0` (the pinned tag), `creator: null`, and
`/health/ready` reported `started_at` 20:12:52 the same day — i.e. Railway had created a container
for this service again, six days after the last failure, from a plain redeploy of the existing pin.
`gitcellar-cloud-api` had also deployed successfully on 2026-09-03. The support thread could not be
read from here (dashboard login). No service setting was changed between the last failure and that
success; the successful manifest shows `multiRegionConfig: europe-west4-drams3a`, the project's region.

**What was done** (GraphQL, header `Project-Access-Token`, env `15941208-…`, both
`serviceInstanceUpdate` calls carrying `registryCredentials` `gitcellar-push` + WCM
`gitcellar-registry-push`, then `serviceInstanceDeployV2`):

| Step | UTC | Deployment | Railway | Graded on |
|---|---|---|---|---|
| `feedbackmonk-api` `50e4291d-…` → `registry.gitcellar.com/feedbackmonk-api:0.4.0` | 16:47:56 | `480ad320-5e67-4fb2-87f2-a72f1e836ba6` | SUCCESS in 12 s | `GET /api/v1/capabilities` → `"version":"0.4.0"`, **15** capabilities (`feedback.sentiment`, `body-optional`, `sentiment-trend`, `solicitation.v1`, `my-feedback`, `delete`, `reply_state`, `export`, `severity`, `rating`, `idempotency`, `attachments`, `erase_all`, `hosting.subdomains`, `hosting.custom_domain`). Note `0.4.0`'s `/health/ready` no longer carries `version` — read it from capabilities. |
| `feedbackmonk-admin-ui` `48918bae-…` → `registry.gitcellar.com/feedbackmonk-admin-ui:0.1.3` | 16:49:57 | `4a6807f1-c8d6-4634-9d69-9b14054834e1` | SUCCESS in 12 s | `triage.gitcellar.com` 200; served bundle `assets/index-5s3eMduH.js` contains the `Won't Fix` label (12× `wontfix`, 0× `wont-fix`). |

**Browser verification (Playwright, real Chromium)**: signed in as `triage@gitcellar.com`,
hard-reloaded `/feedback`: 79 rows, every won't-fix row shows a "✕ Won't Fix" pill. Opened
`FB-3S8XGA` → `/feedback/FB-3S8XGA` renders the drawer: status pill "Won't Fix", kind, sentiment,
body, status history ("Submitted → Won't Fix by triage@gitcellar.com"), replies panel, and the
transition table ("From Won't Fix to: Submitted"). Console: zero errors after sign-in (the only
entries are pre-login: `favicon.ico` 404, the expected `/api/v1/public/site` 404 on a non-tenant-bound
host, and the list's 401 before authentication).

**What this closes at once**: Phase-A A6 (the six Phase-A capabilities are advertised — GitCellar
Phases B/C are unblocked), DEFER-004 (`feedback.rating` live), FR-FBR-32/33 (`hosting.*` live), and
the `wontfix` serde fix (the triage inbox is usable again). The DB was already at `00030`; migration
`00031` (submitter `locale`, 2026-09-06) is **not** applied and `0.4.0` predates it, so nothing is
pending against the running code. The pre-migration backup
`S:\_fbm-deploy-backups\feedbackmonk-20260902T024909Z.sql.gz` has done its job and may be pruned
on the owner's normal schedule.

**Now unblocked, not done — each is an owner decision, listed in `CLAUDE.md` § Pending Follow-Ups**:
`FEEDBACKMONK_TRANSLATION_PROVIDER` (which provider and key), `FEEDBACKMONK_STORAGE_BACKEND=s3`
(needs a production bucket + four variables; the only credentials on the machine are a **test** R2
key), and rotating `FEEDBACKMONK_SESSION_SECRET` / `FEEDBACKMONK_OPS_TOKEN` (the ops token is
mirrored in GitCellar's credential store, so it is a two-repo change).

---

## Stage E (2026-09-01/02) — DB MIGRATED to 00030; images built + pushed; **Railway deploys are BLOCKED**

**Trigger**: the owner reported `triage.gitcellar.com` white-screening on any won't-fix feedback row.
Root cause was a feedbackmonk bug (`FeedbackStatus::WontFix` serialised `wont-fix`, DB/clients use
`wontfix`), fixed at `d7dca56` + `c065b60`. **78 of the 79 production feedback rows are `wontfix`**
(verified by direct `SELECT`), so the triage inbox was ~99% unusable.

### DONE and verified

| Item | State |
|---|---|
| Pre-migration backup | `S:\_fbm-deploy-backups\feedbackmonk-20260902T024909Z.sql.gz` — gzip-verified, 23 tables. Taken **by hand**: `gitcellar-pg-backup` targets the `railway` DB, NOT `feedbackmonk`. |
| **Migrations 00019 → 00030** | **APPLIED** to the prod `feedbackmonk` DB. `_sqlx_migrations` max=30, 0 failures. Data intact: 79 feedback rows (78 wontfix / 1 submitted), 1 tenant. `00019`'s `body_tsv` rebuild verified correct — 44 rows have text, 35 are sentiment-only (body NULL/empty), and **0** text-bearing rows have a NULL `body_tsv`. |
| `feedbackmonk-api:0.4.0` | Built from a clean worktree at `c065b60`, pushed. Digest `sha256:e90569031aa623722ec12dfd78c18e6a91207f016004449ad12eff1e077351e2`. linux/amd64. Binary contains `wontfix`, the `wont-fix` input alias, `feedback.rating`, `hosting.subdomains`, `hosting.custom_domain`, `feedback.export`, `feedback.idempotency`. |
| `feedbackmonk-admin-ui:0.1.3` | Built + pushed. Digest `sha256:f49add4ac38f05f87a2548507d9233584134e0e087edf3fd2e895173675c3d02`. Verified it baked the **Railway** nginx conf (`proxy_pass https://$fbm_api`), not the compose one (`api:14304`). |

### BLOCKED — Railway will not deploy this service

> **CORRECTED 2026-09-10 — the conclusion in this section is WRONG.** The cause was a **stale
> registry credential saved on the `feedbackmonk-api` service**: every deployment failed at the
> IMAGE PULL step with a 401 from our own registry, not at scheduling. What made it unreadable was a
> Railway bug that dropped the real error before it reached us, leaving the bare "Failed to create
> deployment." with empty logs. Full evidence and the measured credential table are in
> `docs/planning/deferred/DEFER-009_railway-deploy-blocked-feedbackmonk-api.md` § ROOT CAUSE, and the
> one-paragraph version is in `CLAUDE.md` § "The trap that cost a week". Everything in this section
> is left as written, because how it went wrong is the useful part: every measurement below is
> individually correct, and the single thing that could not be measured — what credential the service
> itself was using — was the broken thing.

Three `serviceInstanceDeployV2` attempts, all **FAILED in 2–4 seconds** with **empty `buildLogs` AND
empty `deploymentLogs`** and `diagnosis: null`:

| Deployment | Image | Result |
|---|---|---|
| `c9200c3e-9764-4014-9171-ffeb23049e1c` | `feedbackmonk-api:0.4.0` | FAILED (2.1s) |
| `a641150b-541f-4e5b-aaf5-1147154daf73` | `feedbackmonk-api:0.4.0` + explicit `registryCredentials` | FAILED (~2s) |
| `2e5a6af2-50dc-4983-a052-de6bb42a9d4f` | **`feedbackmonk-api:0.2.0` — the known-good image already running** | **FAILED (3.7s)** |

**The third attempt is the decisive one**: the image that is running *right now* also refuses to
deploy. So this is NOT the new image, NOT the migrations, and NOT the code.

Ruled out by measurement:
- **Registry auth** — `gitcellar-push` fetches the `0.2.0` and `0.4.0` manifests over HTTPS (both 200;
  anonymous is 401, so auth is genuinely being used). Credentials were also passed explicitly via
  `ServiceInstanceUpdateInput.registryCredentials` on attempt 2.
- **Image shape** — `0.4.0` and `0.2.0` have byte-identical `Entrypoint` (`["/usr/bin/tini","--"]`),
  `Cmd`, `User`, `WorkingDir`, `ExposedPorts`, and both are `linux/amd64` with a v2 manifest.
- **Staged changes** — `environmentPatchCommitStaged` returns "No patch to apply"; the pin applies
  immediately.
- **Project state** — `subscriptionType: pro`, `deletedAt: null`, not a temp project.

Not determinable with a project-scoped token: account-level billing/payment state, and whether
*other* services can still deploy (every other service's last deployment is SUCCESS but all are from
July/August — none were attempted today, and deploying unrelated production services purely as a
test was not authorised).

**Leading hypothesis for the owner to check in the Railway dashboard**: a workspace-level block on
new deployments (payment/usage) or a region-scheduling failure — both present exactly this way,
i.e. existing containers keep serving while every new deployment dies instantly with no logs.

### Current state — SAFE and CONSISTENT

- Service pin **reverted to `feedbackmonk-api:0.2.0`**, matching the container that is actually
  running, so a restart cannot land on an unpullable tag.
- `feedback.gitcellar.com/health/ready` **200**; `/api/v1/capabilities` **0.2.0**;
  `triage.gitcellar.com` **200**.
- **The DB schema (00030) is intentionally ahead of the running code (0.2.0).** This is supported and
  was the point of migrating first: every pending migration is backward-compatible with 0.2.0
  (additive nullable/defaulted columns, constraint weakenings, and `00019`'s transactional
  drop+re-add of a generated column). Verified live for ~30 min at 200. It is not a state to sit in
  indefinitely, but it is safe.

### Support thread (filed 2026-09-02 ~14:50 UTC)

Private Railway "Technical Help" thread: **"Deploys fail instantly at CREATE_CONTAINER, no logs,
no volume — old container still serving"**. Status **Awaiting Railway Response**, **0 replies as of
2026-09-03 02:15 UTC (11 h)**. Carries all four deployment IDs incl. the 14:43 UTC retry, and asks
explicitly that it not be converted to a community bounty.

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


A fourth attempt (`e12b3923-951a-4397-98c1-07831681700d`) was made at 2026-09-02 14:43 UTC — 11.5 h
after the first — and **failed identically in 4.4 s**, which rules out a transient condition that has
since cleared. Pin was reverted to `0.2.0` afterwards.

**Image integrity re-verified 2026-09-03**: every layer blob of `feedbackmonk-api:0.4.0` (10/10) and
`feedbackmonk-admin-ui:0.1.3` (11/11) is present in the registry, HTTP 200. A Docker daemon restart
at ~03:14Z did not corrupt the push.

### To finish once deploys work again

No rebuild needed — both images are already in the registry. Repoint + deploy:
`feedbackmonk-api` (`50e4291d-b411-4388-a6e5-1f9d47ec8623`) → `0.4.0`, then
`feedbackmonk-admin-ui` (`48918bae-fd9f-459a-b280-f2cc9290e640`) → `0.1.3`,
environment `15941208-6a62-4f8a-ad18-013d67c76df5`, header **`Project-Access-Token`** (NOT
`Authorization: Bearer` — that returns `"Not Authorized"` at HTTP 200). Then verify per
`docs/operations/RAILWAY_GITCELLAR.md` §8, and confirm in a **browser** that a won't-fix row opens.

---

## VERIFIED live state (curl-confirmed 2026-06-03)

| Surface | Result | Probe |
|---|---|---|
| API health (custom domain) | **200** `{"status":"ok","db_connected":true,"version":"0.1.0"}`; up since **2026-06-03T02:07:32Z** | `GET https://feedback.gitcellar.com/health/ready` |
| API health (Railway direct) | **200** | `GET https://feedbackmonk-api-production.up.railway.app/health/ready` |
| Project live + config | **200** — `display_name:"GitCellar"`, footer `"powered by feedbackmonk"` (free-tier footer, FR-FBR-14), `auth_modes:["auth","anonymous"]` | `GET …/api/v1/projects/<PID>/widget-config` |
| **CORS posture (DEC-FBR-IMPL-09)** | **APPLIED & WORKING** — preflight from `Origin: https://gitcellar.com` → 200 + `Access-Control-Allow-Origin: https://gitcellar.com` + `Allow-Credentials: true` + `Allow-Methods: POST,OPTIONS`. `FEEDBACKMONK_CORS_ORIGINS` IS set on the deployed service. | `OPTIONS …/api/v1/projects/<PID>/feedback` |
| SaaS hosts | **not stood up** (expected — self-host-only decision) | `feedbackmonk.com` / `api.feedbackmonk.com` do not resolve |
| Hosting | Railway, region `us-east4-eqdc4a` (`Server: railway-edge`) | response headers |

## Identity / integration facts (from committed `docs/integrations/gitcellar-adoption.md`)

- **`project_id`** = `a1350be8-3ff5-4744-9e1d-e35c97cc8aad` (the JWT `aud` + URL path segment)
- **`tenant_id`** = `020c637c-63cf-4367-b5ba-999a81c2d22a`
- **tenant / admin** = `triage@gitcellar.com`
- **signing key_id** = `4704a9b4-4798-4d2c-a2ed-ba49f887fe6e` (Ed25519 public key registered, Contract C4)
- **anon submit verified end-to-end** = `FB-4R3VS8` (2026-06-02)
- **API base** = `https://feedback.gitcellar.com` (custom domain) / `https://feedbackmonk-api-production.up.railway.app` (Railway direct)
- **deploy model** = GitCellar self-hosts on its existing Railway, reusing its existing Postgres
  (feedbackmonk gets its own `feedbackmonk` database; multi-tenant on one Postgres by design, DEC-FBR-03)

## Authoritative references (read these for the how)

- **Deploy runbook (operator procedure)**: `docs/operations/RAILWAY_GITCELLAR.md`
- **Env catalog (Contract C21)**: `docs/operations/SELFHOST_ENV.md`
- **Integration contract (the meeting point with GitCellar)**: `docs/integrations/gitcellar-adoption.md`
- **Provisioning script**: `scripts/provision-gitcellar.sh` (signup → verify → create-project → register-signing-key → prints `project_id`)
- **Self-host runbook**: `docs/operations/SELFHOST.md`
- **CLAUDE.md** § PF-DEPLOY-01

---

## Re-publish / launch context — CONFIRMED from the GitCellar repo

> **The prior session's resume notes were not lost — they are committed in the GitCellar repo**
> (`E:\Developer\SourceControlled\Apps\GitCellar`, commit `82eaf2ebea`,
> `docs/planning/feedbackmonk-deploy-state.md` + `docs/PROJECT_TRAJECTORY.md`). The prior session was
> doing **GitCellar-side adoption work** and crystallized its deploy-state there, which is correct —
> the remaining adoption work (publish, Desktop cutover) is GitCellar-side. From *this* repo it looked
> "lost" only because no copy was written here. **GitCellar's note is authoritative for the
> publish/cutover resume; this note is feedbackmonk's view + a pointer to it.**

What actually happened (from GitCellar's committed record):

- The full gitcellar.com Astro landing site + feedback widget **was deployed and verified working
  end-to-end ~05:00–05:28 UTC 2026-06-03 (~28 min), then REVERTED to the placeholder per user
  direction.** gitcellar.com must remain the pre-launch **PLACEHOLDER** until launch.
- **Only the public publish was rolled back.** The feedbackmonk backend stays live; all three integration
  gates were resolved and remain live:
  - **CORS** — feedbackmonk added the layer (this repo, `9d1df3c`, DEC-FBR-IMPL-09); GitCellar rebuilt
    its image to `0.1.1`, set `FEEDBACKMONK_CORS_ORIGINS=https://gitcellar.com`, redeployed. (Verified
    live above.)
  - **Cert** — `feedback.gitcellar.com` TLS issued after adding the Railway ownership-verification TXT
    record via the Cloudflare API.
  - **Router** — `gitcellar-landing-router` Worker gained `/feedback/` + `/marketing-assets/` in
    `PAGES_MULTI_SEGMENT_PREFIXES`.
- **Re-publish at launch** (GitCellar-side):
  `pnpm -C apps/gitcellar-landing exec wrangler pages deploy dist --project-name=gitcellar-landing` —
  the widget activates immediately (CORS + cert + router already live). **Do NOT publish the full site
  to prod before launch.**

### GitCellar-side infra facts (recorded for cross-repo orientation; authoritative copy in GitCellar's note)

- **WCM = Windows Credential Manager.** Deploy secrets live there under:
  `gitcellar-railway-account-token` (workspace-scoped, **GraphQL-only** — CLI can't use it),
  `gitcellar-cloudflare-dns-edit`, `gitcellar-registry-push`,
  `gitcellar-feedbackmonk-{session-secret,ops-password,jwt-private}`.
- **Railway**: project `fab620f1-…`, prod env `15941208-…`, `feedbackmonk-api` svc `50e4291d-…`,
  Postgres svc `8573408b-…`.

### Remaining adoption work — ALL GitCellar-side, none in this repo

1. **At launch** — re-publish the full landing (command above).
2. **Stage 3** — Desktop migration + cutover: mint EdDSA JWT from Desktop's Ed25519 identity per the
   frozen contract `docs/integrations/gitcellar-adoption.md` §5 / §5.6 / §6 (signing private key in WCM
   `gitcellar-feedbackmonk-jwt-private`), then disable (not delete) GitCellar's internal feedback backend
   + Forge bridge. feedbackmonk parity = 4/4 (`feedback-parity-status` oracle GATE OPEN).
3. **GLITCHTIP** — create a Glitchtip read token, set `FEEDBACKMONK_GLITCHTIP_{URL,ORG,PROJECT,TOKEN}` on
   the `feedbackmonk-api` service.
4. **admin-ui triage dashboard** — BLOCKED on a feedbackmonk-side login/magic-link (session is currently
   issued only by verify-email; no re-login endpoint). The one item that, if pursued, *would* be
   feedbackmonk dev work — but it's a post-launch nicety, not a blocker.

---

## Doc-drift to fix (follow-up, not blocking)

- `docs/integrations/gitcellar-adoption.md` **§0 TL;DR table** still shows `pending deploy` for API base /
  `project_id` / widget URL — **stale**: the deploy is done and ACTIVE (the doc's own header line + §3.2
  changelog already say ACTIVE/DONE). Update the §0 table cells to `live ✅` for consistency.
