# Intake Assessment
**Source**: /0-uldf-ldis-intake
**Generated**: 2026-05-12T22:11:54
**Task**: Extract GitCellar's feedback system into a standalone SaaS product targetable at other apps/SaaS that want a user-feedback platform (submission, status workflow, public roadmap, crash reporting, admin triage).

---

## PERCEPTION

**Type**: **Hybrid — Architecture Migration + Greenfield Product Launch**
- Migration leg: decouple an in-tree, GitCellar-coupled module into a self-contained reusable core
- Greenfield leg: design, build, and launch a new SaaS product around that core (multi-tenancy, customer-facing surfaces, billing, branding, marketing)

**Scope**: **LARGE → ENTERPRISE** (multi-month, multi-component, solo-dev)

**Risk**: **HIGH**

### Professional Assessment

The reference implementation working inside GitCellar is a strong asset — the user is not starting from zero, they have proof of concept across 5 shipped phases (Reply Loop, Attachments, Forge Triage, Crash Reporting, Public Roadmap). The risk isn't "can it be built" — it's "can it be **decoupled** without destabilizing GitCellar's pre-launch hardening, AND positioned distinctly enough in a crowded feedback-tool market (Canny, Featurebase, Fider, Productboard, GitHub Issues+Discussions) to justify the effort." The market positioning is the missing piece, not the technical extraction.

---

## SPECIFICATION ANALYSIS

**Coverage**: 2/10 dimensions specified (Purpose partial; Behavior partial via existing GitCellar spec)

### Explicitly Specified

| Dimension | Content | Clarity |
|---|---|---|
| Purpose | "Standalone SaaS for user feedback" | partial |
| Scope | Extract existing system, generalize | vague |
| Users | "Other SaaS/apps" | vague |
| Behavior | Existing GitCellar spec describes behavior (Phases 1-5) | partial — but standalone version will diverge on multi-tenancy, auth, roadmap backend |
| Appearance | None | unspecified |
| Data | DB schema exists (PostgreSQL) but single-tenant + FK'd to GitCellar's users | partial |
| Integration | None — how do customers consume it? | unspecified |
| Quality | Implied SaaS standards | vague |
| Constraints | None specified | unspecified |
| Success | None specified | unspecified |

### Implicit Requirements Detected

What SHOULD be specified for a standalone SaaS product but isn't:

| Gap | Severity | Assumable? | Deferrable? |
|---|---|---|---|
| Target user persona (indie devs? B2B teams? size?) | **Critical** | No | No |
| Market positioning vs. Canny/Featurebase/Fider | **Critical** | No | No |
| Multi-tenancy architecture (per-customer DB? shared with tenant_id?) | **Critical** | No | No |
| Business model (open-source self-host / SaaS / both / freemium tier shape) | **Critical** | No | No |
| Customers' end-user auth model (their auth, embedded JWT, OAuth, anonymous?) | **Critical** | No | No |
| Roadmap backend (keep Forge dependency? swap for native DB+UI?) | **High** | No | No |
| Product name / branding | **High** | No (placeholder OK) | Yes |
| Repository home (Shared/ library + product folders here vs. new repo) | **High** | No | No |
| Pricing model | **Medium** | Yes (industry comparables) | Yes |
| Hosting model (Cloudflare? Render? customer self-host?) | **Medium** | Yes | Yes |
| Payment provider (own Polar setup? defer payments?) | **Medium** | Yes | Yes |
| GDPR / data residency posture | **Medium** | No (must decide eventually) | Yes (until first EU customer) |
| Launch timeline | **Medium** | No (needs intent) | Yes |

### Ambiguities Detected

| Item | Possible Interpretations | Risk if Wrong |
|---|---|---|
| "Standalone product" | A: hosted SaaS we run; B: open-source self-host customers run; C: both | Different architectures (multi-tenant vs single-tenant), different revenue models, different ops cost |
| "Like GitCellar's feedback system" | A: full Phase 1-5 fidelity; B: subset of features; C: same shape, different impl | Affects extraction scope by 10×; some features (Crash Reporting via Glitchtip, Forge bridge) may not port |
| "Other SaaS could use this" | A: solo founders' side projects; B: small startup teams; C: enterprise customer-feedback teams | Pricing, scale, support model, and feature priority all diverge |

### Hidden Assumption Detection

What the user might be assuming we already know:

- That the existing feedback system's *spec* is reusable (mostly true) but the *code* is heavily coupled (true — needs real decoupling work)
- That the Forge-as-roadmap-backend would carry over (probably NOT — most SaaS customers won't run a Gitea fork; this is a GitCellar architectural choice, not a general one)
- That PassKey-native auth would carry over (probably NOT — most customers expect their end-users to authenticate via the customer's own auth)
- That the product can be incrementally built while GitCellar's pre-launch hardening continues (TBD — capacity question for solo dev)
- That "standalone product" implies SaaS, not a packaged library (TBD — could be either)

### Oracle Candidates (Proactive Oraculurgy)

**Candidates**:

- **feedback-extraction-coupling-points**: scan GitCellar's feedback code for hardcoded "GitCellar" strings, GitCellar-specific FKs, and PassKey-auth assumptions. Output: structured table of what needs abstracting.
  - Signal: extraction needs an inventory of coupling points; this same question gets asked at each module touch
  - Qualification: deterministic ✓ | recurrent ✓ | freshness-contractable ✓ (re-run when code changes) | gracefully-absent ✓
  - Suggested build timing: Task Zero of the extraction stage (after spec converges)

- **feedback-competitor-feature-matrix**: catalogue features of Canny / Featurebase / Fider / Productboard / GitHub Issues+Discussions for positioning analysis.
  - Signal: positioning question can't be answered without comparable data; would be asked across spec + plan + launch decisions
  - Qualification: deterministic ✗ (subjective interpretation) | recurrent ✓ | freshness-contractable ✗ (competitor features change weekly) | gracefully-absent ✓
  - **Does NOT qualify** as an oracle — subjective + freshness-unfriendly. Better as a one-time research artifact in the spec session.

No other strong candidates at this stage — most repeated questions during this initiative will be subjective (market, pricing, positioning), not deterministic.

---

## CALIBRATION

**Task Type**: Greenfield product + architectural migration → **Production-level spec needed**
**Required Spec Level**: **Thorough** (production-facing, public-product)
**Current Spec Level**: **Minimal** (some intent, no boundaries, no decisions)

### VERDICT: **INSUFFICIENT**

The gap between current spec and what's needed for production-level work is large. Most critical dimensions (target user, market positioning, multi-tenancy, business model, auth model, repository home) are unspecified, and most of them are NOT safely assumable — they're foundational decisions that determine architecture.

This is the canonical case for routing to `/0-uldf-ldis-spec`.

---

## ENGAGEMENT STRATEGY

**Approach**: Do NOT ask 12 questions here. Route to `/0-uldf-ldis-spec`, where the spec session can work through these decisions interactively over multiple exchanges, crystallizing each into living documentation. Intake's job is to surface the question set, not resolve it.

### Questions the spec session should resolve (in priority order)

1. **Target user persona** — who is this for? Indie devs shipping side projects? B2B SaaS teams (5-50 employees)? Mid-market customer-success teams? This decision drives everything downstream (pricing, feature priority, support model, complexity tolerance).
2. **Market positioning** — how does this differ from Canny, Featurebase, Fider, Productboard, GitHub Issues+Discussions? "Privacy-first" / "encryption-first" carries over from GitCellar's brand, but customer feedback data isn't generally encryption-sensitive the way Git content is — what IS the unique value prop?
3. **Multi-tenancy architecture** — shared DB with `tenant_id` column? Schema-per-tenant? DB-per-tenant? Self-host single-tenant only?
4. **Customers' end-user auth** — embed widget that takes a customer-signed JWT? OAuth-via-customer's-provider? Magic-link? Anonymous-by-default with optional auth? (Probably NOT PassKey-native — that's a GitCellar-specific choice tied to identity-as-credential.)
5. **Business model** — open-source self-host + paid SaaS (Sentry/Posthog pattern)? Paid SaaS only? Open-core?
6. **Roadmap backend** — keep Forge-as-issues (means each customer needs a Gitea fork — unlikely)? Native DB+UI? Both as options?
7. **Repository home** — three viable shapes; pick one before code moves:
   - **(a) In-place extraction**: `Shared/feedback-core/` (library) + new top-level folders here (`feedbackr-cloud/`, `feedbackr-web/`, `feedbackr-widget/`). GitCellar becomes customer #1.
   - **(b) New repo from start**: `E:\Developer\SourceControlled\Apps\feedbackmonk` with separate workspace; GitCellar imports the published crate/package. *(Originally proposed as `Apps\Feedbackr`; renamed 2026-05-14 per PF-RENAME-02.)*
   - **(c) Hybrid**: Library in `Shared/` here (so GitCellar consumes it locally as a path dependency), product code in a new repo.
   - My lean from the prior turn was (a) — working reference implementation right there, GitCellar as forcing function for the abstraction, split out later. User responded that "Shared/" feels right — that's consistent with (a) for the library part, but the *product code* still needs a home. Worth confirming in spec.
8. **Branding/name** — placeholder OK during spec; actual identity work can be deferred.
9. **Scope of v1** — which of Phase 1-5 ports over for an MVP? Reply Loop + Public Roadmap is probably the irreducible core. Attachments / Crash Reporting / Forge Bridge are extension features that need design re-think for standalone.
10. **Launch posture** — public launch with marketing? Quiet beta to GitCellar's own future user base? Friends-and-family alpha first?

### Documented Assumptions (these can be revisited in spec)

| ID | Assumption | Rationale |
|---|---|---|
| S1 | Solo-dev capacity is the binding constraint — GitCellar pre-launch hardening continues to take priority | User has stated GitCellar is in Phase 4 pre-launch hardening |
| S2 | Customer end-user auth will NOT be PassKey-native (use customer-signed JWT or similar embed pattern) | PassKey-native is identity-as-encryption-key; that's GitCellar-specific |
| R1 | The library extraction should happen in-place (Shared/feedback-core/) before any product-level work | Working reference implementation is the strongest forcing function for a clean abstraction |
| R2 | Forge-as-roadmap-backend will NOT carry over generically; native DB+UI is the standalone product's roadmap layer | Customers won't run Gitea forks; GitHub Issues + reactions is a pattern, not a deployment requirement |
| X1 | The user's intent is hosted SaaS first, self-host later — not the other way around | Inferred from "standalone SaaS product" phrasing; spec should confirm |

### Decision Points Identified

#### Blocking Decisions (must resolve in spec session)
1. Target user persona — Design — drives everything
2. Multi-tenancy architecture — Architecture — drives DB schema, deployment, billing
3. Business model — Business — drives pricing, hosting, support
4. End-user auth model — Architecture — drives integration shape (widget vs. SDK vs. dashboard)
5. Repository home — Architecture — drives directory structure, dependency graph

#### Accumulating Decisions (queue for plan stage)
6. Roadmap backend choice — UX/Technical — needs MVP scope decision first
7. Attachments port — Technical — needs storage decision (S3 generic vs. R2 vs. customer-provided)
8. Crash Reporting port — Technical — Glitchtip dependency is heavy; may not port
9. Forge bridge port — Technical — almost certainly does NOT port; Forge is GitCellar's specific roadmap backend

#### Auto-Decidable (after blocking decisions resolved)
- Email templating engine choice
- Logging/observability stack
- CI/CD shape (mirroring GitCellar conventions where useful)

### Deferred Decisions

| Decision | Deferred Until | Default if Unresolved | Why Defer |
|---|---|---|---|
| Product name | Pre-launch | "feedbackmonk" placeholder | Identity work needs the product to feel real first |
| Pricing tiers | Post-MVP | $X / month / N teammates | Need market signal |
| GDPR data-residency | First EU customer | Single-region US | Reduces ops complexity at MVP |
| Payment provider setup | Pre-monetization | None (free beta) | Polar setup can mirror GitCellar's pattern when ready |

---

## COLLABORATION ASSESSMENT

**Scope**: LARGE → ENTERPRISE
**Subdivisible**: YES (but not yet — spec must crystallize boundaries first)

Once spec converges, the work decomposes naturally into ~5 streams:
1. **Core library extraction** (Shared/feedback-core/) — decoupling, abstracting auth/storage/forge bridges as traits
2. **Multi-tenancy redesign** — DB schema, tenant isolation, admin → customer-admin model
3. **Customer-facing surfaces** — embeddable widget, customer admin dashboard, public roadmap
4. **Billing / monetization** — Polar setup or alternative, tier enforcement
5. **Marketing / landing** — name, branding, gitcellar.com-style landing site

These streams have decent independence after spec stabilizes. **PODS likely beneficial for the implementation stage**, but premature now — spec sessions are inherently single-stream interactive work with the user.

**Net assessment**: SEQUENTIAL for spec phase (now). Re-evaluate as PARALLEL or STAGED when implementation begins.

---

## RECOMMENDED NEXT STEPS

1. **Route to `/0-uldf-ldis-spec`** — start an iterative specification session for the standalone product. The spec session will work through the 10 critical questions above, crystallize decisions to `docs/specs/{project-slug}/` (separate from GitCellar's `docs/specs/feedback-system/` which captures the GitCellar-integrated version), and produce SPECIFICATION.md + DECISIONS.md.

2. **Spec session should start with the foundational triad** — target user persona + market positioning + multi-tenancy architecture. These three answers cascade into most other decisions. Don't try to resolve all 10 questions in one go; converge on the foundations, then iterate.

3. **Defer the "Shared/ vs new-repo" decision to the spec session** — it's a real architectural question but downstream of "what is this product." User's instinct ("probably in Shared/") is correct for the *library* component; the *product* surfaces need their own home.

4. **Do NOT touch any feedback-system code yet** — GitCellar is in pre-launch hardening; destabilizing the reference implementation before the abstraction is designed risks both projects. Extraction begins after spec converges and a plan exists.

5. **Park the GitCellar LTADS session cleanly if switching focus** — the active LTADS session at PAUSED relates to GitCellar work; if the user wants to invest serious time into feedbackmonk spec, formalize the context switch via `/0-uldf-ltads-stop` rather than letting GitCellar's session drift further into staleness.

---

## NOTES

- This intake DOES NOT decide whether to actually pursue feedbackmonk. It surfaces the question set and the verdict that the current spec is insufficient to start implementation. The user retains the option to spec-explore and *then* decide whether it's worth pursuing — that's the value of running spec before committing.
- The existing `docs/specs/feedback-system/` (GitCellar's spec) is a strong starting reference, but should NOT be edited directly — it documents the GitCellar-integrated system. feedbackmonk's spec is a separate artifact.
- If user signals "let's just spec this," route to `/0-uldf-ldis-spec` with the foundational triad as the opening agenda.
