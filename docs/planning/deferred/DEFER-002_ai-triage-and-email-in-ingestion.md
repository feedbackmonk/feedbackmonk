---
id: DEFER-002
title: AI triage + email-in ingestion (tenant-generic Phase 2 of the anonymous-visitor feedback story)
status: TRIAGED
origin: inject
source-project: GitCellar
source-session-id: cb2a8550-55bf-4cc4-be82-6269e53fadb4
injected-at: 2026-07-11T22:10:00Z
autonomy-hint: collaborative
suggested-entry-point: spec
scope-estimate: multi-session
content-hash: fbm-ai-triage-emailin-v1
---

# DEFER-002: AI triage + email-in ingestion

> ## Disposition — TRIAGED (2026-08-06, `defer-drain-FeedbackMonk-20260806`)
>
> **Historical below this banner; do not cite the body as a statement of what is unbuilt.** The
> § Originating Context is preserved as the only record of the GitCellar owner's design intent
> (2026-07-11) and still governs the *shape* of what remains. The brief's premise about
> feedbackmonk's state does not.
>
> **Feature (1) "AI triage" was ALREADY SHIPPED when this brief was written.** The brief was
> injected 2026-07-11 from a GitCellar session; feedbackmonk's P5a/P5b agentic loop landed
> 2026-06-18..21, three weeks earlier. Point by point against the brief's own wording:
>
> | Brief asks for | Shipped as | Status |
> |---|---|---|
> | "dedupe" — *same thing said many ways → one item* | **FR-FBR-19** feedback clustering (analyst, on submit; `feedback_clusters` + `clusters.rs`) | DONE |
> | "auto-categorize" + *"what should we work on next"* | **FR-FBR-20** analysis sweep — sets priority, classifies each cluster by action type, emits a grounded recommendation | DONE |
> | "periodically digest" | **FR-FBR-20** owner-facing digest, default cadence scheduled + on-demand "review now" | DONE |
> | admin-UI digest / operator decision surface | **FR-FBR-21** `admin-ui/src/pages/autopilot/*` (cluster digest, recommendation cards, approve/tweak/reject) | DONE |
> | "sentiment-classify" | **FR-FBR-28** first-class 3-point sentiment + trend aggregation | DONE (submitter-supplied — see residual below) |
> | "provider-pluggable LLM config per self-host posture… local model option matters" | **FR-FBR-30** translation provider abstraction (`off` default / DeepL / self-hosted LibreTranslate) **and** **FR-FBR-24/25c** BYO-agent runner where project source never leaves the customer environment | DONE — the privacy-first pluggability the brief wanted exists twice over |
> | "annotates rather than mutating operator-owned fields" | **FR-FBR-22** work-order + approval state machine; **FR-FBR-25a** approval-as-security-boundary | DONE — stronger than asked |
>
> Verified by code census, not by reading the spec's own status column: handlers `clusters.rs` /
> `sweeps.rs` / `recommendations.rs` are constructed and merged into the admin surface in
> `crates/feedbackmonk-api/src/main.rs` (`SqlxClusterRepo` / `SqlxRecommendationRepo` /
> `SqlxAnalysisSweepRepo`, wired at `main.rs:266-268`, routed at `main.rs:587`), and the
> customer-side analyst runtime exists at `crates/feedbackmonk-runner/src/analyst/`.
>
> **Residual of feature (1) — narrow, NOT carried as a separate item.** FR-FBR-28 sentiment is
> *submitter-supplied at submit time*; the brief asked for AI **inference** of "sentiment when
> absent" and of a suggested `kind`. That is genuinely unbuilt. It is folded into the triaged
> node below rather than tracked separately, because it needs the same decisions (per-project
> opt-in, egress posture, async-worker placement) that email-in needs, and the FR-FBR-30
> translate-after-accept worker is the pattern both would copy.
>
> **Feature (2) "email-in ingestion" is genuinely unbuilt and is TRIAGED into the spec** as
> **FR-FBR-31 (DEFERRED)** — see `docs/specs/SPECIFICATION.md` § *Capability extension —
> inbound-email ingestion + AI annotation*. Verified absent by census: the repo has **no**
> IMAP / inbound-mail / mailbox-poller code at all; every `Mailbox` symbol in `crates/` is
> outbound `lettre` (`email/{send,mailpit,env_smtp}.rs`).
>
> **Re-arm condition (the brief's own volume gate, made measurable).** Do not build until
> hand-reading hurts *for a real tenant*. That is now directly observable from machinery that
> already exists: the **FR-FBR-20 analysis-sweep digest** reports cluster counts per sweep. Re-arm
> when a tenant's sweep digest shows sustained inbound the owner is not keeping up with, **or**
> when an operator reports manual mailbox triage (GitCellar's Zoho `feedback@gitcellar.com`) as
> the bottleneck. **Not measured by this session** — the live instance's volume needs
> ops credentials this session did not use.
>
> Entry point when it re-arms: `/0-uldf-ldis-spec` (the brief's own `suggested-entry-point`) —
> multi-session, and the shape below is illustrative, not binding.

## Idea

Two tenant-generic feedbackmonk product features that turn low-structure inbound feedback into decision-grade structure on the operator side: (1) **AI triage** — auto-categorize, dedupe, sentiment-classify, and periodically digest feedback into a "what should we work on next" view; (2) **email-in ingestion** — accept feedback arriving at a tenant's support/feedback mailbox (e.g. `feedback@gitcellar.com`) into the same project inbox, AI-parsed into kind/sentiment/body. Both are deliberately **volume-gated**: build when a tenant's inbound flow makes hand-reading uneconomical, not before.

## Originating Context

GitCellar (tenant #1, project `a1350be8-3ff5-4744-9e1d-e35c97cc8aad` on the self-hosted instance at `feedback.gitcellar.com`) is preparing its public beta launch and made anonymous visitor feedback a launch centerpiece: a conspicuous "Send feedback" entry (navbar + home-page callout) on its Cloud Forge, powered by the already-shipped feedbackmonk anonymous mode (FR-FBR-03/06) and the embeddable widget in launcher-less mode.

The design discussion (GitCellar owner, 2026-07-11) resolved a capture-vs-structure tension with the principle: **capture with near-zero structure, add structure downstream**. The visitor types one sentence (optional kind chip, optional email); categorization for the operator's "what to work on next" decisions happens at triage time. The owner explicitly wanted:

- "AI could take whatever general feedback they give and auto-categorize or sort it for us" — that is feature (1). At GitCellar's launch volume the owner reads everything by hand, so this was consciously deferred, but it becomes the mechanism that keeps the zero-structure capture promise sustainable as volume grows — and it is a product differentiator for every future feedbackmonk tenant ("Plausible for product feedback" + honest AI triage).
- "Anything going to feedback@gitcellar.com could also be integrated into FeedbackMonk's feedback with AI if deemed worthy" — that is feature (2). Email is the channel some users default to; today GitCellar reads that Zoho mailbox manually. Email-in is table stakes for helpdesk-adjacent products and is naturally tenant-generic (per-project inbound address or forwarding target, AI-parsed into a submission).

Guardrails carried over from the GitCellar-side consolidation program (GitCellar repo, `docs/planning/plans/20260701-feedback-consolidation-onto-feedbackmonk.md` §2): feedbackmonk contract changes must be **tenant-generic product features, not GitCellar hacks**, additive to the frozen contract, and advertised via the existing `GET /api/v1/capabilities` flag pattern (mirroring the planned `feedback.delete` / `feedback.attachments` / `feedback.reply_state` flags from that program's Phase A).

Suggested shapes (illustrative, not binding — spec session decides):
- AI triage: per-project opt-in; an async worker that annotates submissions (suggested kind, duplicate-of link, sentiment when absent) rather than mutating operator-owned fields; a periodic digest surface (email or admin-UI view) ranking themes by frequency/recency; provider-pluggable LLM config per self-host posture (privacy-first brand ⇒ tenant controls where text is sent — a self-hostable/local model option matters to this audience).
- Email-in: per-project ingest address or IMAP/forwarding poller; AI parse → kind/body/sentiment + submitter email captured as the optional contact email; dedup/rate-limit reusing the anon-gate machinery; moderation_status for spam.

## Success Criterion

A feedbackmonk tenant can (1) enable AI triage on a project and see submissions auto-annotated (category/dupe/sentiment) plus a recurring "top themes" digest, and (2) point a mailbox at the project and see well-formed submissions appear in the same inbox — with both features off by default, capability-flagged, and the AI legs clearly disclosed in the privacy story. GitCellar can then retire manual Zoho triage of `feedback@gitcellar.com`.

## Dependencies

- Volume signal from tenant #1's beta (do not build until hand-reading hurts — the injector's explicit phasing).
- The feedback-consolidation Phase A contract additions (delete/attachments/reply-state) are independent; no ordering constraint, but the capability-flag pattern should stay consistent with them.

## Related Artifacts

- GitCellar repo: `docs/planning/plans/20260701-feedback-consolidation-onto-feedbackmonk.md` (tenant-generic guardrails, capability-flag pattern, Phase A gap list).
- GitCellar repo: `docs/planning/feedbackmonk-deploy-state.md` (live deployment, project/tenant IDs, ops token custody).
- This repo: `docs/specs/SPECIFICATION.md` FR-FBR-03/06 (anonymous submission — already shipped), FR-FBR-28 (sentiment), widget `src/api.ts` (anon submit path).
- GitCellar Zoho mailbox tooling (for the email-in migration story): GitCellar repo `.claude/skills/1-email-triage/SKILL.md`.
