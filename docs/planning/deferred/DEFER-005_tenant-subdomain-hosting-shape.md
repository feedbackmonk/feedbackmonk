---
id: DEFER-005
title: Tenant-subdomain hosting shape + custom domain as the paid upgrade, and move first-party products onto the SaaS
status: PROPOSED
origin: inject
source-project: GitCellar
source-session-id: interactive-20260830T065606Z-514006
injected-at: 2026-08-30T07:05:00Z
autonomy-hint: autopilot
suggested-entry-point: spec
scope-estimate: multi-session
content-hash: fbm-hosting-shape-dec13-14-v1
---

# DEFER-005: Tenant-subdomain hosting shape + custom domain as the paid upgrade, and move first-party products onto the SaaS

> **Counts, file locations and line numbers below are snapshots taken by the named commands on
> 2026-08-30 — re-measure before acting.** The fix shapes and acceptance criteria are the filer's
> hypotheses from outside the work; re-derive both against the live tree, and reject in-lane any
> remedy that cannot be demonstrated red-first.

## Idea

Two owner decisions were taken on 2026-08-30 and are recorded in this repo as **DEC-FBR-13** and
**DEC-FBR-14** (`docs/specs/DECISIONS.md`, section "Commercial hosting shape"). Neither has any
implementation. Build them.

- **DEC-FBR-13** — default every public surface to a **tenant subdomain** (`{tenant}.feedbackmonk.com`),
  sell the **customer's own domain via CNAME** as the paid-tier upgrade covering **both** the public
  board **and** the widget/API endpoint, keep **admin/triage permanently** on one feedbackmonk-owned
  host, and leave custom email `From:` deferred.
- **DEC-FBR-14** — stand up the SaaS at `feedbackmonk.com`, make GitCellar a **tenant** of it (later
  quiqpic and SessionHelm), and CNAME GitCellar's existing hosts at that instance instead of at the
  current self-hosted single-tenant deployment — **with no user-visible change and no GitCellar
  source edit**.

Read both DEC entries in full before planning; they carry the rejected alternatives and the
per-surface table, and this brief does not restate them.

## Originating Context

The owner asked, from the GitCellar repo, how feedbackmonk should work when adopted by the *other*
sibling products — should feedback live at each app's own domain (the way GitCellar does today at
`triage.gitcellar.com`), or at feedbackmonk.com (`feedbackmonk.com/GitCellar` or
`GitCellar.feedbackmonk.com`)? And would the answer differ if it stayed first-party-only?

Investigating that turned up four things that shaped the decisions:

1. **The public board is a route inside the ADMIN SPA.** `admin-ui/src/App.tsx:35,65` maps
   `/public/projects/:projectId/board` to `PublicBoard` in the same bundle that serves `Login.tsx`,
   triage, moderation and settings. Under the path-based shape the P3 gate had committed to, every
   tenant's board — rendering user-submitted text, which *is* this product's core content — would
   share one origin with every other tenant's board **and with the admin console**. Same-origin
   policy is per-origin, not per-path; no path structure fixes it. This was the decisive argument
   for subdomains, and it is recorded in `docs/planning/observations-ledger.md` (2026-08-30) as a
   conditional finding: no harm is observed today because the only live deployment is
   single-tenant self-host, and the harm was conditional on this very decision. **Now that the
   decision has gone the other way, verify the ledger line's premise still holds and judge whether
   the board should additionally be split out of the admin bundle** — the decision does not
   require that split, but it is the belt-and-braces version and the call is the lane's.
   (Command: `grep -n "PublicBoard\|path=" admin-ui/src/App.tsx`.)

2. **The API never inspects the Host header.** Every route is `project_id`-in-the-path —
   `handlers/board.rs:80` (`/api/v1/projects/:project_id/board`), `handlers/roadmap.rs:179`. So
   host-to-tenant resolution is **new work under any option**; the subdomain choice did not create it.
   (Command: `grep -rn "route(" crates/feedbackmonk-api/src/handlers/board.rs crates/feedbackmonk-api/src/handlers/roadmap.rs`.)

3. **The custom-domain tier flag is `true` with no implementation**, and has been since P3 — the
   P3 commercial-gate plan's Deferred table row "Custom domain feature" says exactly that, and
   DEC-FBR-08's OUT list says "Custom domains for $29+ tier — wire up post-launch". The marketing
   `PricingCard.astro` already advertises "Custom domain".

4. **The dogfooding gap is the likely reason for #3.** GitCellar runs a *self-hosted single-tenant*
   instance on its own Railway (`docs/planning/feedbackmonk-deploy-state.md`), so the multi-tenant
   SaaS this project intends to sell is never exercised by its own author — custom-domain routing,
   wildcard TLS, per-tenant isolation under real traffic all go untested by use. DEC-FBR-14 exists
   to close that: become customer #1 of the custom-domain feature so it gets built because it is
   needed, not because a pricing card promises it.

Also load-bearing for scoping the paid tier: the **AGPL self-host option (DEC-FBR-05) is the real
competitor** to the paid tier. That is why DEC-FBR-13 puts the custom domain on the widget/API
endpoint and not only the board — a first-party endpoint dodges tracker blocklists, survives a
strict CSP `connect-src`, and reads as first-party in a customer's security review. That is a
harder-to-self-host benefit than a vanity URL. The widget already takes `data-api-base`, so this
half may be nearly free.

And the free-tier flywheel must survive: default-tier visitors still land on a `feedbackmonk.com`
origin with the badge intact (`footer_url`, DEC-FBR-IMPL-11). The escape from the badge is what is
being sold — do not let a redesign give it away by default.

## Success Criterion

Judged by this project's own standards, roughly:

- Public surfaces resolve by **host**, tenant-scoped, with `{tenant}.feedbackmonk.com` as the
  default and the existing `project_id` routes still working (or deliberately migrated with
  redirects — the lane's call).
- A tenant on the paid tier can point **their own domain** at both their board and their
  widget/API endpoint, end-to-end, with TLS issued automatically; the tier gate is enforced
  server-side, not just in the pricing card.
- Admin/triage is reachable on exactly one feedbackmonk-owned host and is **not** reachable on a
  tenant or custom domain.
- Multi-tenant isolation still green: the `multi-tenant-isolation-check` oracle must not regress,
  and host-based resolution must not become a new way to cross tenants. A new Verification Oracle
  for host-to-tenant resolution is a strong candidate — author its `assertion` block first.
- GitCellar is a **tenant** of the `feedbackmonk.com` instance, with `feedback.gitcellar.com` and
  `triage.gitcellar.com` still serving, no broken links, **and no commit to the GitCellar repo**.
  GitCellar's `TRIAGE_URL` constant (`apps/gitcellar-cloud/admin-ui/src/featureFlags.ts:15`) must
  be left untouched — if a migration plan needs to edit it, the plan has misread DEC-FBR-14.

## Dependencies

- DEC-FBR-14 depends on DEC-FBR-13 — the CNAME target only exists once subdomains do. Sequence
  accordingly; they are not parallel.
- Wildcard DNS + TLS on `*.feedbackmonk.com`, and per-custom-domain ACME. Deployment currently
  rides GitCellar's Railway (`docs/operations/RAILWAY_GITCELLAR.md`); standing up an independent
  `feedbackmonk.com` instance is an ops decision this brief does not pre-make.
- The GitCellar-side cutover is DNS + tenant migration only. **Coordinate before touching anything
  in the GitCellar repo** — per DEC-FBR-07 this repo does not modify GitCellar, and DEC-FBR-14
  explicitly requires no GitCellar source edit. Live GitCellar sessions exist on this machine; a
  DNS cutover on `feedback.gitcellar.com` would move a surface they may be measuring against.

## Related Artifacts

- `docs/specs/DECISIONS.md` — **DEC-FBR-13**, **DEC-FBR-14** (section "Commercial hosting shape"), and
  **DEC-FBR-06** which carries the back-annotated superseded URL-shape line.
- `docs/specs/OPEN_QUESTIONS.md` — Q21, Q22 (both RESOLVED, pointing at the DECs).
- `docs/planning/observations-ledger.md` — the 2026-08-30 shared-origin line.
- `docs/planning/plans/20260514T134816-feedbackmonk-p3-commercial-gate.md` — the Deferred table row
  "Custom domain feature", and the tier/quota machinery the gate must hook into.
- `admin-ui/src/App.tsx` — the admin/public-board routing seam.
- `crates/feedbackmonk-api/src/handlers/board.rs`, `.../roadmap.rs`, `.../main.rs` — public routers.
- `crates/feedbackmonk-repository/` — the tenant-scoped repository layer (DEC-FBR-03: sole query path).
- `widget/src/` — `data-api-base` is the seam that makes the custom endpoint nearly free.
- `marketing/src/components/PricingCard.astro` — already advertises "Custom domain".
- `docs/planning/feedbackmonk-deploy-state.md` — the current GitCellar single-tenant deployment.
- `docs/operations/RAILWAY_GITCELLAR.md` — where it runs today.
