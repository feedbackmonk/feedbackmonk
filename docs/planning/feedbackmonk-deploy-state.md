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
