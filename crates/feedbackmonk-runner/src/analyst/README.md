# `crates/feedbackmonk-runner/src/analyst/` — the customer-side analyst deep read

## Summary

The analyst runtime (FR-FBR-20, C26): run by `feedbackmonk-runner poll --sweep`, it turns the
clusters the runner host fetches into **recommend-only** candidate recommendations and posts each
one through the egress sanitizer. Come here to change how recommendations are derived or how they
leave the customer's machine.

## Notes

- **Recommend-only.** A candidate cannot execute without a later owner `approved` event (the
  approval gate, C22); the analyst only proposes.
- **The deterministic floor is always on.** Every actionable cluster gets a baseline candidate
  from its `FeedbackKind`; an injected `AnalystAgent` augments it, never replaces it, and its
  failure is non-fatal.
- **Every feedback-derived field passes `sanitize_outbound` before the wire.** `cluster_id` and
  `sweep_id` are attached after sanitizing on purpose: they are trusted host-supplied UUIDs, and
  the PII scrubber would rewrite them to `[uuid]`.

## File Index

| File | What it is |
|---|---|
| `mod.rs` | Entry point: `sweep()` over the host-supplied `ClusterInput`s, `SweepTally`, `CandidateRecommendation`. |
| `deep_read.rs` | `deep_read()` — the deterministic floor (`deterministic_candidate`) plus the optional injectable `AnalystAgent`; `StubAnalystAgent` for tests. |
| `ingest.rs` | `ingest()` — builds the content payload, runs it through `sanitize_outbound`, attaches routing IDs and posts via the `RecommendationSink` seam over `WorkOrderClient::post_recommendation`. |
