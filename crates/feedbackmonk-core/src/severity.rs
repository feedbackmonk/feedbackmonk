//! First-class **severity** on a feedback submission (GitCellar parity,
//! Phase A; replaces the `external_metadata.severity` side-channel).
//!
//! A 4-point impact signal supplied by the submitter (or the embedding
//! product) at submit time, ORTHOGONAL to both the triage
//! [`crate::status::FeedbackStatus`] machine (workflow progress) and the
//! [`crate::moderation::ModerationStatus`] machine (public exposure).
//! Severity is OPTIONAL and tenant-generic: a submission may carry one or not
//! — absence is modeled as `Option<Severity>`, never a sentinel variant.
//!
//! DB form (and JSON form) is `lowercase`, matching the `00020` CHECK
//! constraint byte-for-byte: `low | medium | high | blocker`. The order of the
//! variants is the natural impact ordering (low → blocker) so aggregations can
//! rely on it.

use serde::{Deserialize, Serialize};

/// A 4-point impact signal on a feedback row. DB/JSON form is `lowercase`.
///
/// There is deliberately NO `Default`: severity is optional (a feedback row may
/// have none), so absence is modeled as `Option<Severity>` rather than a
/// sentinel variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Low,
    Medium,
    High,
    Blocker,
}

impl Severity {
    /// Every variant, in natural impact order. Used by aggregations to emit a
    /// stable, fully-populated bucket shape (zero-filled).
    pub const ALL: [Severity; 4] = [Self::Low, Self::Medium, Self::High, Self::Blocker];

    /// DB-side string form. Must match the CHECK constraint in migration
    /// `00020_feedback_severity.sql` byte-for-byte.
    #[must_use]
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Blocker => "blocker",
        }
    }

    /// Parse a client-supplied or DB string. Returns `None` for any
    /// unrecognized value — the API boundary maps `None` to a `400`, and the DB
    /// CHECK guarantees only the four canonical values are ever stored (so a
    /// read-path `None` only ever means the column was NULL).
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            "blocker" => Some(Self::Blocker),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_strings_round_trip() {
        for s in Severity::ALL {
            assert_eq!(Severity::parse(s.as_db_str()), Some(s));
        }
    }

    #[test]
    fn unknown_value_is_none() {
        assert_eq!(Severity::parse("critical"), None);
        assert_eq!(Severity::parse(""), None);
        assert_eq!(Severity::parse("HIGH"), None);
    }

    #[test]
    fn json_serialisation_is_lowercase() {
        assert_eq!(serde_json::to_string(&Severity::Blocker).unwrap(), r#""blocker""#);
        let parsed: Severity = serde_json::from_str(r#""low""#).unwrap();
        assert_eq!(parsed, Severity::Low);
    }

    #[test]
    fn all_is_in_impact_order() {
        assert_eq!(
            Severity::ALL,
            [Severity::Low, Severity::Medium, Severity::High, Severity::Blocker]
        );
    }
}
