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
- `main.rs` — poll-based CLI (`poll`/`mint-token`). Loop bodies wired by Worker B/C.

## 3. Public API & Usage

```
feedbackmonk-runner poll [--watch] [--sweep]   # drive dispatched orders (+ analyst sweep)
feedbackmonk-runner mint-token --key <path>    # customer-side runner-token mint helper
```

Library: `WorkOrderClient`, `AgentCommand`/`StubAgent`, `prompt::wrap_untrusted`, `sanitizer::sanitize_outbound`, `implementer::implement`, and the `types` data seam.

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
