# feedbackmonk — Decision Log

**Format**: WHY, not WHAT. No dated entries (per ULADP). Decisions are immutable once recorded; superseded decisions get a SUPERSEDED-BY pointer rather than rewriting.

---

## Inherited from intake

### DEC-FBR-INTAKE-01: Standalone product, not internal reuse
The user explicitly chose "standalone SaaS product" over "internal reuse in other projects" or "fork-and-modify per project." This commits to abstracting away GitCellar-specific assumptions rather than copy-pasting.

### DEC-FBR-INTAKE-02: Spec session before any code moves
GitCellar is in pre-launch hardening. Destabilizing the working reference implementation before the abstraction is designed risks both projects. Extraction begins only after spec converges and a plan exists.

### DEC-FBR-INTAKE-03: New spec home (`docs/specs/feedbackr/`)
feedbackmonk's spec is a separate artifact from GitCellar's `docs/specs/feedback-system/` (which documents the GitCellar-integrated version). Editing the GitCellar spec for feedbackmonk concerns would muddy both. Move trivially if repo split happens later (per Q7).

---

## Triad decisions

### DEC-FBR-01: Target user persona — Persona A (indie/solo founders) + Persona D (privacy-first) combined, with A primary and D as differentiator

**Resolved**: 2026-05-13 (Q1 closed).

**Primary persona** (who the customer is): **A** — indie devs, solo founders, 2-3 person teams shipping side-projects / micro-SaaS / dev tools / indie games. 100-5,000 users. Closes-the-loop matters more than enterprise-grade triage. Pricing tolerance $9-79/mo range, will self-host to avoid SaaS lock-in.

**Differentiator persona** (why us, not them): **D** — privacy-conscious / EU-data-residency / no-third-party-trackers. Not a separate market; a positioning hook layered onto A. The privacy posture is the *reason* the product is simple, not a feature bullet.

**Explicitly NOT primary**:
- ❌ Persona B (small B2B SaaS) — too crowded (Canny/Featurebase/Productboard funded competitors). Welcome as a secondary aspirational segment once A+D foothold is built; not the target for v1.
- ❌ Persona C (mid-market customer-success) — requires sales motion (LinkedIn outreach, demos, MSAs) destructive to solo-founder focus. Long-term upmarket move possible (Plausible-style) but not v1.

**Why this persona combination**:
1. **Defensible niche given founder asset** — GitCellar's brand DNA is privacy-first/encryption-first/self-host-friendly. That brand asset transfers to feedbackmonk and CAN'T transfer to Canny/Featurebase/Productboard. Without it, feedbackmonk would be a worse Canny.
2. **Solo-founder go-to-market fit** — Product-led: SEO + Show HN + Twitter + GitCellar's eventual user base as a channel. No sales motion required.
3. **Underserved segment** — Canny is US-default and ad-tech-y; Fider is dated and SaaS-less; Featurebase targets B; Productboard targets C. The "modern, privacy-first, indie-friendly" slot is open.
4. **Proven template** — Plausible Analytics did this exact maneuver in adjacent category (privacy-first analytics vs Google Analytics): ~80M ARR, founder-controlled, no VCs, self-host + SaaS, EU-hosted. Replicable shape.
5. **GitCellar = customer #0**, and GitCellar's eventual user base (privacy-conscious devs) IS persona A+D — built-in distribution channel post-GitCellar launch.

**What this commits us to** (load-bearing — affects every subsequent decision):
- Open-source self-host + commercial SaaS (Sentry/Plausible/Posthog pattern) — strong lean for Q5.
- EU + US data residency from day one — a baseline, not a feature.
- No third-party trackers in the widget, ever (no Segment, no Mixpanel, no GA, no Intercom embed).
- GDPR DPA + data-export-API + hard-delete ready at launch.
- Pricing in the $9 / $29 / $79 range (Plausible-shaped, well under Canny's $79 floor).
- Optional E2E "sensitive feedback" toggle — opt-in feature extending GitCellar's encryption DNA without forcing it on the default flow. Genuine technical moat from existing Sequoia stack.
- Simplicity discipline — say NO to features that exist only to match Canny. The product must stay simple; saying no is harder than building.

**What this rules out**:
- Persona-B / Persona-C feature bloat in v1 (SSO/SAML, RBAC, audit logs, enterprise reporting, customer-impact scoring, etc.). These may surface later as upmarket moves; not v1.
- Pricing tiers above $99/mo for v1 (would alienate primary persona; signal to enterprise buyers that this isn't for them — fine).
- US-only hosting / US-only data residency.

### DEC-FBR-02: Market positioning — "Plausible Analytics for product feedback" / "Privacy-first product feedback"

**Resolved**: 2026-05-13 (Q2 closed).

**Hero pitch (H1)**: "Privacy-first product feedback. Hear your users without spying on them."
**Tagline / subhead**: "Product feedback that respects everyone in the loop."
**Elevator pitch ("X for Y")**: "Plausible Analytics for product feedback." Used in Show HN, Twitter, sales conversations, blog posts.

**Anti-positioning** (what we are explicitly NOT):
- ❌ Enterprise feedback platform (Productboard, Aha!) — no RBAC/SAML/complex workflows in v1
- ❌ Free-with-aggressive-upsell (Canny pattern) — free tier is honest, not dark-pattern
- ❌ Ad-tech-flavored (Featurebase) — no Segment / Mixpanel / GA / Intercom — period
- ❌ Dated open-source-as-only-option (Fider) — modern UX, self-host is a choice not a punishment
- ❌ GitHub Issues with extra steps — real roadmap + voting + customer-facing UX

**Per-competitor wedge**:
| Competitor | Our differentiator |
|---|---|
| **Canny** | EU residency, no trackers, $9 entry, self-host option |
| **Featurebase** | UX freshness + privacy posture + self-host |
| **Fider** | Modern UX + polished SaaS option + EU residency by default + supported product |
| **Productboard** | Wrong audience for them — built for indies + privacy-conscious teams |
| **GitHub Issues + Discussions** | Real roadmap + voting + status workflow + customer-facing UX (no GitHub account required) |

**Landing-page hero structure** (recommendation, may flex during marketing-site spec):
1. Above the fold: live widget screenshot embedded in fake SaaS dashboard + three trust signals ("EU + US hosting" / "Open-source core" / "Zero third-party trackers")
2. Scrolldown #1: public roadmap with voting (tangible differentiator vs "form on a site")
3. Scrolldown #2: status-loop email/notification flow ("we built it" / "we're working on it") — the closes-the-loop value prop

**What this rules out**:
- ❌ AI-powered feedback insights / sentiment analysis / LLM auto-categorization in v1 (dilutes privacy positioning, not what A wants)
- ❌ "Replace your support tool" framing — not Intercom, not Linear
- ❌ "For enterprise teams" — even if C buyers find us, messaging stays indie/privacy

**What this opens up later**:
- ✅ Show HN narrative ("open-source product feedback") — strong SEO + organic for A+D audience
- ✅ Sponsorship angle (Plausible-style: indie podcasts, GitHub Sponsors, conf sponsorships)
- ✅ Widget as viral artifact — free tier carries "powered by feedbackmonk" link; paid tiers opt-out. Low-key distribution.

---

### DEC-FBR-03: Multi-tenancy architecture — Shared PostgreSQL, `tenant_id` + `project_id`, row-level filtering, multi-product-per-tenant mandatory

**Resolved**: 2026-05-13 (Q3 closed — foundational triad complete).

**Shape**: shared PostgreSQL DB, every domain row carries `tenant_id` (org) and `project_id` (product). Row-level filtering enforced at the data-access layer (every query funnels through a tenant-scoped repository — no raw SQL bypass). Self-host distribution ships with the same schema; `tenant_id` defaults to a single seeded org and the UI hides cross-tenant affordances.

**Multi-product-per-tenant is mandatory**, not optional. Surfaced from user's sibling-projects context (GitCellar + quiqpic + SessionHelm = same buyer, three products). The canonical feedbackmonk customer shape is **one organization with N products**, each with its own widget URL, roadmap board, inbox, status emails, and branding. The admin UI is org-level (triage all products from one place); end-users are product-isolated (a GitCellar feedback submitter cannot see quiqpic feedback).

**Pricing tier flow** (informs Q5 / DEC-FBR-05):
- **Free**: 1 project, capped feedback volume (~50/mo), "powered by feedbackmonk" widget footer.
- **$9 / Starter**: 3 projects, higher volume, custom branding, no footer.
- **$29 / Pro**: unlimited projects, custom domain, EU residency selectable.
- **$79 / Self-host**: license key for own deployment, full multi-tenancy schema (in case customer hosts for their own customers — rare but the schema supports it).

**Why this and not the alternatives**:
- **Schema-per-tenant**: overkill at A+D scale; 10k+ tenants = 10k+ schema migrations; operational nightmare; self-host customers would have to run migrations against an artificial schema-per-tenant model they don't need.
- **DB-per-tenant**: reserves the "we'll move you to dedicated DB for $$$" upsell for Persona C, which we're not targeting. Cost-prohibitive at our pricing.
- **Self-host single-tenant only**: gives up SaaS revenue, makes multi-product-per-tenant impossible.
- **Chosen: shared DB + tenant_id + project_id**: matches Plausible (per-domain), Linear-self-host, PostHog OSS, Posthog Cloud. Cheap to run, single migration path, easy to scale to first 10k tenants. Self-host customers run with `tenant_id=default` and never notice the column.

**Load-bearing implications**:
- Every query MUST go through a tenant-scoped repository. Direct SQL is a security incident. Enforce via codegen / lints if practical.
- Row-level security policies in Postgres are a defense-in-depth option but NOT a substitute for the access layer. Postgres RLS overhead is non-trivial; use selectively.
- `project_id` is a first-class concept; APIs are namespaced `/api/projects/{project_id}/feedback`, etc.
- Widget keys are issued per-project (each project gets its own public ID for the embed code).
- Cross-project search and cross-product analytics happen at the org level (admin UI) but never bleed end-user-side.

**Foundational triad complete.** Spec moves to next-tier questions.

---

## Next-tier decisions

### DEC-FBR-04: End-user auth model — Three-mode hybrid per project (JWT primary / anonymous fallback / magic-link optional)

**Resolved**: 2026-05-13.

**Three modes**, customer selects per project (multiple modes can be enabled simultaneously per project):

**(a) Authenticated — JWT (primary)** — for app-embedded widgets where customer's product already has logged-in users (GitCellar Desktop, SessionHelm, customer SaaS dashboards).
- Customer's backend signs a short-lived JWT for the current user with a project-specific signing key registered to feedbackmonk.
- Widget passes JWT in `Authorization` header. feedbackmonk verifies signature, extracts `sub`, `email`, `name`, `external_metadata`.
- Algorithm: **EdDSA (Ed25519)**. Aligns with existing GitCellar crypto stack; smaller signatures than RS256; supported by every modern JWT library.
- Token TTL: **5 min sliding** (widget re-mounts re-issue). Leaked tokens low-risk; no thrash.
- Customer registers public keys per project; multiple active keys supported for rotation.
- Required claims: `sub`, `iat`, `exp`, `aud` (= project_id). Optional: `email`, `name`, `external_metadata` (arbitrary JSON ≤ 4KB).

**(b) Anonymous (fallback)** — for public surfaces with no auth (gitcellar.com landing page, OSS project READMEs, public roadmap browse pages).
- No JWT required. Widget submits with optional email field.
- Tracked by hashed IP + cookie (cookie-based dedup for roadmap voting; no PII).
- Per-project rate limits prevent spam.
- Optional anti-spam: customer can require verified email for submissions (magic-link verify step).

**(c) Magic-link (optional)** — for customers without their own user auth (rare for our persona).
- feedbackmonk sends one-time link, sets signed cookie. Substack-style.

**Per-project config flags** (admin UI):
- Allowed modes for this project (default: `auth` + `anonymous`)
- Domain allowlist for widget embed (CORS + iframe X-Frame-Options + CSP frame-ancestors)
- Optional: require verified email for anonymous submissions

**Privacy invariant (load-bearing — extends DEC-FBR-02 positioning)**: The JWT customer signs is the **only** identity feedbackmonk ever has for an end-user. feedbackmonk never calls back to the customer's auth provider, never syncs user lists, never accepts long-lived bearer tokens. **No identity-tracking surface area beyond what the customer hands us per-session.**

**Why this and not alternatives**:
- ❌ OAuth-via-customer-provider — redirect dance is jarring for "leave feedback" UX (Featurebase fumbled this); reserved for enterprise we're not targeting.
- ❌ PassKey-native (GitCellar's pattern) — doesn't generalize; not every feedbackmonk customer is a PassKey shop.
- ❌ Anonymous-only — loses identity continuity for status emails ("Hey Alice, we shipped your request"). Auth must be the default for app-embedded use.

**Coverage check against sibling-projects portfolio**:
- GitCellar Desktop, SessionHelm → mode (a) JWT.
- gitcellar.com landing, SessionHelm landing → mode (b) anonymous.
- quiqpic (CLI-only, no user auth) → mode (b) or (c) depending on whether quiqpic users want status emails.

---

### DEC-FBR-05: Business model — Open-source self-host (AGPL-3.0-or-later) + Commercial SaaS, same codebase

**Resolved**: 2026-05-13.

**Two distinct concerns**:
- **License** = AGPL-3.0-or-later. How the code is distributed.
- **Revenue model** = Commercial SaaS subscriptions ($9/$29/$79 per DEC-FBR-03), optional self-host support contracts later. How money flows in.

These are **not alternatives**. The hosted SaaS is the same product as the self-host distribution. **No private "Pro features" branch — single codebase, fully AGPL.**

**Revenue mix (target steady state, mirroring Plausible Analytics)**:
- SaaS subscriptions: ~90-95% of revenue
- Self-host support contracts: ~5-10% (offered later, when customers ask)
- Sponsorships / OpenCollective: small but real

**Why AGPL and not alternatives**:
- ❌ **MIT / Apache-2.0**: lets competitors fork commercially without contributing back. Too generous given our position.
- ❌ **BSL** (Sentry pattern): better cloud-cloning protection but trips OSS purists in our audience; not OSI-approved → no "real OSS" badge. Two-license maintenance burden — overkill at solo-founder scale.
- ❌ **SSPL** (MongoDB pattern): even more aggressive than AGPL; OSI-rejected; community distrust.
- ❌ **Proprietary**: forfeits DEC-FBR-02 privacy positioning. Persona D refuses closed-source feedback tools.
- ✅ **AGPL-3.0-or-later** (Plausible's choice): real OSS, OSI-approved, FSF-blessed. AGPL forces share-back from anyone running it as a service → kills the AWS-clone risk. Privacy-first community trusts the badge.

**Why AGPL doesn't reduce SaaS revenue**:
- Operating Postgres + Rust binary + email + EU R2 + custom domains + backups + uptime is real ops work. Most customers gladly pay $29/mo to not do it.
- AGPL doesn't block customers from using it commercially inside their business — only blocks running it as a *public managed service*.
- For Persona D specifically, AGPL is the entry ticket — without it, the privacy differentiation collapses.
- GitHub stars are free marketing. Plausible's 19k+ stars drive constant organic traffic at near-zero CAC.

**Estimate**: AGPL+SaaS produces ~2-3× the revenue of closed-source SaaS for feedbackmonk's specific positioning over a 3-year horizon. TAM expansion + cheap GTM compensates for the small self-host leakage (~5-10% of would-be paying customers).

**Concrete commitments** (lock now so they don't drift):
- All product code AGPL. No private "pro features" branch.
- Marketing site, landing, docs are open-source too (Plausible-style).
- Contributor agreement: lightweight DCO sign-off, not copyright-assignment CLA.
- Self-host customers get the same release cadence as SaaS customers.
- No artificial feature gating beyond Q3's pricing-tier caps (projects/volume).

**Reverse stress test — when AGPL would be wrong**:
| Scenario | Skip AGPL? |
|---|---|
| Targeting Persona C primary | Yes — some F500 have AGPL bans |
| Consumer (B2C) product | Yes — consumers don't care |
| Network-effects monopoly play | Maybe |
| **feedbackmonk** (B2D, privacy-first, indie/small-team) | **No — AGPL is right** |

feedbackmonk hits the "AGPL works" markers and none of the "AGPL hurts" ones.

---

### DEC-FBR-06: Roadmap backend — Native PostgreSQL data model + UI, drop Forge dependency entirely

**Resolved**: 2026-05-13.

**Decision**: Roadmap is a first-class native feature of feedbackmonk. No Gitea / Forge / external-issue-tracker dependency.

**Why drop the Forge bridge** (GitCellar uses Gitea-as-roadmap, but that's GitCellar-specific reuse):
- Customers won't run Gitea forks — no reasonable expectation to ask them to.
- DEC-FBR-03 multi-tenancy doesn't fit Gitea's single-tenant data model.
- DEC-FBR-02 privacy positioning requires controlling data flow — adding Gitea multiplies attack surface, audit complexity, deployment overhead.
- We're building an admin UI anyway; adding roadmap views is small incremental cost.
- GitCellar's Forge bridge was opportunistic (Forge already existed), not architectural rightness.

**Data model**:
```
roadmap_items     (id, tenant_id, project_id, title, body, status, author_id, created_at, updated_at)
roadmap_votes     (roadmap_item_id, voter_id, voted_at)   PK (item, voter)
roadmap_promotes  (feedback_id, roadmap_item_id, promoted_by, promoted_at)
```

**Status enum** (matches GitCellar's FB-ROADMAP-02 proven UX): `considering` / `planned` / `in-progress` / `shipped` / `wontfix`.

**Voting**: 1 vote per `(item, voter)`. `voter_id` is JWT `sub` for authenticated mode (DEC-FBR-04 mode a) or hashed cookie+IP for anonymous mode (mode b). Top-voted endpoint with 60s cache (port of GitCellar's `roadmap_voting.rs` algorithm).

**Public browse**: anonymous by default at `feedbackmonk.com/{tenant}/{project}/roadmap` (custom domain in $29+ tier). Browse without auth; auth required to vote (per Q4 modes).

**Promote-from-feedback**: admin clicks "Promote to roadmap" on a feature_request. Creates `roadmap_items` row, links via `roadmap_promotes`, transitions source feedback to `duplicate`. **Q24 privacy invariant carries over from GitCellar (load-bearing)**: rendered roadmap-item body contains the feedback message verbatim with NO submitter attribution and NO feedback ID reference. Inline test asserts byte-for-byte.

**Port from GitCellar's `roadmap_*` files**:
- ✅ Port: status state machine + labels (re-implement natively), voting aggregator + 60s cache, promote workflow, Q24 invariant
- ❌ Drop: `roadmap_bootstrap.rs` (no Gitea repo to bootstrap), `roadmap_issues.rs` (no Gitea REST client), Forge webhook plumbing, Gitea reactions API integration

---

### DEC-FBR-07: Repository home — New public GitHub repo, peer to GitCellar/quiqpic/SessionHelm

**Resolved**: 2026-05-13.

**Local working directory**: `E:\Developer\SourceControlled\Apps\feedbackmonk` (peer to GitCellar, quiqpic, SessionHelm; originally `Apps\Feedbackr` per DEC-FBR-07, renamed 2026-05-14 per PF-RENAME-02).

**Primary remote**: public GitHub repo at `github.com/<handle>/feedbackr` (or `github.com/feedbackr/feedbackr` if pre-registering the org). Stars, issues, PRs, Show HN posts live here.

**Optional local Gitea mirror**: standard backup workflow per user's machine setup; does NOT replace the public GitHub remote.

**Cloudflare Pages / Workers deploys** point at the GitHub repo (for `feedbackmonk.com` landing site).

**Why a new public repo and NOT in-place extraction in GitCellar's workspace**:
- **AGPL only has business value if visible.** GitCellar's repo is private (local Gitea). Embedding feedbackmonk there means nobody can find it, star it, fork it, audit it — killing the OSS-as-marketing channel that DEC-FBR-05's revenue math depends on.
- **No source-level dependency** — GitCellar consumes feedbackmonk via API + widget (DEC-FBR-04 mode a JWT), NOT via Rust-crate imports. The "library in Shared/" instinct from intake assumed a Rust-level dependency; that disappears with the API-only consumption model.
- **License hygiene** — GitCellar isn't AGPL. Mixing licenses in one repo is messy and confusing for contributors.
- **Release-cadence independence** — GitCellar's pre-launch hardening shouldn't gate feedbackmonk's release pace. Separate repos = each evolves at its own pace.

**GitCellar's role flips** from "host of feedback module" to "feedbackmonk's customer #1":
- GitCellar's internal `gitcellar-cloud/src/feedback/` keeps running unchanged until feedbackmonk v1 ships.
- GitCellar embeds feedbackmonk's widget (via DEC-FBR-04 mode a JWT) as customer-side validation — runs both internal feedback and embedded feedbackmonk in parallel during transition.
- Eventually GitCellar exports historical feedback → imports into feedbackmonk via admin API → removes internal feedback code.
- quiqpic + SessionHelm onboard post-GitCellar-launch or in parallel (simpler integrations than GitCellar's).

**Pre-registration recommendation**: lock `github.com/feedbackr` org and `feedbackr.com` domain now (cost: ~$10/yr for domain; org is free). Prevents squatters; defer rename if final product name differs (Q9).

**Spec migration plan**:
- During spec session: spec stays at `docs/specs/feedbackr/` in this GitCellar working dir (don't halt the session to switch directories).
- Post-spec, pre-implementation: move `docs/specs/feedbackr/` → `E:\Developer\SourceControlled\Apps\feedbackmonk\docs\specs\` (canonical home; the directory existed as `Apps\Feedbackr` at the time the migration step executed; renamed 2026-05-14 per PF-RENAME-02). Leave a breadcrumb pointer in GitCellar's `docs/specs/feedbackr/README.md`.

---

### DEC-FBR-08: MVP scope — 18-item IN list across 5 phases, ~12 weeks FTE to public launch

**Resolved**: 2026-05-13.

**v1 IN scope** (18 items must ship to launch):

| # | Capability | Port from GitCellar? |
|---|---|---|
| 1 | Multi-tenant data model (tenants + projects + RLS) | New |
| 2 | Customer signup + onboarding flow | New |
| 3 | Submission API (`POST /api/v1/feedback`) with JWT verify | Port + multi-tenant |
| 4 | Embeddable widget (JS/HTML, < 30KB, themed, accessible) | NEW |
| 5 | JWT verification (EdDSA, per-project keys, rotation) | New |
| 6 | Anonymous mode (IP+cookie dedup, rate limits) | New |
| 7 | Admin UI: list + drawer + reply composer (public/internal) | Port (Phase 1 core) |
| 8 | Status workflow (6-state + audit history) | Port |
| 9 | Status emails (confirmation, change, reply) — FB-1234 ID style | Port + parameterize |
| 10 | PII scrubber (canonical 20-pattern set) | Port from `gitcellar-service` |
| 11 | Public roadmap page (anon browse, auth vote) | Port logic, drop Gitea |
| 12 | Promote-to-roadmap + Q24 privacy invariant | Port + native impl |
| 13 | Roadmap voting + top-voted aggregator (60s cache) | Port |
| 14 | Tier enforcement (projects + volume caps, free-tier footer) | New |
| 15 | Billing (Polar integration, $9/$29/$79 + free) | Port pattern |
| 16 | Marketing site + landing (feedbackmonk.com) | NEW (Astro, like GitCellar landing) |
| 17 | Self-host distribution (`docker compose up`, env config) | NEW |
| 18 | Health/observability (`/health`, structured logs, error rates) | Port |

**v1 OUT of scope (defer to v1.1+)**:
- Attachments (storage backend needs re-design)
- Crash reporting / Glitchtip integration (huge scope; PII scrubber stays for future)
- Magic-link auth (DEC-FBR-04 mode c — optional, not blocking)
- Webhook integrations (Slack/Linear/GitHub/Discord) — let demand drive prioritization
- Custom domains for $29+ tier — wire up post-launch
- Email digest cadences (port from GitCellar's `digest_worker`)
- Full-text search across feedback (list + filters in v1)
- Reply via incoming email parsing
- i18n / multi-language (English-only v1, Plausible took same path)
- Roadmap kanban board view in admin (list view sufficient v1)
- Privacy mode E2E "sensitive feedback" toggle — real crypto work, deferred to v2

**v1 explicit NON-goals** (rule out forever or multi-year):
- ❌ SSO / SAML (Persona C — ruled out by DEC-FBR-01)
- ❌ Audit-log compliance reporting (Persona C)
- ❌ RBAC / role-based admin permissions (Persona C; single-tier admin v1)
- ❌ Customer-impact scoring / revenue weighting (Persona B)
- ❌ AI categorization / sentiment analysis / LLM features (DEC-FBR-02)
- ❌ "Replace your support inbox" framing (DEC-FBR-02)
- ❌ Cross-project feedback merging

**5-phase MVP shape** (informs `/0-uldf-ldis-plan`):

| Phase | Items | Output | Duration FTE |
|---|---|---|---|
| **P0 — foundation** | 1, 2, 3, 5, 6, 18 | Tenant signup → first project → POST feedback works | ~2 weeks |
| **P1 — closes the loop** | 7, 8, 9, 10 | Admin UI + status transitions + emails + PII scrubber active | ~3 weeks |
| **P2 — customer-facing** | 4, 11, 12, 13 | Widget shipped + public roadmap + voting + promote | ~3 weeks |
| **P3 — commercial** | 14, 15 | Tier enforcement + Polar billing live | ~2 weeks |
| **P4 — go-public** | 16, 17 | Marketing site + self-host docker + Show HN ready | ~2 weeks |

**Total ~12 weeks FTE** spec-ready → public launch. Calendar time depends on parallel commitments (GitCellar pre-launch hardening will likely 2× the FTE → ~6 months calendar).

**Surfaced concerns to acknowledge**:
1. 12 weeks FTE is real — context-switching with GitCellar is the dominant calendar-time risk.
2. The widget is the most novel piece (GitCellar had no widget — its feedback was inline in Desktop). UX iteration time should be budgeted generously.
3. Marketing site quality matters more than usual for OSS+SaaS positioning — Plausible's site sets a high bar; plan for real design work.
4. Self-host Docker distribution has hidden production-readiness work (env-var ergonomics, migrations, backup docs, etc.).
5. P0-P1 will surface learning that changes P3-P4 — `/0-uldf-ldis-plan` should leave room for replanning.

---

### DEC-FBR-09: Product name — "feedbackmonk" as working name; real branding pass at P4 (pre-launch)

**Resolved**: 2026-05-13.

**Working name**: "feedbackmonk". Used throughout spec, plan, implementation phases P0-P3.

**Real branding pass**: scheduled for **P4** (marketing site + go-public phase). Logo, color, font, voice all done together with the landing site. If a better name surfaces during brand work, rename then — costs are low pre-launch.

**Pre-registration tasks (zero/near-zero cost, do early)**:
- Check `github.com/feedbackr` org availability — register if available (free).
- Check WHOIS on `feedbackr.com`, `feedbackr.io`, `feedbackr.app`, `feedbackr.dev` — register the first available `.com`/`.app`/`.dev` (~$10-15/yr). If `.com` is squatted, signal to consider a different name rather than collecting hodgepodge TLDs.
- If both org AND domain are squatted: decide a different working name BEFORE P0 starts. Candidates worth investigating (unverified): Earshot, Plumbline, Listenly.

**Why workmanlike naming, not artistic**: matches founder's existing portfolio (GitCellar, quiqpic, SessionHelm — all clear, none precious). "feedbackmonk" fits the vibe; signals what it is; in the Flickr/Twittr lineage that's slightly dated but harmless for a dev tool.

**Don't optimize naming now** — spend the energy on the product. If brand work surfaces a clearly-better name at P4, rename then.

---

### DEC-FBR-10: Launch posture — Three-stage gradient: dogfood alpha → public AGPL beta → marketed launch

**Resolved**: 2026-05-13.

**Stage 1 — Dogfood alpha** (~2 weeks): triggered at end of P3 (commercial gate works). Audience: you. GitCellar embeds feedbackmonk's widget; you triage your own feedback through it. Goal: find UX bugs only real usage surfaces. Rapid iteration cadence.

**Stage 2 — Public AGPL beta** (~1-2 months): triggered at end of P4 (marketing site ready). Action: Show HN post + Twitter thread + GitHub repo public. Free tier open; paid tiers visible but NOT marketed. Goal: 100 free-tier signups, 5-10 self-host installs, qualitative widget UX feedback.

**Stage 3 — Marketed launch**: triggered after Stage 2 stabilization. Action: paid Twitter/X/HN, dev-community sponsorships, conf talks if relevant, GitCellar user-base co-promotion. Goal: first paying customers, recurring revenue.

**Coordination with GitCellar**:
- ⚠️ **Stage 3 MUST wait for GitCellar 1.0 to ship**. Running two cold-start marketing motions in parallel splits founder bandwidth in the worst way.
- ✅ Stages 1-2 can overlap with GitCellar pre-launch hardening — they're low-marketing-volume.
- ✅ GitCellar 1.0 → feedbackmonk Stage 3 as a coordinated launch arc is a real win (Desktop users see feedbackmonk; feedbackmonk roadmap hosted by GitCellar; cross-reference organic).

**Anti-patterns ruled out**:
- ❌ "Stealth mode" → public-launch big bang. Rarely works for indie OSS; build in public quietly.
- ❌ Show HN before widget polished. One bad Show HN damages brand for years.
- ❌ Paid ads pre-Stage 3. Wasted spend before product-market signal.

---

### DEC-FBR-11: Working name changed to "feedbackmonk" — DEC-FBR-09 squat-contingency enacted

**Resolved**: 2026-05-14 (post-DEC-FBR-09 enactment, mid-P1 Stage 2 close).

**Trigger**: pre-public-commit availability scan (run from a fresh planning-completion session before the Stage 2→Stage 3 boundary) found:
- `github.com/Feedbackr` org **TAKEN** (dormant since 2024-05-20, owner `b.invisibilities@outlook.com`, blog claims `feedbackr.live`)
- `feedbackr.com` **TAKEN** (Verisign authoritative; registrar Namecheap; "client transfer prohibited")
- `feedbackr.app` and `feedbackr.dev` AVAILABLE

DEC-FBR-09's contingency activated: *"If both org AND domain are squatted: decide a different working name BEFORE P0 starts."* The work had already moved past the literal pre-P0 deadline, but the contingency principle still applies for any pre-public-commit moment — the rename must land before any public push references the name.

DEC-FBR-09's three suggested-candidate names (Earshot / Plumbline / Listenly) all blocked at `.com` on rescan. A second-batch brainstorm (8 candidates including compounds matching the founder's portfolio pattern) and a third-batch user-proposed set (`glitchjuggle`, `glitchjuggler`, `bugglitch`, `feedbackmonk`, `feedbackamole`, `gnufeedback`) were evaluated. **`feedbackmonk`** chosen.

**Why `feedbackmonk`**:
- Both `github.com/feedbackmonk` and `feedbackmonk.com` confirmed open (RDAP HTTP 404 + `gh api orgs/feedbackmonk` HTTP 404).
- Strongest alignment with DEC-FBR-02 brand promise *"Privacy-first product feedback. Hear your users without spying on them."* — "monk" semantically reinforces *quiet, disciplined craft, listening* without amending DEC-FBR-02.
- Dev-tool register matches founder portfolio (GitCellar, quiqpic, SessionHelm — workmanlike, none precious).
- Clean spell-out and pronunciation (vs. `feedbackamole`, the clever whack-a-mole alternative, whose pronunciation was ambiguous in spell-out tests).
- No trademark / political baggage (vs. `gnufeedback`, where FSF's GNU mark carries risk).
- Avoids bug-tracker miscategorization (vs. `glitchjuggle` / `bugglitch`, which read as Sentry/Bugsnag competitors — wrong market).

**Identifier-stability rule**: existing decision IDs `DEC-FBR-01..11` and requirement IDs `FR-FBR-01..18` **keep the `FBR` prefix permanently**. ID prefixes outlive renames in mature codebases (e.g., GitHub's `gh_` URL stem is durable across any future GitHub rebrand). From this point forward `FBR` is a stable identifier-prefix divorced from the brand. No bulk-rename of IDs.

**Identity-rename scope (executed in the session that recorded this decision)**:
- `CLAUDE.md`, `README.md`, `.claude/project.json` (name + description fields)
- Spec front-matter and §"What X is" sections in `SPECIFICATION.md`
- This decision entry; OPEN_QUESTIONS.md Q9 status note
- `LICENSE` body left as-is (canonical 661-line AGPL-3.0 text is name-independent)

**Identity-rename scope (DEFERRED — tracked as Pending Follow-Up in CLAUDE.md)**:
- Cargo crate prefixes `feedbackr-*` → `feedbackmonk-*` (workspace + member crate names + `Cargo.toml [dependencies]` entries)
- Env-var prefix `FEEDBACKR_*` → `FEEDBACKMONK_*` in code + docs + `.env.example`
- Postgres schema items if any are `feedbackr_*`-prefixed (audit at rename time)
- `admin-ui/package.json` name field
- `cargo sqlx prepare` cache regeneration after env-var rename
- Working directory `E:\Developer\SourceControlled\Apps\Feedbackr` → `\feedbackmonk` (the agent recording this decision cannot rename its own CWD; live PODS sibling sessions also have CWD-locked terminals; must be done by user after autopilot chain reaches quiescence)
- Future git remote URL: `github.com/feedbackmonk/feedbackmonk` (no remote currently set; user-action when pre-registering the org)

**Why defer the code-level rename**: the live PODS workers (CLAUDE-A backend + CLAUDE-B frontend) and the LD session are mid-arc on P1. A 50+-file rename committed during their flight creates merge friction and forces them to re-resolve sqlx compile-time checks. The natural quiescent window is the P1 finalize → P2 plan transition; the rename becomes a single atomic commit there.

**Brand pass at P4 unchanged**: DEC-FBR-09's scheduling of the FULL branding pass (logo, color, font, voice, possible re-rename) for P4 stands. DEC-FBR-11 is the WORKING-name swap pulled forward by the squat contingency. If P4 surfaces a clearly-better name, rename then per DEC-FBR-09.

---

## Spec session — COMPLETE ✅

All 10 critical questions resolved. Foundational triad (Q1-Q3) + 7 next-tier (Q4-Q10) closed. 10 decisions (DEC-FBR-01..10) plus 1 contingency amendment (DEC-FBR-11) recorded. 18 functional requirements derive from DEC-FBR-08. Ready for `/0-uldf-ldis-plan`.

---

## Implementation-Discovered Decisions (P0 Stage 1)

These decisions were ratified during P0 Stage 1 implementation. They refine but do not contradict DEC-FBR-01..10. Each is recorded here for permanent traceability beyond the development-complete report.

### DEC-FBR-IMPL-01: Contract C1 extensions — `FeedbackRepo::submit_*` carry `kind`; `list_recent` is part of the trait

**Resolved**: 2026-05-13 (P0 Stage 1).

**Decision**: The frozen Contract C1 repository surface includes two extensions beyond the plan's literal §C1 sketch:

1. **`FeedbackRepo::submit_authenticated` and `submit_anonymous` accept an explicit `kind: FeedbackKind` parameter.**
2. **`FeedbackRepo::list_recent(scope, limit)` is part of the trait** (used by 3/4 feedback tests as round-trip read path; consumed by Stage 2 Worker A's forthcoming admin-feedback-list endpoint).

**Rationale**: The schema declares `feedback.kind` with a `CHECK` constraint, and FR-FBR-03 Contract C3 accepts an optional `kind` field. Defaulting `kind` at the DB layer would push a fundamental piece of feedback metadata through the wrong seam (HTTP handlers would have to omit it, forcing schema-default semantics into the type system). Adding `list_recent` to the trait avoids forcing 3 of 4 unit tests to use raw SQL — which the multi-tenant-isolation oracle forbids on principle.

Both additions are **EXTENSIONS** (additional info / additional method), not **WIDENINGS** (the `&ProjectScope` first-arg discipline is preserved on every method).

**Trade-offs**: Mild departure from the plan's literal §C1 sketch. If a future Contract C1 amendment ratifies the other direction, both are trivially removable (one parameter removal + one method removal).

**Implementation**: `crates/feedbackmonk-repository/src/feedback.rs` trait definitions. Reflected in `docs/planning/handoffs/stage1-to-stage2.md` as the frozen surface for Stage 2 workers.

---

### DEC-FBR-IMPL-02: `TenantRepo::scope_for(Uuid)` allow-listed as a third pre-auth method

**Resolved**: 2026-05-13 (P0 Stage 1).

**Decision**: `TenantRepo::scope_for(uuid) -> Result<TenantScope>` is the third allow-listed pre-auth method (joining `create` and `find_by_email`). It bridges a verified session-cookie tenant_id to a fresh `TenantScope`. Recorded in `.claude/oracles/multi-tenant-isolation-check/allowlist.toml` with inline rationale.

**Rationale**: The pre-authentication boundary necessarily mints the **first** `TenantScope` from a verified caller. `TenantScope::new` is `pub(crate)`, so without `scope_for`, Stage 2 Worker A's login handler has no path from "I've validated this session cookie" to "...therefore here is a `TenantScope` for downstream calls." Making the boundary explicit — and gating it through a single named method with a documented rationale — is more honest than back-channels.

**Trade-offs**: Adds a third entry to the pre-auth allowlist. Risk is gradual allowlist growth. Mitigated by required-rationale convention and oracle freshness trigger on allowlist edits.

**Implementation**: `crates/feedbackmonk-repository/src/tenants.rs`. Allowlist entry: `.claude/oracles/multi-tenant-isolation-check/allowlist.toml` lines 32-35.

---

### DEC-FBR-IMPL-03: Oracle canonical implementation in Python 3.8+

**Resolved**: 2026-05-13 (P0 Stage 1, oracle build).

**Decision**: The `multi-tenant-isolation-check` oracle's canonical implementation is `oracle.py` (Python 3.8+). `oracle.ps1` and `oracle.sh` are thin shims that delegate to `python3 oracle.py`. This pattern is the recommended default for future feedbackmonk Verification Oracles that need non-trivial parsing.

**Rationale**: Probe B requires balanced-paren multi-line Rust signature parsing with context tracking. The initial bash port produced 25 false positives on a clean tree due to POSIX shell's context-tracking limitations. A false-positive oracle silently degrades to no-oracle (trained-to-ignore) within weeks. Python 3.8+ is ubiquitous on CI Ubuntu and developer machines; the dependency cost is real but small (and bounded — no `pip install` required for stdlib-only oracles).

**Trade-offs**: Adds Python to the oracle dependency set. Documented in oracle file headers and `.github/workflows/ci.yml` (which installs Python if absent). Some oracles that need only simple grep stay in pure shell — the pattern is "Python when parsing crosses lines or needs context."

**Implementation**: `.claude/oracles/multi-tenant-isolation-check/{oracle.py, oracle.ps1, oracle.sh}`. Shims verified to produce identical output on clean tree (PASS) and on a planted violation (FAIL with same offender line).

---

### DEC-FBR-IMPL-04: Dev Postgres on port 5433 (not 5432) to deconflict with gitcellar-cloud

**Resolved**: 2026-05-13 (P0 Stage 1, dev-environment setup).

**Decision**: feedbackmonk's local-dev Postgres container binds **port 5433**, not the Postgres default of 5432.

**Rationale**: The peer gitcellar-cloud repo already runs a Postgres container on 5432 on this development machine. A clash would either prevent both containers from running simultaneously OR — worse — silently write feedbackmonk test data into gitcellar's database. The 5433 choice preserves project isolation and matches the Dev Port Registry convention (each project gets its own port range; see `~/.claude/MACHINE_CONFIG.md`).

**Trade-offs**: Developers need to remember `localhost:5433` for feedbackmonk. Documented in `docs/operations/LOCAL_DEV.md` and the `DATABASE_URL` env vars used by `sqlx::test`.

**Implementation**: `docs/operations/LOCAL_DEV.md` documents the container shape. `~/.claude/MACHINE_CONFIG.md` records the port claim. `sqlx::test` macros consume `DATABASE_URL=postgres://postgres:dev@localhost:5433/feedbackmonk_dev`.

---

### DEC-FBR-DEFER-01: Polar billing deferred from P3

**Resolved**: 2026-05-14 (P3 Stage 1 planning, ratified by user direction).

**Decision**: FR-FBR-15 (Polar billing integration) is **deferred** from P3's commercial-gate phase. P3 ships the tier model + cap enforcement + free-tier footer + admin tier-status endpoint per FR-FBR-14, but does NOT ship a Polar webhook receiver, customer/subscription schema columns, or a self-service upgrade flow. Stage 2's admin "Upgrade" button is a stub reading *"Contact support to upgrade"*. Operators promote tenants between tiers via the SQL helper in `docs/operations/TIER_OVERRIDE.md` until Polar lands.

The deferred Polar contract — webhook envelope, event → tier mapping, schema migration shape, GitCellar port pointers — is captured in `docs/deferred/polar-integration.md` so a future worker can implement without re-deriving.

**Rationale**: Per user direction during P3 planning: *"we just don't need to set up billing yet for consumers"*. The founder is dogfooding their own feedbackmonk instance via the `self_host` tier override and has no public-paying-customer pressure on P3's exit gate. Decoupling tier enforcement (load-bearing for P4 launch readiness — the free-tier footer is the brand-promise surface) from the consumer-billing flow (NOT load-bearing until consumer GTM motion exists) lets P3 close on the commercial-gate **mechanism** without paying the Polar integration cost up front. The arc plan's original P3 exit gate ("Polar webhook → tier flip end-to-end on Polar sandbox") is **relaxed** to: tier caps fire correctly + footer tier-flip works + oracle GREEN + admin UI displays current tier and usage.

**Trade-offs**: Two concrete: (1) The Stage 2 "Upgrade" stub button is a deliberate user-experience seam — anyone clicking it gets pointed at email, not checkout. Documented at `docs/deferred/polar-integration.md` so the seam is intentional, not stale. (2) Self-service tier downgrade/upgrade requires an operator-in-the-loop until Polar lands; manageable at the dogfood scale (≤10 tenants expected through P4). When consumer-billing pressure arrives, the worker reads `docs/deferred/polar-integration.md` and ports from `gitcellar-cloud/src/billing/polar.rs` (per DEC-FBR-07 read-only reference convention).

**Implementation**: `docs/deferred/polar-integration.md` (deferred-work stub); `docs/operations/TIER_OVERRIDE.md` (interim operator workflow); P3 Stage 1 ships everything in `docs/planning/plans/20260514T134816-feedbackmonk-p3-commercial-gate.md` §Stage 1 minus FR-FBR-15.

---

### DEC-FBR-IMPL-05: P4 marketing-site pricing single-source-of-truth — build-time Rust→JSON export

**Resolved**: 2026-05-14 (P4 Stage 1).

**Decision**: The marketing site's `/pricing` page derives its tier caps and tier-label strings from a **build-time JSON export** of `feedbackmonk-core::tier::tier_quotas()` (Contract C19), NOT from hand-typed pricing constants in Astro/MDX/TypeScript. Implementation: Stage 2 Worker A adds a thin Rust binary (a new `examples/` target in `crates/feedbackmonk-core/` or a tiny new crate `crates/feedbackmonk-marketing-export/`) that prints `tier_quotas()` as JSON to stdout, plus a `marketing/scripts/export-tier-quotas.{sh,ps1}` shim that runs the binary and writes the output to `marketing/src/data/tier_quotas.json`. Astro's `prebuild` npm-script invokes the shim. The `/pricing` page imports the JSON at build time.

**Rationale**: Option-A vs Option-B was scoped at P4 Stage 1 (per `docs/planning/plans/20260514T163356-feedbackmonk-p4-go-public.md` Decision-1):

- **Option A — build-step export (chosen)**: pricing drift is **structurally impossible**. Astro cannot import stale data because the JSON IS the export. The `tier-enforcement-status` Verification Oracle (built P3 Stage 1) already defends `tier_quotas()` against Contract C19 drift via its Probe B; combining that with this build-step makes site↔code parity transitively verified.
- **Option B — hand-typed pricing + `marketing-pricing-parity` Verification Oracle**: drift is detected after the fact, not prevented. Worse: parity oracle has Q5 drift-detection risk if it's not rebuilt on Astro-side changes (operator forgets `npm run prebuild` after editing copy → silent drift until next CI).

Option A's cost (one small Rust binary + one shim script + one `prebuild` npm-script entry) is bounded and one-shot. The Astro-build pipeline already needs node and ts; adding `cargo run` is one tool further but the build host is already a developer machine with `cargo` (or a CI runner with `cargo` for the Rust workspace anyway).

**Trade-offs**: (1) Marketing-site Astro build now depends on `cargo` being available in the same environment. Documented in `marketing/README.md`. CI's marketing job needs `cargo` if the marketing site is built in CI. Acceptable — every existing CI job for this repo has `cargo`. (2) Hot-reloading the pricing data during Astro dev requires re-running the shim; acceptable since `tier_quotas()` rarely changes (the const fn predates P4 and was frozen in P3). (3) Doesn't extend to other code↔site invariants (e.g., feature-matrix); each invariant decides its own SSOT approach. Decision applies ONLY to pricing tier caps/labels.

**Implementation**: Worker A's Task Zero in P4 Stage 2. Worker A authors the new Rust binary + shim + Astro prebuild wiring + the `/pricing` page importing `tier_quotas.json`. The `marketing-pricing-parity` Verification Oracle candidate from the P4 plan is **withdrawn** — Option A makes it redundant.

---

### DEC-FBR-IMPL-06: P4 `selfhost-compose-smoke` Verification Oracle — three-probe, build-at-Task-Zero of Stage 2 Worker B

**Resolved**: 2026-05-14 (P4 Stage 1).

**Decision**: P4 ships a new Verification Oracle `selfhost-compose-smoke` defending FR-FBR-17 self-host distribution as a code-level invariant. Three probes:

- **Probe A (fast, always-on)**: `deploy/docker/docker-compose.yml` exists and parses (yaml-lint or `docker compose config` --quiet equivalent). Catches yaml-syntax breakage without spinning up containers.
- **Probe B (fast, always-on)**: every `${FEEDBACKMONK_*}` and `${DATABASE_URL}` reference in `deploy/docker/docker-compose.yml`'s `environment:` blocks is present in `docs/operations/SELFHOST_ENV.md`'s canonical catalog table (parses the table, extracts var names, set-compares against compose-env references). Catches typos, undocumented additions, schema drift.
- **Probe C (`--full`, opt-in)**: `docker compose down --volumes && docker compose up -d && wait-for-healthy && curl http://localhost:14304/health` returns 200 with the documented JSON body. Clean-state smoke — catches "works only because volume is stale" and "works only because image is cached" failure modes that ate two real cycles of GitCellar's own self-host bring-up.

Built as `.claude/oracles/selfhost-compose-smoke/` with the established Python canonical + bash/ps1 shims pattern (DEC-FBR-IMPL-03).

**Rationale**: P4 Stage 1's Testability Gate scored FR-FBR-17 at composite ~14 (Q1=4 iteration cost, Q2=4 fidelity risk — clean-state-vs-stale-state is the canonical docker fidelity gap; Q3=4 critical path for the P4 exit gate). The composite-12+ threshold AND the Q3-Q4 combination both flag scaffolding-leverage; the `selfhost-compose-smoke` oracle is the scaffolding. Building it as Worker B's Task Zero locks the verification surface in before main implementation, mirroring the P3 Stage 1 Task Zero pattern for `tier-enforcement-status`.

**Probe-C gating**: Probe C requires a docker daemon and is heavy (~30-60 seconds per run). Like `tier-enforcement-status` Probe C (`--full` integration smoke trio), it's opt-in for CI / convergence / pre-release sweeps, NOT every-commit. `--full` flag pattern matches the existing oracle convention.

**Trade-offs**: (1) Probe C is OS-dependent — Windows / macOS dev machines need Docker Desktop running; Linux servers run docker natively. Documented in oracle README. (2) Probe B's catalog-parser is fragile against `SELFHOST_ENV.md` formatting changes; mitigation: Probe B uses a documented table-extraction pattern (start anchor, end anchor, column position) tested against the file frozen at Stage 1. If the table format changes, the oracle's parser updates. (3) Two probes (A and B) are cheap and always-on; the failure mode is "I ran the oracle, both quick probes passed, I assumed Probe C was also fine" — countered by `/0-uldf-finalize` Phase 1.5 calling out which probes ran vs `--full` skipped.

**Implementation**: Worker B's Task Zero in P4 Stage 2. The `marketing-pricing-parity` Verification Oracle candidate (alternate path under DEC-FBR-IMPL-05 Option B) is NOT built since DEC-FBR-IMPL-05 chose Option A.

---

### DEC-FBR-IMPL-07: `FEEDBACKMONK_BIND_ADDR` env var — api binary bind-address configurability (DEC-PODS-B-01 ratified)

**Resolved**: 2026-05-14 (P4 Stage 2 — surfaced by `selfhost-compose-smoke` Probe C during PODS session `collab-20260514-170323`).

**Context**: FR-FBR-17 self-host blocker surfaced during P4 Stage 2 Probe C `--full` verification. The api binary at `crates/feedbackmonk-api/src/main.rs:71` was hard-coded to bind `[127, 0, 0, 1]`. Inside the api container this passes the local healthcheck (curl localhost:14304 inside the container) but the admin-ui edge container (separate IP on the docker bridge, e.g. 172.20.0.4 → api at 172.20.0.3) gets `Connection refused`. The nginx reverse-proxy to `http://api:14304` returns 502 to operators. Without this fix, the B2 topology (separate admin-ui nginx edge) cannot work AND a B1 topology (api serves admin-ui via ServeDir) would still fail external healthchecks.

**Decision**: Add a new optional env var `FEEDBACKMONK_BIND_ADDR` (default `127.0.0.1`) controlling the IP address the api binary binds to. Docker-compose sets it to `0.0.0.0` so containers on the docker bridge can reach the api.

**Scope** (minimal-additive):
- `crates/feedbackmonk-api/src/main.rs` — adds env-reader with `127.0.0.1` default. ~10 LOC added. No existing handler/route/error/test changed.
- `docs/operations/SELFHOST_ENV.md` (C21 catalog) — appends one row in the HTTP Binding section, alphabetically near `FEEDBACKMONK_PORT`. Catalog is grow-only; no existing rows touched. C21 grew from 18 → 19 entries.
- `deploy/docker/docker-compose.yml` — adds one `environment:` entry defaulting `0.0.0.0`. No existing env entry changed.
- `deploy/docker/.env.example` — adds commented row documenting the new var.

**Backwards compatibility**: optional env var with backwards-compatible default. Existing `cargo run` / dev / CI flows unaffected — nothing currently sets `FEEDBACKMONK_BIND_ADDR`, and the absent-env-var branch reads `127.0.0.1`, identical to the prior hard-coded literal.

**Witnesses**:
- Probe C `--full` GREEN end-to-end (`docker compose down -v && docker compose up -d --build --wait` succeeds; `curl /health/ready` 200 in <90s via admin-ui→api over docker bridge).
- `pii-scrub-audit` re-verified post-change — canonical hash unaffected (no `tracing_subscriber::*` surface touched).
- `multi-tenant-isolation-check`, `widget-bundle-size`, `tier-enforcement-status` regression-checked GREEN.

**Self-mediation provenance**: surfaced + authored by CLAUDE-B during PODS session `collab-20260514-170323`. Pre-authorized per session `GUIDE.md §8` row "Worker B: SELFHOST_ENV.md appends" — *"If compose authoring surfaces a missed env var, B may APPEND a row to the C21 catalog and reference it in compose. Tagged self-mediated; LD ratifies at convergence. NEVER modify existing rows."* Ratified by LD at 2026-05-14T18:18:00Z (channels/decisions.md DEC-PODS-B-01).

**Rollback**: single `git revert` removes all four touched files cleanly. Three files implicated (main.rs +10 LOC, SELFHOST_ENV.md +1 row, docker-compose.yml +1 env entry, deploy/docker/.env.example +1 commented row). No DB/migration/contract surface implicated.

**Alternatives considered**:
- *Keep hard-coded literal* — blocks FR-FBR-17 self-host distribution.
- *Hard-code `0.0.0.0`* — broadens default attack surface for `cargo run` dev flows on multi-user dev machines; rejected.
- *Per-startup `LD_PRELOAD` override or similar* — significantly more invasive; rejected.

---

