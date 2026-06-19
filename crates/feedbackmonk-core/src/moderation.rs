//! Feedback **moderation** state machine — Contract C28 of the Public Feedback
//! Board plan. The public-visibility axis, ORTHOGONAL to the triage
//! [`crate::status::FeedbackStatus`] machine (which tracks workflow progress,
//! never public exposure).
//!
//! State machine ported in spirit from the work-order pattern
//! (`work_order.rs`) and the feedback status workflow (`status.rs`): the legal
//! transition table lives here, illegal transitions are rejected pre-DB-check,
//! and every status change writes both the `feedback.moderation_status` column
//! AND a `feedback_moderation_events` ledger row in one DB transaction (C28
//! Hard Invariant — the moderation handler enforces this in Stage 1).
//!
//! Stage 0 freezes the enum + transition table + the in-code visibility
//! predicate [`ModerationStatus::is_publicly_visible`]. The
//! `public-board-moderation-gate` Verification Oracle reads this module's
//! source (Probe A) to prove — structurally, not from a self-reported flag —
//! that **only `Approved` is publicly visible**. That is the moderation trust
//! boundary (FR-FBR-25a sibling): no feedback reaches a public surface without
//! a prior owner-authored `approve` event in the ledger.
//!
//! DB form (and JSON form) is `lowercase`, matching the `00016` CHECK
//! constraint byte-for-byte: `pending | approved | rejected`.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Public-visibility moderation status for a feedback row. DB/JSON form is
/// `lowercase`.
///
/// Variants:
/// - `Pending` — initial state for every newly submitted feedback row. NOT
///   publicly visible. Awaits owner review in the moderation queue.
/// - `Approved` — owner has approved public exposure. The ONLY state a public
///   board read ever returns (C29). Reaching it requires a recorded
///   owner-authored `approve` event (C28 invariant).
/// - `Rejected` — owner declined public exposure. NOT visible. Re-openable to
///   `Pending` (reconsider) or directly `Approved`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModerationStatus {
    #[default]
    Pending,
    Approved,
    Rejected,
}

impl ModerationStatus {
    /// DB-side string form. Must match the CHECK constraint in migration
    /// `00016_feedback_moderation.sql` byte-for-byte.
    #[must_use]
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
        }
    }

    /// Lenient DB-string parser. Unknown values fall back to `Pending` — the
    /// SAFE default (never publicly visible) — mirroring
    /// `FeedbackStatus::from_db_str`. The CHECK constraint guarantees only the
    /// three canonical values are ever stored.
    #[must_use]
    pub fn from_db_str(s: &str) -> Self {
        match s {
            "approved" => Self::Approved,
            "rejected" => Self::Rejected,
            _ => Self::Pending,
        }
    }

    /// The in-code visibility predicate — the structural moderation gate.
    ///
    /// **Only `Approved` is publicly visible.** A public-board read MUST never
    /// return a row for which this returns `false`. The
    /// `public-board-moderation-gate` oracle (Probe A) reads this function's
    /// source and fails if it ever classifies `Pending` or `Rejected` as
    /// visible, or stops classifying `Approved` as visible — exactly as the
    /// work-order oracle reads `is_execution_state`. This is the
    /// anti-reward-hacking anchor: it cannot be satisfied by a self-reported
    /// flag, and changing it requires a plan revision.
    #[must_use]
    pub fn is_publicly_visible(self) -> bool {
        matches!(self, Self::Approved)
    }
}

/// Legal moderation transitions out of a given state. Returned ordering is
/// stable so admin UIs can render buttons left-to-right without re-sorting.
///
/// - `Pending` → approve / reject.
/// - `Approved` → reject (pull from board + decline) / reset to pending (pull
///   from board, undecided).
/// - `Rejected` → approve (reconsider straight to visible) / reset to pending.
///
/// There is no terminal state: an owner can always re-moderate. A no-op
/// (`from == to`) is NOT a legal transition (the handler rejects it, mirroring
/// the triage machine).
#[must_use]
pub fn legal_moderation_transitions_from(s: ModerationStatus) -> &'static [ModerationStatus] {
    use ModerationStatus::{Approved, Pending, Rejected};
    match s {
        Pending => &[Approved, Rejected],
        Approved => &[Rejected, Pending],
        Rejected => &[Approved, Pending],
    }
}

/// The moderation event type implied by a target status — the `event_type`
/// written to `feedback_moderation_events`. Matches the `00016` CHECK set.
/// `Pending` as a target is a `reset` (un-approve / un-reject).
#[must_use]
pub fn event_type_for_target(to: ModerationStatus) -> &'static str {
    match to {
        ModerationStatus::Approved => "approve",
        ModerationStatus::Rejected => "reject",
        ModerationStatus::Pending => "reset",
    }
}

/// Errors emitted by Stage 1's moderation handler. Stage 0 freezes the variants
/// so the handler signature and the admin-UI error rendering implement against
/// the same surface.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ModerationError {
    #[error("illegal moderation transition from {from:?} to {to:?}")]
    IllegalTransition {
        from: ModerationStatus,
        to: ModerationStatus,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_strings_round_trip() {
        for s in [
            ModerationStatus::Pending,
            ModerationStatus::Approved,
            ModerationStatus::Rejected,
        ] {
            assert_eq!(ModerationStatus::from_db_str(s.as_db_str()), s);
        }
    }

    #[test]
    fn unknown_db_str_falls_back_to_pending() {
        // The SAFE default — an unrecognized value must NEVER be publicly visible.
        assert_eq!(
            ModerationStatus::from_db_str("not-a-status"),
            ModerationStatus::Pending
        );
        assert!(!ModerationStatus::from_db_str("not-a-status").is_publicly_visible());
    }

    #[test]
    fn default_is_pending_and_not_visible() {
        assert_eq!(ModerationStatus::default(), ModerationStatus::Pending);
        assert!(!ModerationStatus::default().is_publicly_visible());
    }

    #[test]
    fn only_approved_is_publicly_visible() {
        // The structural moderation gate. If this ever changes, the oracle
        // (Probe A) and this test both fail — by design.
        assert!(ModerationStatus::Approved.is_publicly_visible());
        assert!(!ModerationStatus::Pending.is_publicly_visible());
        assert!(!ModerationStatus::Rejected.is_publicly_visible());
    }

    #[test]
    fn pending_can_approve_or_reject() {
        assert_eq!(
            legal_moderation_transitions_from(ModerationStatus::Pending),
            &[ModerationStatus::Approved, ModerationStatus::Rejected]
        );
    }

    #[test]
    fn approved_can_be_pulled_from_board() {
        // Approved -> Rejected (decline) or Approved -> Pending (undecided).
        let targets = legal_moderation_transitions_from(ModerationStatus::Approved);
        assert!(targets.contains(&ModerationStatus::Rejected));
        assert!(targets.contains(&ModerationStatus::Pending));
        assert!(!targets.contains(&ModerationStatus::Approved), "no self-transition");
    }

    #[test]
    fn rejected_can_be_reconsidered() {
        let targets = legal_moderation_transitions_from(ModerationStatus::Rejected);
        assert!(targets.contains(&ModerationStatus::Approved));
        assert!(targets.contains(&ModerationStatus::Pending));
    }

    #[test]
    fn no_self_transition_is_legal() {
        for s in [
            ModerationStatus::Pending,
            ModerationStatus::Approved,
            ModerationStatus::Rejected,
        ] {
            assert!(
                !legal_moderation_transitions_from(s).contains(&s),
                "{s:?} must not list itself as a legal target"
            );
        }
    }

    #[test]
    fn event_type_matches_target() {
        assert_eq!(event_type_for_target(ModerationStatus::Approved), "approve");
        assert_eq!(event_type_for_target(ModerationStatus::Rejected), "reject");
        assert_eq!(event_type_for_target(ModerationStatus::Pending), "reset");
    }

    #[test]
    fn json_serialisation_is_lowercase() {
        let s = serde_json::to_string(&ModerationStatus::Approved).unwrap();
        assert_eq!(s, r#""approved""#);
        let parsed: ModerationStatus = serde_json::from_str(r#""approved""#).unwrap();
        assert_eq!(parsed, ModerationStatus::Approved);
    }
}
