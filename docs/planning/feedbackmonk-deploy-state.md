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
>   **tenant_id** (`020c637c-…`), not the project_id. Ops token in **WCM
>   `gitcellar-feedbackmonk-ops-token`**.
> - **GitCellar-side**: widget re-synced + Forge embed flipped to launcher-less + dark (GitCellar
>   commit `bfa5562e23`); verified live on Cloud Forge :3222 — no launcher, navbar button opens dark modal.
>
> Authoritative operator log of this deploy: GitCellar repo
> `docs/planning/feedbackmonk-deploy-state.md` § Stage C. Original checklist:
> `docs/planning/followups/20260609-gitcellar-widget-theming-and-footer-decoupling-resync.md`.

---

## Stage F (2026-09-10) — **DEPLOYED**: `feedbackmonk-api:0.4.0` + `feedbackmonk-admin-ui:0.1.3` LIVE; DEFER-009 resolved

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

### BLOCKED — Railway will not deploy this service, for a reason unrelated to the image

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
