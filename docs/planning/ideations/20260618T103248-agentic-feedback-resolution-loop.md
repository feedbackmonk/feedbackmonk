# Ideation Notes — Agentic Feedback Resolution Loop
**Source**: /0-uldf-ldis-ideate
**Generated**: 2026-06-18T10:32:48
**Project**: feedbackmonk (feature-set on the existing product — NOT a new project)
**Working name**: "feedbackmonk Autopilot" (provisional umbrella for the proprietary agentic loop; the analyst role keeps the user's original term "Project Feedback Agent"). Finalize naming during the spec session.

## The Idea
A closed-loop, agentic layer that turns feedbackmonk from "a nicer feedback inbox" into
"the thing that tells you what to build next — and then builds it." Public feedback comes in
one end; tested, verified, shipped code comes out the other. A per-project agent reads the
project (including its source code, deeply) and the project's feedback corpus, then:

1. **Organizes** — clusters near-duplicate feedback ("same thing said many ways") under one
   generic label/item.
2. **Analyzes & prioritizes** — decides high/low priority across the whole corpus.
3. **Recommends action** — flags actionable items with a *type* (bug fix, feature enhancement,
   feature implementation, etc.) and a concrete recommendation grounded in the real code.
4. **Executes (optionally, on owner approval)** — "Fix this bug" / "Build this feature" runs the
   full ULDF pipeline (LDIS spec → LTADS build → finalize/verify) to development completion with
   tested verification, then reports back.

The owner's job collapses to: glance at a digest → approve / tweak / reject → done.

## Problem Space
Product owners drown in feedback. The cognitive work — reading everything, noticing duplication,
deciding what's urgent, translating "users want X" into "here's the bug/feature and how to fix it,"
then actually doing it — is all manual today. feedbackmonk already collects, triages (status
workflow), and surfaces (roadmap + voting). This feature automates the *judgment and the execution*
that currently sit between collection and shipped change.

## Key Insights from Discussion
- **This is a layer on the existing product, not a new product.** It slots between collection and
  triage and reaches forward into execution. The existing roadmap-promote path (Q24 / FR-FBR-12)
  is a natural hand-off point.
- **The big reveal: feedbackmonk becomes the intake-and-control surface for an autonomous dev loop.**
  Front half = feedback agent; back half = ULDF itself (LDIS/LTADS/finalize). Nobody connects
  "voice of the customer" directly to "autonomously implemented and verified." That's the novel wedge.
- **The architecture and the licensing boundary are the same line.** Agent lives in the customer's
  project, reads source *where it already sits*, and does everything else via a feedbackmonk API —
  only conclusions cross the wire, never source code. Because the open repo is AGPL, anything
  proprietary MUST live outside it (in ULDF / a separate commercial runner) talking to feedbackmonk
  only through the public API. Same line, drawn once.
- **The approval gate is a SECURITY boundary, not a UX step.** Feedback originates from anyone on the
  public web. If a submitter could ever trigger execution (via crafted feedback text, poisoned
  cluster, prompt injection in a feedback body), it becomes remote code execution from the internet.
- **Open-source is load-bearing to the brand** ("Plausible Analytics for product feedback"; AGPL,
  self-host, no trackers, JWT-only identity). It makes the customer-side-agent architecture *stronger*:
  a self-hoster can run backend + agent entirely inside their own walls and trust nothing external.

## Identified Feature Domains (six-part decomposition)
1. **Collection** — widget + API *(exists today)*.
2. **Analyst agent** *(read-mostly, proprietary)* — continuous lightweight clustering of new
   submissions + scheduled deep sweep for prioritization & recommendation. Reads project source deeply.
3. **Review & approval surface** — dashboard digest + autonomy-ladder dial + the approval gate
   (= the security boundary between public input and code execution).
4. **Work-order API** — the contract between an approved decision and a dispatched job.
5. **Implementer** *(write-heavy, proprietary)* — the ULDF loop: spec → build → finalize/verify →
   report status/PR back over the API.
6. **The runner** — customer-side host for #2 and #5. Open path = bring-your-own-agent (call any
   coding agent via the work-order API); proprietary path = turnkey ULDF-powered runner.

## Decisions Made (to be ratified + ID'd in DECISIONS.md)
- **Cadence default**: analysis runs *scheduled* (digest waiting for you); on-demand "review now"
  always available. *Execution is never automatic by default* — always an explicit owner approval,
  until the owner deliberately raises the autonomy rung.
- **Graduated autonomy ladder** governs how far the agent walks before it needs a signature
  (Rung 0 organize+prioritize → R1 draft artifacts → R2 auto-execute low-stakes + escalate the rest
  → R3 act-and-report). Mirrors ULDF's manual→collaborative→autopilot. Owner picks the rung; same
  approve/tweak gate at every rung.
- **How the agent runs**: a thin "runner" installed in the customer's repo. Analysis half wakes on a
  schedule (cron/CI/GitHub Action); implementation half is event-driven (owner approval enqueues a
  work order the runner picks up). One integration point, two trigger modes. For the owner's *own*
  projects, the runner is just their existing Claude Code + ULDF setup pointed at the API.
- **Monetization = open-core.** Open (AGPL feedbackmonk repo): collection, triage, roadmap/voting,
  the API, the review surface, the work-order contract. Proprietary (the moneymaker, outside the AGPL
  repo): the autonomous agents — **both analyst and implementer** — and the turnkey ULDF-powered runner.
  Self-hosters get BYO-agent; payers get the turnkey magic.
- **Analyst is proprietary — but REVERSIBLY.** The user judged analysis quality itself a premium
  differentiator, so it starts proprietary. Explicit open door: revisit moving the analyst into open
  once we understand it better and have built it out. Record this as a *reversible* decision so a
  future session doesn't treat it as permanent.

## Load-bearing invariants (must be specced as hard requirements)
- **Approval gate = security boundary.** Only the authenticated owner ever triggers execution.
- **All feedback content is untrusted DATA, never instructions.** Prompt-injection defense is a
  first-class requirement, not a later patch. (Sibling to DEC-FBR-02/04 privacy invariants and the
  Q24 invariant.)
- **Source code never leaves the customer environment.** Only conclusions/artifacts cross the API.
- **Proprietary code never lands in the AGPL repo.** Licensing boundary == architecture boundary.

## Scope Thinking
- **This feature-set spans many sessions of spec → doc → plan BEFORE any LTADS implementation.**
  Justified: it's a strategic decision (open-core boundary), a security-critical surface (public
  input → code execution), and genuinely cross-cutting (API + data model + new dashboard + runner
  contract + ULDF integration). Profile = "Vague OR Complex OR Large" → LDIS routes through spec.
- **Likely earliest buildable slice**: the analyst (cluster + prioritize + recommend) + review
  surface + work-order API, *without* the implementer — i.e. decisions stop at "here's what to do."
  The implementer/ULDF loop is the higher-risk, higher-value second movement.

## Open Questions (for the spec session)
- Exact `FR-FBR-*` decomposition and phase placement (P5+? new arc?).
- Work-order schema + approval state machine shape.
- Runner auth: extend the existing project-scoped Ed25519 / JWT handshake to a write-scoped API token
  for the agent client. (Natural extension of widget trust machinery — confirm shape.)
- Data model for clusters + recommendations + work orders + approval/audit trail.
- How "tweak before approve" round-trips edits back to the agent.
- Where the proprietary runner repo lives and how it's distributed/licensed/billed.
- Prompt-injection threat model + test fixtures (adversarial feedback bodies).

## Next Step
Transition to `/0-uldf-ldis-spec` to fold these into feedbackmonk's existing
`docs/specs/` (new `FR-FBR-*` requirements + the open-core `DEC-FBR-*` + the security invariants),
then `/0-uldf-ldis-plan` for the build arc. No code until spec + strategy are settled.
