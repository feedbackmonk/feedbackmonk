# Cross-repo follow-up — GitCellar re-sync + embed flip + prod redeploy

**Created**: 2026-06-09 · **Closes**: the dogfood intake
`docs/planning/intakes/20260609T133518-gitcellar-dogfood-widget-theming-and-footer-decoupling.md`
· **Decisions**: DEC-FBR-IMPL-11 (footer/tier decoupling + ops endpoint),
DEC-FBR-IMPL-12 (theme + per-tenant color/logo), DEC-FBR-IMPL-13 (launcher-less trigger).

This is the **closing follow-up** for the three widget/footer changes built in this repo. The
feedbackmonk-side code + tests + oracles + spec are DONE and committed. What remains is
**operational**: (1) redeploy the API to prod, (2) flip GitCellar's tenant via the new ops
endpoint, (3) re-sync the rebuilt widget into GitCellar and flip its embed. Steps 2–3 are a
**GitCellar-side session's** job; this doc is the authoritative checklist for it.

> ⚠️ **Prod is live** (`https://feedback.gitcellar.com`). Migration 00012 is additive + all-NULL
> (no backfill, no behavior change until an override is set), and the new ops endpoint is OFF
> unless `FEEDBACKMONK_OPS_TOKEN` is set — so this is a low-risk deploy. Redeploy recipe lives in
> `docs/planning/feedbackmonk-deploy-state.md` (`serviceInstanceUpdate(source.image)` +
> `serviceInstanceDeployV2`, **NOT** `Redeploy`).

---

## What shipped in this repo (done)

- **Migration `00012_tenant_widget_brand_overrides.sql`** — five nullable `tenants` columns
  (`footer_text_override`, `footer_url`, `widget_theme`, `widget_primary_color`,
  `widget_logo_url`). Runs on deploy via the migrate one-shot.
- **API**: `WidgetBrand` gains `footer_url` + `theme`, `primary_color` now nullable;
  `get_widget_brand` resolves override-over-tier-default; new ops endpoint
  `PATCH /api/v1/ops/tenants/{id}` (`OpsAuth` bearer-token guard, `FEEDBACKMONK_OPS_TOKEN`).
- **Widget** (`widget/dist` rebuilt, 22,905 B / 30,720 cap): dark theme (`data-theme`
  `auto|light|dark` + `prefers-color-scheme`), per-tenant `primary_color`/`logo`, launcher-less
  mode (`data-fbm-no-auto-mount` now initializes launcher-less), `[data-feedback-open]`
  auto-wiring + `window.feedbackmonk.open()/destroy()`.
- All Rust tests + 11 widget e2e + 7 Verification Oracles GREEN; clippy `-D warnings` clean;
  offline (`SQLX_OFFLINE`) `--all-targets` build clean.

## Key IDs (from the intake brief + deploy-state)

- GitCellar tenant `triage@gitcellar.com`, `project_id` `a1350be8-3ff5-4744-9e1d-e35c97cc8aad`,
  `tenant_id` `020c637c-63cf-4367-b5ba-999a81c2d22a`, currently **Free**.
- Railway: project `fab620f1-…`, prod env `15941208-…`, `feedbackmonk-api` svc `50e4291d-…`,
  Postgres svc `8573408b-…` (db `feedbackmonk`). Account token WCM `gitcellar-railway-account-token`
  (workspace-scoped → GraphQL-only). Registry push WCM `gitcellar-registry-push`.

---

## Step 1 — Redeploy feedbackmonk-api to prod (feedbackmonk operator)

1. Build + push a new `feedbackmonk-api` image (bump tag, e.g. `0.1.2`) per
   `docs/operations/RAILWAY_GITCELLAR.md` (creds WCM `gitcellar-registry-push`).
2. Generate and set the ops token on the Railway service (only needed to perform Step 2; it can
   be left set afterward — the endpoint is otherwise inert):
   ```
   FEEDBACKMONK_OPS_TOKEN = $(openssl rand -hex 32)
   ```
   Store it in WCM (suggest `gitcellar-feedbackmonk-ops-token`).
3. Redeploy: `serviceInstanceUpdate(source.image=<new tag>)` then `serviceInstanceDeployV2`
   (per deploy-state recipe). The migrate one-shot applies migration 00012.
4. Verify: `GET https://feedback.gitcellar.com/health/ready` → 200; widget-config still returns
   the Free footer for the (not-yet-flipped) GitCellar tenant.

## Step 2 — Flip GitCellar's tenant via the ops endpoint (replaces raw SQL)

Decouple branding from tier: give the owner tenant generous quotas **and** suppress the badge
**temporarily** (restore at launch per the settled product decision), and set the dark theme so
the modal matches Cloud Forge.

```
curl -X PATCH https://feedback.gitcellar.com/api/v1/ops/tenants/a1350be8-3ff5-4744-9e1d-e35c97cc8aad \
  -H "Authorization: Bearer $FEEDBACKMONK_OPS_TOKEN" \
  -H "content-type: application/json" \
  -d '{
        "tier": "self_host",
        "branding": {
          "footer_text_override": "",
          "theme": "dark",
          "primary_color": "<GitCellar accent hex, e.g. #6d28d9>"
        }
      }'
```

Response echoes the resolved tier + brand override. Confirm `resolved_widget_brand.footer_text`
is `null` (suppressed) and `theme` is `dark`.

**At feedbackmonk.com launch** — restore the badge (decoupled from tier; GitCellar stays SelfHost):
```
curl -X PATCH …/ops/tenants/a1350be8-… -H "Authorization: Bearer $FEEDBACKMONK_OPS_TOKEN" \
  -d '{ "branding": { "theme": "dark", "primary_color": "<accent>" } }'
```
(omitting `footer_text_override` clears it → falls back to the tier default; but SelfHost has no
tier footer — so to actively SHOW the badge on the owner tenant, set
`"footer_text_override": "powered by feedbackmonk"` and `"footer_url": "https://feedbackmonk.com"`).

## Step 3 — GitCellar-side re-sync + embed flip (GitCellar session)

> GitCellar serves **vendored, re-synced copies** of the widget build — editing them in place is
> pointless (overwritten on next sync). Re-sync from this repo's rebuilt `widget/dist`.

1. **Re-sync the build** (`widget/dist/{widget.js,redact.js,widget.css}`) into:
   - `apps/gitcellar-landing/public/feedback/{widget.js,redact.js,widget.css}` (landing + router)
   - `forge-versions/current/custom/public/assets/feedback/` (both Forge navbars; `git add -f`)
2. **Flip the Forge embed** (`forge-versions/current/custom/templates/custom/footer.tmpl`) now that
   Issue C has landed:
   - add `data-fbm-no-auto-mount` to the widget `<script>` (launcher-less),
   - add `data-theme="dark"` (matches Cloud Forge) and the accent if set via embed,
   - mark the navbar "Send feedback" button `[data-feedback-open]`,
   - **delete** the interim `.fbm-launcher{display:none}` `<style>` (footer.tmpl) **and** the
     `feedback-trigger.js` shim — both are now unnecessary (the widget natively honors
     `[data-feedback-open]` and creates no launcher).
3. Verify on Cloud Forge: no floating launcher, no footer overlap; the navbar button opens the
   dark modal; the badge is suppressed (until launch).

Coordinate via the integration contract `docs/integrations/gitcellar-adoption.md`.

---

## Verification expectations (from the intake brief) — status

- ✅ FR-FBR-14 holds for external **Free** tenants (badge present, not self-removable) — Probe B
  unchanged; Probe C scenario 4 proves override is admin-ops-only + supersedes.
- ⏳ `widget-config` for GitCellar's tenant returns suppressed footer + dark theme — **after Step 2**.
- ⏳ Live round-trip against `https://feedback.gitcellar.com` after redeploy — **Step 1**.
- ✅ Dark modal renders on a dark host; light on light; `auto` follows OS — widget e2e GREEN.
- ✅ `data-fbm-no-auto-mount` ⇒ no `.fbm-launcher`, yet `[data-feedback-open]` /
  `window.feedbackmonk.open()` open the modal — widget e2e GREEN.
