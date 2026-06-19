# feedbackmonk-runner

> Agent-triage synopsis: the autonomous implementer + runner host (FR-FBR-23/24, P5b). Authenticates to the work-order API with a customer-minted runner write-token, polls dispatched orders, drives a swappable agent under the prompt-injection data-envelope discipline, and reports conclusions-only results. **The component that turns approved work orders into code, behind the owner-approval security boundary.**

## 1. Purpose & Responsibilities

Wire the (proven, recommend-only) work-order seam to ACTUAL code execution in the customer's repo, behind the owner-approval boundary (the high-value/high-risk movement P5a sequenced *after* the analyst + approval gate were proven). Responsibilities:

- **Runner host loop** (C26): poll `GET /work-orders?state=dispatched` → claim → drive the implementer → report (`building`/`verifying`/`reported`/`failed`) over the work-order API with runner-token auth.
- **Autonomous implementer** (FR-FBR-23): assemble an injection-disciplined prompt, run a swappable agent against the repo, capture a conclusions-only result.
- **Analyst runtime** (FR-FBR-20, customer-side): scheduled deep-read → recommendation ingestion through the egress sanitizer.
- **Security contracts** (FR-FBR-25b/c): the prompt data-envelope + the outbound source-never-leaves sanitizer.

## 2. File Index

- `lib.rs` — crate root; declares modules + re-exports the frozen seams.
- `types.rs` — frozen data seam: `ClaimedOrder` (trusted/untrusted split), `AssembledPrompt`, `ImplementResult`, `ResultRef` (conclusions-only egress), `RepoContext`.
- `agent.rs` — `AgentCommand` trait (BYO + test-injection seam) + `StubAgent` fake.
- `client.rs` — `WorkOrderClient` (HTTP + runner-token bearer auth). Transport wired by Worker B.
- `prompt.rs` — 25b data-envelope: `wrap_untrusted` (the single chokepoint) + `DEC84_PREAMBLE`. Assembly finalized by Worker A.
- `sanitizer.rs` — 25c egress chokepoint `sanitize_outbound` (reuses `feedbackmonk_tracing::scrub`). Finalized by Worker A.
- `implementer.rs` — `implement` (the B→A seam). Filled by Worker A.
- `poll.rs` — main runner loop: poll dispatched orders → claim → drive implementer → report.
- `claim.rs` — claim a dispatched work order (state `dispatched → building`).
- `report.rs` — report results and transition states (building/verifying/reported/failed); posts sanitized `result_ref`.
- `schedule.rs` — scheduler: `--watch` continuous loop, `--sweep` analyst run, cron/systemd/CI-portable.
- `token_mint.rs` — customer-side runner-token mint helper (Ed25519 seed → signed JWT).
- `default_agent.rs` — `DefaultAgent` impl of `AgentCommand`: spawns `FEEDBACKMONK_AGENT_CMD` (default `claude`) with prompt on stdin; captures `FEEDBACKMONK_RESULT_REF: {json}` from final stdout line.
- `analyst/mod.rs` — analyst runtime entry: `sweep()` over clusters, `ClusterInput`, `SweepTally`, `CandidateRecommendation`.
- `analyst/deep_read.rs` — `deep_read()` (deterministic floor + injectable `AnalystAgent`), `StubAnalystAgent`.
- `analyst/ingest.rs` — `ingest()` per candidate through egress sanitizer + `RecommendationSink` trait, `RecordingSink`.
- `main.rs` — poll-based CLI (`poll`/`mint-token`); wired to the real loop (poll/watch/sweep) and token mint.

## 3. Public API & Usage

```
feedbackmonk-runner poll [--watch] [--sweep]   # drive dispatched orders (+ analyst sweep)
feedbackmonk-runner mint-token --key <path>    # customer-side runner-token mint helper
```

Library: `WorkOrderClient`, `AgentCommand`/`StubAgent`/`DefaultAgent`, `prompt::wrap_untrusted`, `sanitizer::sanitize_outbound`, `implementer::implement`, the `types` data seam, and the analyst runtime (`analyst::sweep`, `analyst::deep_read`, `analyst::ingest`).

**Agent result protocol**: agents print `FEEDBACKMONK_RESULT_REF: {json}` as their final stdout line; `DefaultAgent` captures this and returns it as the `ImplementResult`.

## 4. Constraints & Business Rules

- **Feedback-derived text is DATA, never instructions** — it enters the prompt ONLY via `prompt::wrap_untrusted` (the single chokepoint; `feedback-as-data-audit` Probe A).
- **Source never leaves** — every outbound payload passes `sanitizer::sanitize_outbound` (Probe B); `result_ref`/`source_refs` carry references, never contents (C27 25c).
- **No private key here** — runner tokens are customer-minted (DEC-FBR-04); the runner holds only a short-TTL bearer token. A runner token can NEVER author `approve` (C22 inv. 2), so even full compromise can't bypass the approval gate.
- **The real `claude` spawn is out of the unit-test loop** — exercised only by a manual/`--full` e2e dry-run via the injectable `AgentCommand`.

## 5. Relationships & Dependencies

- `feedbackmonk-tracing` — the canonical PII scrubber reused by the egress sanitizer.
- `feedbackmonk-core` — shared domain types (`ActionType`, …).
- Talks to `feedbackmonk-api`'s work-order + recommendation endpoints over HTTP (runner-token auth) — Contracts C25/C26.

## 6. Decision Log

- **In-repo, AGPL, one coherent system** (not a proprietary split): the split tax serves a monetization model not yet needed; the frozen work-order API seam keeps every packaging option open at near-zero cost (DEC-FBR-12).
- **Customer-mints token model**: dictated by the frozen C22 `verify_runner_token` seam (verifies against the project's registered public keys); feedbackmonk stays private-key-free (DEC-FBR-04).
- **Poll-based, not webhook**: portable to local repos with no public endpoint; matches the self-host story. Push is a later optimization.
- **One abstraction for BYO + testability**: the swappable `AgentCommand` is both the BYO-agent seam (Q20) and the test-injection seam (Testability Gate Flag 1).
- **`/runner/` namespace for server-side read endpoints** (DEC-FBR-IMPL-20): avoids axum merge-conflict with the admin router; the different auth principal (runner-token, no CORS) and call pattern (state-poll vs. project-CRUD) make the separation explicit.
- **Server-side read/ingestion endpoints as Stage 1 blocker** (DEC-FBR-IMPL-19): the runner binary cannot poll without a server to poll; these endpoints were a gap that had to be closed before Stage 1 convergence.
- **C27 25b single chokepoint design**: feedback-derived text (cluster summaries, recommendation body/rationale, member bodies, source_refs) enters the implementer prompt through exactly ONE function — `prompt::wrap_untrusted` — inside a delimited `<untrusted-feedback-data>` envelope. The trusted instruction layer carries only owner-approved `instructions`/`owner_overrides` + the DEC-84 critical-action preamble. Forged-delimiter defanging (`assemble` strips the delimiter string from any untrusted field before passing it to `wrap_untrusted`) closes the envelope-escape attack. The `feedback-as-data-audit` oracle Probe A verifies this single-chokepoint property from code, not from a self-reported flag.
- **C27 25c egress design**: every outbound POST (implementer `result_ref`, analyst recommendations) routes through `sanitizer::sanitize_outbound` — no exceptions. The sanitizer reuses `feedbackmonk_tracing::scrub` (the canonical 20-pattern PII scrubber), adds a secret-pattern denylist (reject on secret-dump), and enforces references-not-dumps on `source_refs`/`result_ref`. `SanitizeError` has exactly two variants: `SecretDump` (reject the payload) and `Malformed` (parse failure). Probe B verifies the "every outbound path" property from code.
- **Runner-token structural trust bound**: a runner token (key_class=runner) authorizes only runner state transitions (`claim`/`building`/`verifying`/`reported`/`failed`) and can never author an `approved` event (C22 invariant 2). The `verify_runner_token` function selects only `runner`-class keys; the `approve` handler requires `AdminSession` (a different auth path entirely). This means even complete compromise of a runner token cannot bypass the owner-approval gate — the security boundary is structural, not policy.
