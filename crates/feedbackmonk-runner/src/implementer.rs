//! The autonomous implementer (FR-FBR-23) — the B→A seam.
//!
//! Worker B's runner loop calls [`implement`] on a claimed order; Worker A
//! fills it. The decomposition (Testability Gate Flag 1 mitigation) is:
//!   (a) **pure prompt-assembly** (`prompt::assemble`, hermetically corpus-tested),
//!   (b) an **injectable [`AgentCommand`]** (mocked in tests; real `claude` only
//!       in a manual/`--full` e2e dry-run),
//!   (c) **pure result-capture** (build the conclusions-only `ResultRef`, run it
//!       through `sanitizer::sanitize_outbound`).
//!
//! Stage-0: the signature is frozen (stub bails) so Worker B's loop compiles
//! against it today; Worker A finalizes the body + the C24 (g) corpus.

use crate::agent::AgentCommand;
use crate::types::{ClaimedOrder, ImplementResult, RepoContext};

/// Drive the implementer for one claimed order: assemble the envelope-disciplined
/// prompt (`prompt::assemble`), run the injectable `agent`, and capture a
/// sanitized conclusions-only result.
///
/// # Errors
/// Stage-0 stub: returns an error until Worker A (Stage 1) fills the body.
pub async fn implement(
    _agent: &dyn AgentCommand,
    _order: &ClaimedOrder,
    _repo: &RepoContext,
) -> anyhow::Result<ImplementResult> {
    // SEAM (Worker A, Stage 1):
    //   1. prompt::assemble(order)              — 25b envelope (single chokepoint)
    //   2. agent.run(prompt, repo)              — injectable; real claude in e2e
    //   3. sanitizer::sanitize_outbound(...)    — 25c references-not-dumps egress
    anyhow::bail!("implementer::implement is finalized by Worker A (Stage 1, FR-FBR-23)")
}
