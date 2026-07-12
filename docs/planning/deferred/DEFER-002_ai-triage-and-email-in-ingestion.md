---
id: DEFER-002
title: AI triage + email-in ingestion (tenant-generic Phase 2 of the anonymous-visitor feedback story)
status: PROPOSED
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
