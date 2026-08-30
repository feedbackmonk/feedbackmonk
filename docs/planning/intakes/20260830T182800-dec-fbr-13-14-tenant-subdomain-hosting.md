# Intake Assessment

**Source**: /0-uldf-ldis-intake
**Generated**: 2026-08-30T18:28:00Z
**Task**: DEFER-005 — implement DEC-FBR-13 (tenant-subdomain public-surface shape + custom domain as
the paid upgrade) and DEC-FBR-14 (first-party products become SaaS tenants), injected from GitCellar.
**Brief**: `docs/planning/deferred/DEFER-005_tenant-subdomain-hosting-shape.md`
**Autonomy**: `autopilot` (owner-authorised for spec, planning and implementation in the inject mandate)

---

```
═══════════════════════════════════════════════════════════════
       LEAD DEVELOPER INTELLIGENCE ASSESSMENT
═══════════════════════════════════════════════════════════════

TASK: Build the hosting shape DEC-FBR-13/14 already decided — host-based tenant
      resolution, {tenant}.feedbackmonk.com as the public default, customer
      custom domains (board + widget/API) as the tier-gated paid upgrade, admin
      pinned to one feedbackmonk-owned host; then move GitCellar from a
      self-hosted single-tenant instance onto that SaaS as a tenant.

───────────────────────────────────────────────────────────────
PERCEPTION
───────────────────────────────────────────────────────────────

Type: Enhancement → Capability Extension, with a Migration (platform) tail
Scope: LARGE  (multi-stage; the brief's own estimate `multi-session` holds)
Risk: HIGH — it introduces a NEW tenant-resolution axis (the Host header)
      alongside the existing project_id-in-path axis. Two axes that can
      disagree is exactly the shape multi-tenant leaks take.

Professional Assessment:
The decision work is DONE and load-bearing — this is not a design lane, it is a
build lane with one genuine spec conflict to reconcile (below). The dangerous
part is not "add a subdomain"; it is that adding host-resolution WITHOUT binding
the existing path routes to the resolved host would ship the illusion of origin
isolation: a.feedbackmonk.com/api/v1/projects/{B's project}/board would still
render tenant B's user-generated content on tenant A's origin, and DEC-FBR-13's
reason #1 — the decisive argument — would be silently unmet. **Host binding, not
host routing, is the deliverable.**

───────────────────────────────────────────────────────────────
SPECIFICATION ANALYSIS
───────────────────────────────────────────────────────────────

Coverage: 9/11 dimensions specified (unusually high — two owner DECs, a per-surface
          table, rejected alternatives, success criteria and a file map all exist)
Gaps: 1 critical, 2 high, 3 assumable

Critical Gap — **DEC-FBR-13 and DEC-FBR-14 conflict on triage.gitcellar.com.**
  - DEC-FBR-13: admin/triage lives on exactly one feedbackmonk-owned host; custom
    domain for admin = "Never".
  - DEC-FBR-14: CNAME the existing GitCellar hosts — explicitly including
    triage.gitcellar.com — at the SaaS instance, with no user-visible change and
    no GitCellar source edit (TRIAGE_URL untouched).
  Both cannot hold if "CNAME at" means "serve the admin SPA there".
  → Resolved in-lane, NOT re-litigated: triage.gitcellar.com CNAMEs at the SaaS
    and is answered with a **301 to the canonical admin host**. Admin is *served*
    on one host only (DEC-FBR-13 honoured — no per-domain sessions, no SSO
    surface, no admin cookie on a customer domain); the existing link keeps
    working and TRIAGE_URL is never edited (DEC-FBR-14 honoured). Recorded as
    DEC-FBR-IMPL-27.

High Gap — **there is no tenant-level slug today.** `tenants` carries only
  `email`; only `projects` has a `slug`, and it is unique per-tenant, not
  globally. {tenant}.feedbackmonk.com needs a globally-unique, DNS-legal tenant
  label that does not exist in the schema. New column + backfill + reserved-label
  policy is unavoidable first work.

High Gap — **custom-domain TLS issuance is unspecified.** DEC-FBR-13 sells the
  CNAME; nothing says who issues the certificate. Resolved in-lane by putting
  ACME at the edge and giving it an authorisation seam in the API (Caddy
  on-demand-TLS `ask` endpoint) rather than building ACME into the Rust binary.
  Consequence: the app owns the *mapping and its authorisation*; the edge owns
  the *certificate*. That is the correct split and keeps the self-host story
  (FR-FBR-17) untouched for operators who want neither.

Assumable: default subdomain source (derived from the tenant's first project
  slug, owner-editable); reserved-label list contents; domain-claim uniqueness
  semantics.

───────────────────────────────────────────────────────────────
CALIBRATION
───────────────────────────────────────────────────────────────

Required Spec Level: standard (Enhancement on a mature codebase with frozen
  contracts; the DECs already supply the thorough-level material)
Current Spec Level: standard-plus at the decision layer, THIN at the
  implementation layer (no FR row, no contract, no schema)

VERDICT: **SUFFICIENT** — proceed to /0-uldf-ldis-spec to integrate two new
FR rows + contracts, then /0-uldf-ldis-plan. No blocking question for the
owner on the CODE half. One blocked item on the OPS half (below).

───────────────────────────────────────────────────────────────
ENGAGEMENT STRATEGY
───────────────────────────────────────────────────────────────

Questions: 0 asked (autopilot; brief pre-authorises spec+plan+implement and
  explicitly says the scope call is the lane's), 3 assumed, 1 deferred to owner
  as a BLOCKED ops item rather than a stall.

Assumptions documented:
  SAFE (95%+)
   A1. The existing /api/v1/projects/{project_id}/… public routes stay live and
       are NOT migrated away. Redirecting them would break every board link and
       every deployed widget, and DEC-FBR-13 reason #2 exists precisely to avoid
       breaking shared links. Host binding is added ON TOP of them.
   A2. Self-host (FR-FBR-17) deployments configure no root domain and therefore
       see byte-identical behaviour to today. Host binding is opt-in by
       configuration, and unconfigured ⇒ inert.
   A3. The widget needs no change: data-api-base already exists
       (widget/src/widget.ts:97), so pointing a customer's widget at their own
       endpoint is configuration, not code.

  REASONABLE (80%+)
   A4. One custom domain record serves BOTH the board and the widget/API for a
       tenant, distinguished by path, not by a second hostname. DEC-FBR-13's
       table lists them as two surfaces but names one CNAME mechanism, and
       feedback.gitcellar.com already serves the API on the shape a board would
       also live on.
   A5. Domain ownership is proven by the CNAME itself (you cannot point DNS at us
       for a domain you do not control) plus a tenant-scoped claim record with a
       UNIQUE constraint. A DNS-TXT pre-check is hardening, not v1.

  RISKY (needs validation, flagged not assumed)
   A6. Splitting the public board out of the admin bundle is NOT required for
       DEC-FBR-13 to be met — host separation already puts the board on a
       different ORIGIN from the admin console, which is the whole of reason #1.
       The split is bundle-hygiene, and is scoped as an optional later stage
       rather than smuggled into the critical path. **The ledger line's premise
       was re-checked and it holds: the board is still App.tsx routes at
       lines 51-67, still in the same bundle as Login.tsx.**

Deferred:
   D1. Custom email From: — DEC-FBR-13 defers it explicitly (needs DKIM
       delegation). Not in scope.
   D2. Per-tenant board SEO/sitemap under the new host shape.
   D3. Re-claim policy for a stale `pending` custom domain (squatting a domain
       you do not own to deny it to another tenant). Documented limitation.

───────────────────────────────────────────────────────────────
DECISION POINTS IDENTIFIED
───────────────────────────────────────────────────────────────

🛑 Blocking (owner/ops, NOT code):
   - Standing up an independent feedbackmonk.com deployment with wildcard DNS
     (*.feedbackmonk.com) and wildcard TLS. Needs a hosting account and DNS
     panel hands. **Surfaced as a blocked item with a recommendation; the code
     half does NOT wait on it.**
   - The feedback.gitcellar.com DNS cutover. Live GitCellar sessions exist on
     this machine; the brief says coordinate, do not act unilaterally. Runbook
     is delivered; the cutover is not executed.

⏸️ Accumulating (auto-decided in-lane, recorded as DEC-FBR-IMPL entries):
   - triage.gitcellar.com → 301 (the DEC-13/14 reconciliation above).
   - Tenant subdomain label rules + reserved list.
   - Edge-owns-ACME / app-owns-authorisation split.

ℹ️ Auto-decidable: repository/handler shapes, migration numbering, test layout.

───────────────────────────────────────────────────────────────
ORACLE CANDIDATES (Proactive Oraculurgy)
───────────────────────────────────────────────────────────────

Candidates:
- **host-tenant-binding** — "Can a request arriving on tenant A's host reach
    tenant B's data, and is the admin surface reachable on any host other than
    the canonical admin host?"
    Signal: this is the exact invariant DEC-FBR-13 reason #1 buys, it is
      NOT covered by multi-tenant-isolation-check (which polices the
      repository-scope axis, not the host axis), and it is the one thing that
      would fail silently — a missing guard produces correct-looking pages.
    Qualification: deterministic ✓ (parses the router + guard source and runs
      behavioural probes) · recurrent ✓ (every future public route can forget
      the guard, exactly as public-route-ceiling guards the rate-limit floor)
      · freshness-contractable ✓ · gracefully-absent ✓ (vacuous-PASS when no
      root domain is configured).
    Suggested build timing: **Task Zero — assertion block written FIRST**, per
      the brief's explicit instruction.
    Precedent to mirror: public-route-ceiling (a floor every public router
      must carry) + public-board-moderation-gate (detection-from-code, not a
      self-reported flag).

- custom-domain-tier-gate — folded INTO the oracle above as a probe rather than
    built separately; tier-enforcement-status Probe A already owns the
    "handler coverage of the tier predicate" shape and would duplicate it.

───────────────────────────────────────────────────────────────
COLLABORATION ASSESSMENT
───────────────────────────────────────────────────────────────

Scope: LARGE

Friction:
- Subdivisible (≥3 units, clean ownership): **NO** — the three plausible units
  (schema+repo, host guard, admin/edge) are strictly sequential: the guard needs
  the repo, the UI needs the guard, and every one of them touches the same
  build_app seam. Fan-out would produce merge contention on one file.
- Spec stable: YES (two resolved DECs).
- Coupling low: NO (see above).
Value: thin — no specialisation gain, no independent-discovery gain; wall-clock
  gain is eaten by the coordination cost on a single-file seam.

Verdict: **SEQUENTIAL** — one lane, staged.
Partitioning rung: **0 (Sequential)**.

Agent Teams Candidate: NO — this is write-heavy on a hot shared seam.

───────────────────────────────────────────────────────────────
RECOMMENDED NEXT STEPS
───────────────────────────────────────────────────────────────

1. /0-uldf-ldis-spec — integrate FR-FBR-32 (host-based tenant resolution +
   tenant subdomain) and FR-FBR-33 (custom domain as tier-gated paid upgrade)
   into docs/specs/SPECIFICATION.md; record DEC-FBR-IMPL-27/28/29; freeze
   Contracts C32 (host resolution), C33 (custom domain lifecycle).
2. /0-uldf-ldis-plan — stage the build: Task Zero oracle assertion →
   schema/repo → host guard + public site endpoint → tier-gated domain admin →
   admin-UI + edge → DEC-FBR-14 runbook.
3. Implement, sequenced DEC-FBR-13 before DEC-FBR-14 per the brief.
4. Surface the two ops-blocked items with recommendations; do NOT stall code on
   them and do NOT execute the GitCellar DNS cutover unilaterally.

═══════════════════════════════════════════════════════════════
```
