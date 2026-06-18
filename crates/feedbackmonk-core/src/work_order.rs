//! Work-order approval state machine — Contract C22 of the P5a plan.
//!
//! THE security boundary between public internet input and code execution
//! (FR-FBR-25a). The state machine mirrors `status.rs`'s proven FR-FBR-08
//! pattern: an explicit enum, a `legal_transitions_from` table, illegal
//! transitions rejected pre-DB-check, and (in Worker A's handler) a
//! `work_order_events` audit row written in the SAME transaction as the
//! `work_orders.state` update (C22 inv. 3).
//!
//! Stage 0 freezes the enum + transition table + the two security-load-bearing
//! helpers ([`WorkOrderState::is_terminal`], [`WorkOrderState::is_execution_state`]).
//! Worker A (Stage 1) implements the transition handler + authz matrix on top.
//!
//! **C22 invariant 1 (the security boundary)**: no work order may reach a state
//! for which [`WorkOrderState::is_execution_state`] is true without a prior
//! owner-authored `approved` event in the `work_order_events` ledger. This enum
//! is the in-code anchor for that invariant; the `approval-gate-enforcement`
//! Verification Oracle independently proves it from the state-machine source +
//! the event ledger (detection-from-state, not a self-reported flag).

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Work-order lifecycle state. The DB form (and JSON form) is `kebab-case` and
/// must match the `state` / `to_state` / `from_state` CHECK constraints in
/// `00014_work_orders.sql` byte-for-byte.
///
/// Lifecycle (owner-authored transitions marked ⊙, runner-authored ▶,
/// system ◆):
/// ```text
/// draft ⊙approve→ approved ◆dispatch→ dispatched ▶claim→ claimed
///                                                          │
///                                                     ▶building→ building
///                                                          │
///                                                    ▶verifying→ verifying
///                                                          │
///                                                     ▶reported→ reported
///   reported ⊙accept→ completed (terminal)
///   reported ⊙request-changes→ building          (re-open loop, carries overrides)
///   reported ⊙reject→ failed ⊙retry→ approved
///   any non-terminal ⊙cancel→ cancelled (terminal)
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkOrderState {
    /// Agent-drafted order awaiting owner approval. Entered only at autonomy
    /// Rung >= 1 (C22 inv. 4). The initial state for every created work order.
    #[default]
    Draft,
    /// Owner approved (the security gate). The `approved` event in the ledger
    /// is the authorization C22 inv. 1 requires before any execution state.
    Approved,
    /// System dispatched the approved order to the runner queue. First
    /// execution-class state.
    Dispatched,
    /// Runner claimed the dispatched order.
    Claimed,
    /// Runner is implementing.
    Building,
    /// Runner is verifying its work (tests/build).
    Verifying,
    /// Runner reported a result back; awaits owner accept / request-changes /
    /// reject.
    Reported,
    /// Owner accepted the reported result. Terminal.
    Completed,
    /// Build or verification failed. Owner may retry (-> approved) or cancel.
    Failed,
    /// Owner cancelled a non-terminal order. Terminal.
    Cancelled,
}

impl WorkOrderState {
    /// DB-side string form. Must match the CHECK constraints in
    /// `00014_work_orders.sql` byte-for-byte.
    #[must_use]
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Approved => "approved",
            Self::Dispatched => "dispatched",
            Self::Claimed => "claimed",
            Self::Building => "building",
            Self::Verifying => "verifying",
            Self::Reported => "reported",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Lenient DB-string parser. Unknown values fall back to `Draft` (the
    /// non-execution initial state) to mirror `FeedbackStatus::from_db_str`;
    /// the CHECK constraint guarantees only the ten canonical values are ever
    /// stored. Falling back to `Draft` (never an execution state) means a
    /// malformed DB value can never be read as an already-approved/dispatched
    /// order.
    #[must_use]
    pub fn from_db_str(s: &str) -> Self {
        match s {
            "approved" => Self::Approved,
            "dispatched" => Self::Dispatched,
            "claimed" => Self::Claimed,
            "building" => Self::Building,
            "verifying" => Self::Verifying,
            "reported" => Self::Reported,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => Self::Draft,
        }
    }

    /// Terminal states accept no further transitions (C22 inv. 5).
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled)
    }

    /// **THE security predicate (C22 inv. 1).** True for every state that
    /// represents the work order being (or having been) handed to the runner
    /// for execution: `dispatched` and everything reachable only through it
    /// (`claimed`, `building`, `verifying`, `reported`, `completed`, `failed`).
    ///
    /// A work order MUST NOT enter any execution state without a prior
    /// owner-authored `approved` event in the ledger. `draft`, `approved`, and
    /// `cancelled` are NOT execution states — approval is the gate, not an
    /// execution; cancellation is an owner escape hatch from any non-terminal
    /// state.
    ///
    /// This is the single in-code source of truth for "execution-class state."
    /// The `approval-gate-enforcement` oracle proves the same property
    /// independently from the event ledger.
    #[must_use]
    pub fn is_execution_state(self) -> bool {
        matches!(
            self,
            Self::Dispatched
                | Self::Claimed
                | Self::Building
                | Self::Verifying
                | Self::Reported
                | Self::Completed
                | Self::Failed
        )
    }
}

/// Legal transitions out of a given state (C22). Returned ordering is stable so
/// admin UIs can render controls left-to-right without re-sorting.
///
/// Mirrors `status.rs::legal_transitions_from`. `completed` and `cancelled`
/// are terminal (empty). `reported -> building` is the request-changes loop;
/// `failed -> approved` is the retry path.
#[must_use]
pub fn legal_transitions_from(s: WorkOrderState) -> &'static [WorkOrderState] {
    use WorkOrderState::{
        Approved, Building, Cancelled, Claimed, Completed, Dispatched, Draft, Failed, Reported,
        Verifying,
    };
    match s {
        Draft => &[Approved, Cancelled],
        Approved => &[Dispatched, Cancelled],
        Dispatched => &[Claimed, Cancelled],
        Claimed => &[Building, Cancelled],
        Building => &[Verifying, Failed, Cancelled],
        Verifying => &[Reported, Failed, Cancelled],
        Reported => &[Completed, Building, Failed], // Building = request-changes loop
        Failed => &[Approved, Cancelled],           // Approved = retry
        Completed => &[],
        Cancelled => &[],
    }
}

/// True iff `from -> to` is a legal transition (membership in
/// [`legal_transitions_from`]). The pre-DB-check Worker A's handler runs before
/// opening a transaction.
#[must_use]
pub fn is_legal_transition(from: WorkOrderState, to: WorkOrderState) -> bool {
    legal_transitions_from(from).contains(&to)
}

/// Errors emitted by Worker A's transition handler (Stage 1). Frozen in
/// Stage 0 so Worker A's handler signature and Worker C's UI error-rendering
/// implement against the same surface. Mirrors `status.rs::TransitionError`.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WorkOrderTransitionError {
    #[error("illegal transition from {from:?} to {to:?}")]
    IllegalTransition {
        from: WorkOrderState,
        to: WorkOrderState,
    },
    /// The transition would enter an execution state but no owner-authored
    /// `approved` event exists in the ledger (C22 inv. 1 — the security gate).
    #[error("execution state {to:?} requires a prior owner-authored approval")]
    ApprovalRequired { to: WorkOrderState },
    /// The actor is not authorized to author this transition per the C22 authz
    /// matrix (e.g. a runner attempting `approve`, or an admin attempting a
    /// runner-only `building`).
    #[error("actor not authorized to author this transition")]
    ActorNotAuthorized,
    /// Target order is in a terminal state (C22 inv. 5).
    #[error("work order is in a terminal state and cannot transition")]
    TerminalState,
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [WorkOrderState; 10] = [
        WorkOrderState::Draft,
        WorkOrderState::Approved,
        WorkOrderState::Dispatched,
        WorkOrderState::Claimed,
        WorkOrderState::Building,
        WorkOrderState::Verifying,
        WorkOrderState::Reported,
        WorkOrderState::Completed,
        WorkOrderState::Failed,
        WorkOrderState::Cancelled,
    ];

    #[test]
    fn db_strings_round_trip() {
        for s in ALL {
            assert_eq!(WorkOrderState::from_db_str(s.as_db_str()), s);
        }
    }

    #[test]
    fn unknown_db_str_falls_back_to_draft() {
        assert_eq!(WorkOrderState::from_db_str("not-a-state"), WorkOrderState::Draft);
        // The fallback is never an execution state (defense-in-depth).
        assert!(!WorkOrderState::from_db_str("garbage").is_execution_state());
    }

    #[test]
    fn default_is_draft() {
        assert_eq!(WorkOrderState::default(), WorkOrderState::Draft);
    }

    #[test]
    fn json_serialisation_is_kebab_case() {
        let s = serde_json::to_string(&WorkOrderState::Dispatched).unwrap();
        assert_eq!(s, r#""dispatched""#);
        let parsed: WorkOrderState = serde_json::from_str(r#""reported""#).unwrap();
        assert_eq!(parsed, WorkOrderState::Reported);
        // Sanity: "request-changes" is an event_type, NOT a state — it must
        // fail to deserialize as a WorkOrderState.
        assert!(serde_json::from_str::<WorkOrderState>(r#""request-changes""#).is_err());
    }

    #[test]
    fn terminal_states_have_no_transitions() {
        assert!(legal_transitions_from(WorkOrderState::Completed).is_empty());
        assert!(legal_transitions_from(WorkOrderState::Cancelled).is_empty());
        assert!(WorkOrderState::Completed.is_terminal());
        assert!(WorkOrderState::Cancelled.is_terminal());
    }

    #[test]
    fn non_terminal_states_are_not_terminal() {
        for s in ALL {
            if matches!(s, WorkOrderState::Completed | WorkOrderState::Cancelled) {
                continue;
            }
            assert!(!s.is_terminal(), "{s:?} should not be terminal");
        }
    }

    #[test]
    fn execution_states_are_exactly_dispatch_and_after() {
        // draft / approved / cancelled are NOT execution states.
        assert!(!WorkOrderState::Draft.is_execution_state());
        assert!(!WorkOrderState::Approved.is_execution_state());
        assert!(!WorkOrderState::Cancelled.is_execution_state());
        // dispatched + everything reachable only through it IS.
        for s in [
            WorkOrderState::Dispatched,
            WorkOrderState::Claimed,
            WorkOrderState::Building,
            WorkOrderState::Verifying,
            WorkOrderState::Reported,
            WorkOrderState::Completed,
            WorkOrderState::Failed,
        ] {
            assert!(s.is_execution_state(), "{s:?} should be an execution state");
        }
    }

    #[test]
    fn approval_is_the_only_gate_into_execution() {
        // C22 inv. 1 structural check: the ONLY legal predecessor of the first
        // execution state (`dispatched`) is `approved`. No other state lists
        // `dispatched` as a legal target.
        for s in ALL {
            let reaches_dispatched =
                legal_transitions_from(s).contains(&WorkOrderState::Dispatched);
            assert_eq!(
                reaches_dispatched,
                s == WorkOrderState::Approved,
                "only `approved` may transition to `dispatched`, but {s:?} {}",
                if reaches_dispatched { "also does" } else { "does not" }
            );
        }
    }

    #[test]
    fn request_changes_and_retry_loops_exist() {
        // reported -> building (request-changes)
        assert!(is_legal_transition(WorkOrderState::Reported, WorkOrderState::Building));
        // failed -> approved (retry)
        assert!(is_legal_transition(WorkOrderState::Failed, WorkOrderState::Approved));
    }

    #[test]
    fn cancel_reachable_from_every_non_terminal_except_reported() {
        // C22 frozen table: `cancel` is legal from every non-terminal state
        // EXCEPT `reported`, whose owner exits are accept (-> completed),
        // request-changes (-> building) and reject (-> failed) — not cancel.
        for s in ALL {
            if s.is_terminal() {
                continue;
            }
            let cancellable = is_legal_transition(s, WorkOrderState::Cancelled);
            if s == WorkOrderState::Reported {
                assert!(!cancellable, "reported must NOT be directly cancellable (C22)");
            } else {
                assert!(cancellable, "{s:?} should be cancellable");
            }
        }
    }

    #[test]
    fn illegal_transitions_are_rejected() {
        // Can't skip approval: draft -> dispatched is illegal.
        assert!(!is_legal_transition(WorkOrderState::Draft, WorkOrderState::Dispatched));
        // Can't skip the runner lifecycle: approved -> building is illegal.
        assert!(!is_legal_transition(WorkOrderState::Approved, WorkOrderState::Building));
        // Can't resurrect a terminal: completed -> anything is illegal.
        for s in ALL {
            assert!(!is_legal_transition(WorkOrderState::Completed, s));
        }
    }
}
