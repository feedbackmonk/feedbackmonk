//! Recommendation / work-order action type — Contract C23 of the P5a plan.
//!
//! `ActionType` classifies what an actionable cluster's recommendation asks
//! for. It is DISTINCT from `FeedbackKind` (the submission taxonomy): a `bug`
//! cluster maps to a `bug_fix` action, a `question` cluster to `investigation`
//! or `no_action`. The mapping is provided by [`ActionType::from_feedback_kind`]
//! as a deterministic default; the analyst (P5b) may override it per cluster.
//!
//! The DB form (and JSON form) is `snake_case` — it must match the
//! `action_type` CHECK constraint in migrations `00013_feedback_clusters.sql`
//! (recommendations) and `00014_work_orders.sql` (work_orders) byte-for-byte.

use serde::{Deserialize, Serialize};

use crate::models::FeedbackKind;

/// The action a recommendation / work order asks for.
///
/// Variants:
/// - `BugFix` — fix a defect.
/// - `FeatureImplementation` — build a net-new capability.
/// - `Enhancement` — improve an existing capability.
/// - `Investigation` — needs research before an action can be decided.
/// - `NoAction` — acknowledged, nothing to build (the inert default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    BugFix,
    FeatureImplementation,
    Enhancement,
    Investigation,
    /// Default + lenient-parse fallback: the inert action. Choosing the inert
    /// variant on an unknown DB string means a malformed value can never
    /// silently escalate into an executable action class.
    #[default]
    NoAction,
}

impl ActionType {
    /// DB-side string form. Must match the `action_type` CHECK constraints in
    /// migrations 00013/00014 byte-for-byte.
    #[must_use]
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::BugFix => "bug_fix",
            Self::FeatureImplementation => "feature_implementation",
            Self::Enhancement => "enhancement",
            Self::Investigation => "investigation",
            Self::NoAction => "no_action",
        }
    }

    /// Lenient DB-string parser. Unknown values fall back to `NoAction` (the
    /// inert variant) to mirror `FeedbackKind::from_db_str`; the CHECK
    /// constraint guarantees only the five canonical values are ever stored.
    #[must_use]
    pub fn from_db_str(s: &str) -> Self {
        match s {
            "bug_fix" => Self::BugFix,
            "feature_implementation" => Self::FeatureImplementation,
            "enhancement" => Self::Enhancement,
            "investigation" => Self::Investigation,
            _ => Self::NoAction,
        }
    }

    /// Deterministic default mapping from a cluster's `FeedbackKind` to an
    /// action type. Used when a cluster is first formed (FR-FBR-19); the deep
    /// analyst sweep (P5b) may revise it. `Other` maps to `NoAction` so an
    /// unclassified cluster never defaults into an executable action class.
    #[must_use]
    pub fn from_feedback_kind(kind: FeedbackKind) -> Self {
        match kind {
            FeedbackKind::Bug => Self::BugFix,
            FeedbackKind::Feature => Self::FeatureImplementation,
            FeedbackKind::Question => Self::Investigation,
            FeedbackKind::Other => Self::NoAction,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_strings_round_trip() {
        for a in [
            ActionType::BugFix,
            ActionType::FeatureImplementation,
            ActionType::Enhancement,
            ActionType::Investigation,
            ActionType::NoAction,
        ] {
            assert_eq!(ActionType::from_db_str(a.as_db_str()), a);
        }
    }

    #[test]
    fn unknown_db_str_falls_back_to_no_action() {
        assert_eq!(ActionType::from_db_str("not-an-action"), ActionType::NoAction);
    }

    #[test]
    fn default_is_no_action() {
        assert_eq!(ActionType::default(), ActionType::NoAction);
    }

    #[test]
    fn json_serialisation_is_snake_case() {
        let s = serde_json::to_string(&ActionType::FeatureImplementation).unwrap();
        assert_eq!(s, r#""feature_implementation""#);
        let parsed: ActionType = serde_json::from_str(r#""bug_fix""#).unwrap();
        assert_eq!(parsed, ActionType::BugFix);
    }

    #[test]
    fn feedback_kind_mapping_is_deterministic() {
        assert_eq!(ActionType::from_feedback_kind(FeedbackKind::Bug), ActionType::BugFix);
        assert_eq!(
            ActionType::from_feedback_kind(FeedbackKind::Feature),
            ActionType::FeatureImplementation
        );
        assert_eq!(
            ActionType::from_feedback_kind(FeedbackKind::Question),
            ActionType::Investigation
        );
        // Other never defaults into an executable action class.
        assert_eq!(ActionType::from_feedback_kind(FeedbackKind::Other), ActionType::NoAction);
    }
}
