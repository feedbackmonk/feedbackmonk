---
status: resolved 2026-09-30 (enveloped on the owner's word, DEC-FBR-IMPL-33)
source: project-checks review, 2026-09-30 (docs/planning/project-checks-review-2026-09.md)
deferred_because: the owner's decision — a design choice about the prompt's trust layers (FR-FBR-25b), not a defect with one fix
harm: model-derived feedback text reaches the runner agent's trusted instruction layer, guarded only by owner approval
beneficiary: every tenant that enables the autopilot runner
---

# Recommendation text enters the runner prompt outside the untrusted envelope

## What is true today

`create_work_order` copies a recommendation's `title` and `body` into the work
order's title and instructions (`crates/feedbackmonk-api/src/handlers/work_orders.rs`,
the work-order create path). The runner's `assemble` then places those in the
**trusted** instruction layer of the agent prompt (`crates/feedbackmonk-runner/src/prompt.rs`,
the instruction block ahead of the envelope). Only raw feedback bodies are wrapped by
`wrap_untrusted` in `<untrusted-feedback-data>`.

A recommendation is written by the analyst model *from* feedback, so a prompt injection
in a feedback body can survive into the recommendation's text and be presented to the
implementing agent as instruction rather than data. The only guard on that path is the
owner's approval of the work order (FR-FBR-25a) — which reviews the text, but is a human
reading a model's summary.

## The decision (a DEC)

1. **Envelope it** (recommended): the trusted layer carries only owner-authored text
   (the approval note, a fixed task preamble); recommendation title and body move inside
   the untrusted envelope and are quoted as the analyst's summary of feedback.
2. **Treat approval as the trust boundary**: record in a DEC that an approved work
   order's text is owner-endorsed and therefore trusted, and say so in FR-FBR-25b.

The `feedback-as-data-audit` oracle checks the envelope chokepoint; whichever is chosen,
extend it to assert the trusted layer's sources.
