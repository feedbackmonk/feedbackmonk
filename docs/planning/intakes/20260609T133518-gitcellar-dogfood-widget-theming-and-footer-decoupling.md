# Intake — Widget theming + footer/tier decoupling (surfaced by GitCellar dogfooding)

**Created**: 2026-06-09 · **Origin**: GitCellar session observed two issues while testing the
live FeedbackMonk widget on GitCellar's Cloud Forge. Both levers live entirely in **this**
(FeedbackMonk) repo, so the work belongs here, not in GitCellar. This brief front-loads the
discovery so this session doesn't have to re-derive it.

> **Who the reporter is**: GitCellar is the *owner* of FeedbackMonk and its first real tenant
> (dogfooding). GitCellar's tenant = `triage@gitcellar.com`, project_id
> `a1350be8-3ff5-4744-9e1d-e35c97cc8aad`, currently **Free tier**. Contract:
> `docs/integrations/gitcellar-adoption.md`. Live API: `https://feedback.gitcellar.com`.

---

## What was observed

A screenshot of the widget modal embedded on GitCellar's dark-themed Cloud Forge showed:

1. **Theming mismatch** — the modal is a fixed **light** card (white surface, dark text)
   sitting on a dark host page. It looks bolted-on, not integrated.
2. **"powered by feedbackmonk" footer** — present (correct for Free tier), but the link target
   `https://feedbackmonk.com` **does not exist yet** (marketing site unbuilt). So the badge
   currently advertises a dead link.

The reporter's actual requirement on the footer is subtle and important:
**they *want* the badge long-term** (it's FeedbackMonk's own marketing) — they only want it
**suppressed until feedbackmonk.com is live**, and restorable with a flip when it is.

---

## Issue A — Footer is welded to tier; decouple it

### Current mechanism (the conflation)
`crates/feedbackmonk-core/src/tier.rs` → `tier_quotas()`:
- `Tier::Free` → `footer_text: Some("powered by feedbackmonk")`
- `Tier::Starter | Pro | SelfHost` → `footer_text: None` (widget renders no footer)

The widget renders the footer iff `brand.footer_text` is truthy in the `widget-config`
response; the `<a>` href is **hardcoded** to `https://feedbackmonk.com` in `widget/src/ui.ts`.
`crates/feedbackmonk-repository/src/tenants.rs` builds the brand row with
`footer_text: tier_quotas(tier).footer_text` (purely tier-derived — no per-tenant override
column today). Guarded by FR-FBR-14 / Contract C19, asserted by the `tier-enforcement-status`
Verification Oracle Probe B and the unit tests in `tier.rs` / `admin_tier.rs`.

### Why "just upgrade GitCellar's tier" is the wrong long-term answer
- It conflates **badge visibility** with **quota tier**. Hiding the badge would force GitCellar's
  own dogfood tenant onto paid quotas, and *restoring* the badge later would force it back to
  Free's 50-feedback/month cap. The owner's tenant should have generous quotas **and**
  independently-controllable branding.
- The "free users advertise us" brand promise (FR-FBR-14) must stay intact for **real external
  free tenants** — they must not be able to remove the badge themselves.

### Recommended design — per-tenant, admin-only footer override
Add a nullable per-tenant override that supersedes the tier default:
- `None` (default) → fall back to `tier_quotas(tier).footer_text` (FR-FBR-14 unchanged for
  external free tenants).
- `Some(suppressed)` / `Some(custom text)` → explicit per-tenant value, **settable only via the
  admin/ops surface**, never self-serve (or free tenants would strip the badge).
- Migration adds the column; `widget_config` + `tenants` brand assembly consult the override
  first, tier default second. Update Probe B / Contract C19 expectations **with a `DEC-FBR-*`
  decision entry** (the oracle will otherwise flag drift — that's by design).
- Consider also making the footer **href** configurable (`footer_url`) for future white-label,
  and so the GitCellar badge can later point at the real marketing URL without a widget rebuild.

Then: set GitCellar's tenant to **SelfHost** (honest label — owner-operated instance: unlimited
volume, custom_branding) **and** set its footer override to suppressed for now. When
feedbackmonk.com launches, clear the override → badge returns. Quotas and branding now move
independently.

### Prerequisite gap — there is no admin *set-tier* / *set-branding* endpoint
`crates/feedbackmonk-api/src/handlers/admin_tier.rs` exposes only `GET /api/v1/admin/tier`
(read). The only writer is the test-only `set_tier_for_test` repo helper. Today the *sole* way
to change a tenant's tier in prod is raw SQL on the Railway `feedbackmonk` Postgres
(`UPDATE tenants SET tier=… WHERE …`). For the long term, add a proper admin mutation endpoint
(the `admin_tier.rs` header comment already anticipates "Stage 2's admin UI tier-settings page"
as the consumer) covering tier + the new branding/footer override, then drive the GitCellar
flip through it rather than SQL.

---

## Issue B — Widget theming (the bigger feature)

### Current state
`widget/src/styles.css` hardcodes a light palette (`--fbm-surface:#fff`, `--fbm-text:#111827`,
…). The **only** per-tenant token is the accent `--fbm-primary`, sourced from
`brand.primary_color` — and even that is currently **hardcoded** to `#3b82f6` in
`tenants.rs` (~L343), not actually per-tenant. There is **no** dark variant, **no**
`prefers-color-scheme` handling, and (correctly) no attempt to read the host page's CSS.

### Framing — this is normal, and the fix is a theme knob (not CSS inheritance)
A cross-origin embed *cannot* read the host's CSS variables, so true "inherit the client's
theme" is not how embed widgets work (Sentry User Feedback, Canny, Featurebase, Marker.io all
ship self-contained styling). The industry-standard fix is an explicit
**`theme: auto | light | dark`** knob the embedder sets. Recommended:
- Add a dark set of CSS variables in `widget/src/styles.css`, toggled by a `data-theme` /
  root class.
- Resolve theme from (priority): embed attribute `data-theme` on the script tag →
  per-tenant brand default → `auto` (follow `prefers-color-scheme`). Touch points:
  `widget/src/widget.ts` (read attribute/config), `widget/src/ui.ts` (apply), `styles.css`
  (dark vars).
- Make `primary_color` (and `logo_url`) genuinely per-tenant via the brand config /
  `custom_branding` capability (it's flagged "forward-looking P4+" in `tier.rs` today). This is
  the natural home for the footer override above too — one "branding" surface.
- `crates/feedbackmonk-api/src/handlers/widget_config.rs` returns the theme/brand; extend its
  shape + tests.

GitCellar would then embed with `data-theme="auto"` (or `dark`) and set its accent → the modal
matches its dark Cloud Forge.

---

## Issue C — Launcher-less / embedder-trigger mode (auto-mount overlaps host chrome)

### What surfaced it
On GitCellar's Cloud Forge, `widget.js` auto-mounts its floating `.fbm-launcher`
(`position:fixed; right:24px; bottom:24px; z-index:2147483600`). The host page already provides
its **own** trigger — a navbar "Send feedback" button — so the floating launcher is **redundant**
*and* it **overlaps the page footer's Terms/Privacy links**. GitCellar has shipped an interim
hide (`.fbm-launcher{display:none}` inline in its Forge footer template), but that only works
because of a quirk: the host trigger opens the modal by **`document.querySelector('.fbm-launcher').click()`**,
which still fires on a `display:none` element. So the launcher must stay in the DOM as a hidden
dummy click-target — a hack the widget should make unnecessary.

### The real gap
`widget.js` supports `data-fbm-no-auto-mount` (skip the launcher) — **but exposes no public API
to open the modal otherwise.** The only open path is clicking `.fbm-launcher`. So an embedder
that brings its own trigger is stuck: either keep the floating launcher (overlap/redundancy) or
suppress it and lose the ability to open the modal entirely.

### Recommended design
When `data-fbm-no-auto-mount` is set, give embedders a real open mechanism — either (preferred)
**auto-wire any `[data-feedback-open]` element on the page to open the modal** (so the host just
marks its own button — no JS glue, no dummy launcher), and/or expose **`window.feedbackmonk.open()`**
(or have `mountFeedbackMonk()` return an `{ open, destroy }` handle). Then GitCellar flips its
Forge embed to `data-fbm-no-auto-mount`, drops the dummy-hide CSS, and marks its navbar button
`[data-feedback-open]` — clean, no floating button, no overlap. Touch points:
`widget/src/widget.ts` (auto-mount gate + trigger wiring), `widget/src/ui.ts` (open entry),
the exported surface in `widget/src/widget.ts`.

> Note `[data-feedback-open]` is *already* GitCellar's convention (its `feedback-trigger.js`
> shim wires it today by clicking the launcher). Making the widget honor that attribute natively
> would let GitCellar delete the shim entirely.

## Cross-repo propagation back to GitCellar (do NOT patch downstream)

GitCellar serves **vendored, re-synced copies** of the widget build — editing them is pointless
(overwritten on next sync). After the widget rebuild (`widget/dist`), the GitCellar side must
re-sync:
- `apps/gitcellar-landing/public/feedback/{widget.js,redact.js,widget.css}` (landing + router)
- `forge-versions/current/custom/public/assets/feedback/` (both Forge navbars; `git add -f`)
and then, **once Issue C lands**: flip the Forge embed (`forge-versions/current/custom/templates/
custom/footer.tmpl`) to `data-fbm-no-auto-mount`, mark the navbar button `[data-feedback-open]`,
**delete** the interim `.fbm-launcher{display:none}` `<style>` (footer.tmpl) and the
`feedback-trigger.js` shim, and set the embed's `data-theme` + accent. Capture this as the closing
cross-repo follow-up (a GitCellar session does the re-sync; coordinate via the contract doc).

---

## Recommended sequencing

This has two real design decisions (footer-override shape; theme-resolution API), so run a
short LDIS spec/intake pass before building — don't free-hand it:

1. **Spec** the footer-override + admin-mutation endpoint, the theme-knob API, and the
   launcher-less trigger API (Issue C) — decision entries `DEC-FBR-*`; update Contract C19 /
   Probe B expectations deliberately.
2. **Build** footer decoupling + admin set-tier/branding endpoint → flip GitCellar tenant to
   SelfHost + footer suppressed.
3. **Build** dark-theme + real per-tenant primary_color/logo (Issue B).
4. **Build** launcher-less / `[data-feedback-open]` + public open API (Issue C).
5. **Rebuild** `widget/dist`; hand GitCellar the re-sync + embed flip (`data-fbm-no-auto-mount`,
   `data-theme`, drop the interim hide + shim).
6. **Verify** (below), then update `docs/planning/feedbackmonk-deploy-state.md`.

## Open decisions to resolve in the spec
- Footer override granularity: boolean suppress vs. nullable custom-text vs. both (+ `footer_url`?).
- Theme resolution precedence + default (`auto` recommended).
- Admin-endpoint auth scope (ops-only; must not be reachable by tenant self-serve).
- Does GitCellar's tenant want the badge back **at all** at launch, or stay branding-suppressed
  permanently? (Reporter implied "bring it back when feedbackmonk.com is live" → temporary.)

## Verification expectations
- FR-FBR-14 still holds for external **Free** tenants (badge present; not self-removable) — the
  oracle Probe B + tier tests stay green or are updated via an explicit DEC.
- `widget-config` for GitCellar's tenant returns suppressed footer + chosen theme/accent.
- Live round-trip against `https://feedback.gitcellar.com` after redeploy (the deploy-state doc
  has the Railway image/redeploy recipe: `serviceInstanceUpdate(source.image)` +
  `serviceInstanceDeployV2`, NOT `Redeploy`).
- Dark modal renders correctly on a dark host; light on light; `auto` follows the OS.
- With `data-fbm-no-auto-mount`, **no** `.fbm-launcher` is created, yet a host
  `[data-feedback-open]` element (and/or `window.feedbackmonk.open()`) opens the modal (Issue C).

## Key facts / IDs / infra (all on this machine)
- GitCellar tenant `triage@gitcellar.com`, project_id `a1350be8-3ff5-4744-9e1d-e35c97cc8aad`, Free.
- API svc `feedbackmonk-api` id `50e4291d-b411-4388-a6e5-1f9d47ec8623`; Railway project
  `fab620f1-…`, env production `15941208-…`, Postgres svc `8573408b-…` (db `feedbackmonk`,
  proxy `switchback.proxy.rlwy.net:30877`, user postgres). Railway token WCM
  `gitcellar-railway-account-token` (**workspace-scoped → GraphQL-only**, CLI can't use it).
- Registry push creds WCM `gitcellar-registry-push`. Admin/triage UI at
  `https://triage.gitcellar.com` (ops `triage@gitcellar.com`, pw WCM
  `gitcellar-feedbackmonk-ops-password`).
- Deploy recipe + history: `docs/planning/feedbackmonk-deploy-state.md`.

## Guardrails
- Don't weaken FR-FBR-14 for real free tenants; the override is **admin-only**.
- Any change to `tier_quotas()` shape / Contract C19 needs a `DEC-FBR-*` entry (Probe B guards it).
- Don't edit GitCellar's vendored widget copies — change `widget/src`, rebuild, re-sync downstream.
</content>
</invoke>
