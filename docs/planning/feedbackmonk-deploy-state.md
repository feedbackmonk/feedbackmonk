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

## Stage E (2026-09-01/02) — DB MIGRATED to 00030; images built + pushed; **Railway deploys are BLOCKED**

**Trigger**: the owner reported `triage.gitcellar.com` white-screening on any won't-fix feedback row.
Root cause was a feedbackmonk bug (`FeedbackStatus::WontFix` serialised `wont-fix`, DB/clients use
`wontfix`), fixed at `d7dca56` + `c065b60`. **78 of the 79 production feedback rows are `wontfix`**
(verified by direct `SELECT`), so the triage inbox was ~99% unusable.

### DONE and verified

| Item | State |
|---|---|
| Pre-migration backup | `S:\_fbm-deploy-backupseedbackmonk-20260902T024909Z.sql.gz` — gzip-verified, 23 tables. Taken **by hand**: `gitcellar-pg-backup` targets the `railway` DB, NOT `feedbackmonk`. |
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
