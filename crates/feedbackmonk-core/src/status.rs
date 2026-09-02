//! Feedback status workflow — Contract C6 of the P1 plan.
//!
//! State machine ported from `gitcellar-cloud/src/feedback/db.rs` (DEC-FBR-07:
//! GitCellar is a read-only reference). The variants match the
//! `feedback.status` CHECK constraint from migration 00003; the DB-side
//! string form is `kebab-case` (e.g. `in-progress`) so JSON serialisation
//! and SQL serialisation share one representation.
//!
//! Stage 1 freezes the enum + transition table. Stage 2 Worker A implements
//! the transition handler (the function that takes a `&ProjectScope`, a
//! `FeedbackId`, a target state, and an audit-row context, then writes both
//! the `feedback.status` column AND a `feedback_status_history` row in the
//! same DB transaction — Contract C6 Hard Invariant #4).

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ids::FeedbackId;

/// Feedback status. The DB form (and JSON form) is `kebab-case`.
///
/// Variants:
/// - `Submitted` — initial state for every newly accepted feedback row.
/// - `Triaged` — admin has reviewed; not yet started.
/// - `InProgress` — work underway; visible to submitter.
/// - `Shipped` — terminal positive state.
/// - `WontFix` — closed, not actioned. Re-openable to `Submitted`.
/// - `Duplicate` — closed, points at another feedback via the audit row's
///   `duplicate_of_feedback_id`. Un-mergeable back to `Submitted`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FeedbackStatus {
    #[default]
    Submitted,
    Triaged,
    // `kebab-case` produces `in-progress` for this variant — matches the
    // migration 00003 CHECK constraint byte-for-byte.
    InProgress,
    Shipped,
    // `kebab-case` would produce `wont-fix` for this variant, but Contract
    // C6 + migration 00003 both store `wontfix` (no hyphen). Explicit
    // rename keeps DB <-> JSON byte-equivalent, exactly as
    // `RoadmapItemStatus::WontFix` does. Without it the admin UI receives a
    // status string outside its six-value union: the status badge renders
    // blank and `LEGAL_TRANSITIONS[status]` is `undefined`, white-screening
    // the feedback drawer.
    //
    // `alias` accepts the old broken spelling on INPUT only — it is never
    // emitted. Between the introduction of this enum and 2026-09-01 the API
    // accepted `wont-fix` and nothing else, so any caller that ever set a row
    // to won't-fix was necessarily sending the hyphenated form; dropping it
    // outright would 422 that caller the moment the fix deploys. Cheap
    // insurance, and it costs nothing semantically. Safe to delete once no
    // client is known to send `wont-fix`.
    #[serde(rename = "wontfix", alias = "wont-fix")]
    WontFix,
    Duplicate,
}

impl FeedbackStatus {
    /// DB-side string form. Must match the CHECK constraint in migration
    /// `00003_feedback_status_history.sql` byte-for-byte.
    #[must_use]
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Submitted => "submitted",
            Self::Triaged => "triaged",
            Self::InProgress => "in-progress",
            Self::Shipped => "shipped",
            Self::WontFix => "wontfix",
            Self::Duplicate => "duplicate",
        }
    }

    /// Lenient DB-string parser. Unknown values fall back to `Submitted`
    /// to mirror `FeedbackKind::from_db_str`; the CHECK constraint
    /// guarantees only the six canonical values are ever stored.
    #[must_use]
    pub fn from_db_str(s: &str) -> Self {
        match s {
            "triaged" => Self::Triaged,
            "in-progress" => Self::InProgress,
            "shipped" => Self::Shipped,
            "wontfix" => Self::WontFix,
            "duplicate" => Self::Duplicate,
            _ => Self::Submitted,
        }
    }
}

/// Legal transitions out of a given state. Returned ordering is stable so
/// admin UIs can render buttons left-to-right without re-sorting.
///
/// `Shipped` is terminal. `WontFix` and `Duplicate` each have one re-open
/// path back to `Submitted` (admin un-close / un-merge).
#[must_use]
pub fn legal_transitions_from(s: FeedbackStatus) -> &'static [FeedbackStatus] {
    use FeedbackStatus::{Duplicate, InProgress, Shipped, Submitted, Triaged, WontFix};
    match s {
        Submitted => &[Triaged, WontFix, Duplicate],
        Triaged => &[InProgress, WontFix, Duplicate, Submitted],
        InProgress => &[Shipped, WontFix, Duplicate, Triaged],
        Shipped => &[],
        WontFix => &[Submitted],
        Duplicate => &[Submitted],
    }
}

/// Errors emitted by Stage 2 Worker A's transition handler. Stage 1 freezes
/// the variants so Worker A's signature and Worker B's UI error-rendering
/// can be implemented against the same surface.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TransitionError {
    #[error("illegal transition from {from:?} to {to:?}")]
    IllegalTransition {
        from: FeedbackStatus,
        to: FeedbackStatus,
    },
    /// Target status is `Duplicate` but caller did not supply
    /// `duplicate_of: Some(FeedbackId)`.
    #[error("transition to Duplicate requires a duplicate_of target")]
    DuplicateRequiresTarget,
    /// `duplicate_of` references a feedback row not present in the same
    /// `ProjectScope` (cross-tenant or cross-project leak attempt).
    #[error("duplicate target feedback id is missing or out of scope")]
    DuplicateTargetMissing,
    /// `duplicate_of == feedback_id` — a feedback row can't be its own
    /// duplicate. The DB enforces this via the
    /// `feedback_status_history_no_self_duplicate` CHECK.
    #[error("duplicate target equals the source feedback id")]
    DuplicateSelfReference { feedback_id: FeedbackId },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_strings_round_trip() {
        for s in [
            FeedbackStatus::Submitted,
            FeedbackStatus::Triaged,
            FeedbackStatus::InProgress,
            FeedbackStatus::Shipped,
            FeedbackStatus::WontFix,
            FeedbackStatus::Duplicate,
        ] {
            assert_eq!(FeedbackStatus::from_db_str(s.as_db_str()), s);
        }
    }

    #[test]
    fn unknown_db_str_falls_back_to_submitted() {
        assert_eq!(
            FeedbackStatus::from_db_str("not-a-status"),
            FeedbackStatus::Submitted
        );
    }

    #[test]
    fn shipped_is_terminal() {
        assert!(legal_transitions_from(FeedbackStatus::Shipped).is_empty());
    }

    #[test]
    fn submitted_has_three_targets() {
        assert_eq!(legal_transitions_from(FeedbackStatus::Submitted).len(), 3);
    }

    #[test]
    fn wontfix_can_be_reopened() {
        assert_eq!(
            legal_transitions_from(FeedbackStatus::WontFix),
            &[FeedbackStatus::Submitted]
        );
    }

    #[test]
    fn duplicate_can_be_unmerged() {
        assert_eq!(
            legal_transitions_from(FeedbackStatus::Duplicate),
            &[FeedbackStatus::Submitted]
        );
    }

    #[test]
    fn json_serialisation_is_kebab_case() {
        let s = serde_json::to_string(&FeedbackStatus::InProgress).unwrap();
        assert_eq!(s, r#""in-progress""#);
        let parsed: FeedbackStatus = serde_json::from_str(r#""in-progress""#).unwrap();
        assert_eq!(parsed, FeedbackStatus::InProgress);
    }

    /// The wire form and the DB form MUST be the same string for every
    /// variant. `WontFix` is the trap: `rename_all = "kebab-case"` emits
    /// `wont-fix` while migration 00003's CHECK constraint (and every client
    /// status union) uses `wontfix`. The old single-variant test above only
    /// covered `InProgress`, so the drift shipped: a `wontfix` row serialised
    /// as `wont-fix`, which the admin UI could neither label nor look up in
    /// its transition table (white screen on the feedback drawer), and a
    /// `?status=wontfix` filter or `to_status: "wontfix"` transition was
    /// rejected as an unknown variant. Assert all six, both directions.
    #[test]
    fn json_form_matches_db_form_for_every_variant() {
        for s in [
            FeedbackStatus::Submitted,
            FeedbackStatus::Triaged,
            FeedbackStatus::InProgress,
            FeedbackStatus::Shipped,
            FeedbackStatus::WontFix,
            FeedbackStatus::Duplicate,
        ] {
            let json = serde_json::to_string(&s).unwrap();
            assert_eq!(
                json,
                format!("\"{}\"", s.as_db_str()),
                "JSON form of {s:?} must equal its DB form"
            );
            let parsed: FeedbackStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, s, "{json} must deserialise back to {s:?}");
        }
    }

    /// The pre-fix wire spelling stays accepted on input so the deploy of the
    /// fix cannot 422 a caller that was speaking the old form — but it must
    /// never be produced. Both halves matter; asserting only the first would
    /// let the bug back in.
    #[test]
    fn legacy_wont_fix_spelling_is_accepted_but_never_emitted() {
        let parsed: FeedbackStatus = serde_json::from_str(r#""wont-fix""#).unwrap();
        assert_eq!(parsed, FeedbackStatus::WontFix);
        assert_eq!(
            serde_json::to_string(&FeedbackStatus::WontFix).unwrap(),
            r#""wontfix""#
        );
    }
}
