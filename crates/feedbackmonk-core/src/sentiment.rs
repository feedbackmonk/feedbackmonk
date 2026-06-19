//! First-class **sentiment** on a feedback submission (Capability 1 of the
//! GitCellar in-app solicitation request; FR-FBR-28).
//!
//! A 3-point satisfaction signal captured at submit time, ORTHOGONAL to both
//! the triage [`crate::status::FeedbackStatus`] machine (workflow progress) and
//! the [`crate::moderation::ModerationStatus`] machine (public exposure).
//! Sentiment is OPTIONAL: a submission may carry a body, a sentiment, or both —
//! a sentiment-only submission (no body) is valid (the one-tap signal from
//! users who won't type). The "body OR sentiment present" invariant is enforced
//! at the DB layer (migration `00017`, `feedback_body_or_sentiment_check`) and
//! at the API boundary (`handlers::feedback::submit`).
//!
//! DB form (and JSON form) is `lowercase`, matching the `00017` CHECK
//! constraint byte-for-byte: `negative | neutral | positive`. The order of the
//! variants is the natural satisfaction ordering (negative → positive) so
//! aggregations can rely on it.

use serde::{Deserialize, Serialize};

/// A 3-point satisfaction signal on a feedback row. DB/JSON form is `lowercase`.
///
/// There is deliberately NO `Default`: sentiment is optional (a feedback row may
/// have none), so absence is modeled as `Option<Sentiment>` rather than a
/// sentinel variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sentiment {
    Negative,
    Neutral,
    Positive,
}

impl Sentiment {
    /// Every variant, in natural satisfaction order. Used by aggregations to
    /// emit a stable, fully-populated bucket shape (zero-filled).
    pub const ALL: [Sentiment; 3] = [Self::Negative, Self::Neutral, Self::Positive];

    /// DB-side string form. Must match the CHECK constraint in migration
    /// `00017_feedback_sentiment_and_solicitation.sql` byte-for-byte.
    #[must_use]
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Negative => "negative",
            Self::Neutral => "neutral",
            Self::Positive => "positive",
        }
    }

    /// Parse a client-supplied or DB string. Returns `None` for any
    /// unrecognized value — the API boundary maps `None` to a `400`, and the DB
    /// CHECK guarantees only the three canonical values are ever stored (so a
    /// read-path `None` only ever means the column was NULL).
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "negative" => Some(Self::Negative),
            "neutral" => Some(Self::Neutral),
            "positive" => Some(Self::Positive),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_strings_round_trip() {
        for s in Sentiment::ALL {
            assert_eq!(Sentiment::parse(s.as_db_str()), Some(s));
        }
    }

    #[test]
    fn unknown_value_is_none() {
        assert_eq!(Sentiment::parse("meh"), None);
        assert_eq!(Sentiment::parse(""), None);
        assert_eq!(Sentiment::parse("POSITIVE"), None);
    }

    #[test]
    fn json_serialisation_is_lowercase() {
        assert_eq!(serde_json::to_string(&Sentiment::Positive).unwrap(), r#""positive""#);
        let parsed: Sentiment = serde_json::from_str(r#""negative""#).unwrap();
        assert_eq!(parsed, Sentiment::Negative);
    }

    #[test]
    fn all_is_in_satisfaction_order() {
        assert_eq!(
            Sentiment::ALL,
            [Sentiment::Negative, Sentiment::Neutral, Sentiment::Positive]
        );
    }
}
