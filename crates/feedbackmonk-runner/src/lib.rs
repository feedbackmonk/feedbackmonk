//! `feedbackmonk-runner` — the autonomous implementer + runner host (FR-FBR-23 /
//! FR-FBR-24, Contracts C26/C27 — P5b).
//!
//! **This is the component that wires the (already-proven, recommend-only)
//! work-order seam to ACTUAL code execution in the customer's repo, behind the
//! owner-approval security boundary.** Built in-repo, fully AGPL, as one
//! coherent system (DEC-FBR-12 packaging: no proprietary split now; the frozen
//! work-order API seam keeps every option open at near-zero cost).
//!
//! ## What this crate is (P5b scope) and the Stage-0 skeleton
//!
//! Stage 0 (foundation) freezes the shared seams so the Stage-1 workers fan out
//! without inventing incompatible prompt-envelope shapes, `result_ref` schemas,
//! or token semantics — on the security-critical path, where incompatibility is
//! a hole, not a merge headache. The seams frozen here:
//!
//! - [`types`] — `ClaimedOrder` / `AssembledPrompt` / `ImplementResult` /
//!   `ResultRef` (the conclusions-only egress shape, 25c) / `RepoContext`.
//!   Concrete; A/B/C build against them verbatim.
//! - [`agent`] — the swappable [`agent::AgentCommand`] trait (the BYO seam, Q20;
//!   also the test-injection seam — Testability Gate mitigation: the real
//!   `claude` spawn is exercised only by a manual/`--full` e2e dry-run, never
//!   unit tests).
//! - [`client`] — `WorkOrderClient` (HTTP client + runner-token auth). Method
//!   signatures freeze in Stage 0; Worker B wires the transport.
//! - [`prompt`] — the 25b data-envelope: the SINGLE chokepoint
//!   [`prompt::wrap_untrusted`] through which feedback-derived text enters the
//!   prompt, + the DEC-84 critical-action preamble. Worker A finalizes `assemble`.
//! - [`sanitizer`] — the 25c egress chokepoint [`sanitizer::sanitize_outbound`]
//!   every outbound payload passes through. Worker A finalizes.
//! - [`implementer`] — the B→A seam [`implementer::implement`]. Worker A fills it.
//!
//! Worker B appends `poll`/`claim`/`report`/`schedule`/`token_mint`; Worker C
//! appends `analyst/*` — both APPEND to this module list (the one manual-merge
//! point under `--worktrees`).
//!
//! ## The load-bearing security contract (C27, FR-FBR-25b/c)
//!
//! Feedback-derived text is **never** concatenated into the instruction/system
//! layer — only inside a delimited untrusted-data envelope ([`prompt`]). And
//! only conclusions/references cross the wire — [`sanitizer`] is the egress
//! chokepoint (source never leaves). The `feedback-as-data-audit` Verification
//! Oracle proves both FROM CODE (detection-from-code, not a self-reported flag);
//! a green oracle is Worker A's Stage-1 exit gate.

pub mod agent;
pub mod client;
pub mod implementer;
pub mod prompt;
pub mod sanitizer;
pub mod types;

pub use agent::{AgentCommand, StubAgent};
pub use client::WorkOrderClient;
pub use types::{
    AssembledPrompt, ClaimedOrder, DiffStat, ImplementResult, RecommendationContext, RepoContext,
    ResultRef, Verification,
};
