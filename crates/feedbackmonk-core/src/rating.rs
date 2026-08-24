//! First-class **rating** — an optional 1–5 satisfaction score on a feedback
//! submission, and the finer-grained sibling of [`crate::sentiment::Sentiment`].
//!
//! # Why a separate field rather than a wider `Sentiment`
//!
//! The obvious way to offer a 5-point scale is to widen `Sentiment` from three
//! variants to five. That was deliberately NOT done:
//!
//! - `sentiment` is a **published wire enum** (`negative | neutral | positive`)
//!   on a public submit API with more than one consumer (the embeddable widget,
//!   the admin UI, and each adopting product). Widening it is a breaking change
//!   for every one of them.
//! - Every already-stored row carries the 3-point value, and the admin
//!   sentiment-trend aggregation emits a fixed three-bucket shape. Widening the
//!   enum strands that history and forces a backfill whose mapping is a guess.
//!
//! So `rating` is **additive**: a submission may carry a rating, a sentiment,
//! both, or neither. When a rating is present and a sentiment is not, the
//! sentiment is **derived** from it via [`Rating::to_sentiment`] at the API
//! boundary — so a 5-point client keeps populating the 3-point column, every
//! existing reader keeps working unchanged, and no backfill is needed.
//!
//! The derivation is deliberately lossy and one-way: 1–2 ⇒ negative, 3 ⇒
//! neutral, 4–5 ⇒ positive. The full-resolution value stays in `rating`.
//!
//! DB form is a `SMALLINT` with a `1..=5` CHECK (migration `00029`); JSON form
//! is a plain integer.

use serde::{Deserialize, Serialize};

use crate::sentiment::Sentiment;

/// A 1–5 satisfaction score. Construct via [`Rating::new`] — the invariant
/// (`1 <= value <= 5`) is enforced there and mirrored by the DB CHECK, so an
/// out-of-range value can never reach storage.
///
/// There is deliberately NO `Default`: a rating is optional (a feedback row may
/// have none), so absence is modeled as `Option<Rating>` rather than a sentinel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "i16", into = "i16")]
pub struct Rating(i16);

impl Rating {
    /// Lowest valid rating.
    pub const MIN: i16 = 1;
    /// Highest valid rating.
    pub const MAX: i16 = 5;

    /// Every valid rating, ascending. Used by aggregations that want a stable,
    /// fully-populated (zero-filled) bucket shape, mirroring `Sentiment::ALL`.
    pub const ALL: [Rating; 5] = [Self(1), Self(2), Self(3), Self(4), Self(5)];

    /// Construct a rating, returning `None` when `value` is outside `1..=5`.
    /// The API boundary maps `None` to a `400`.
    #[must_use]
    pub fn new(value: i16) -> Option<Self> {
        (Self::MIN..=Self::MAX).contains(&value).then_some(Self(value))
    }

    /// The underlying 1–5 score.
    #[must_use]
    pub fn value(self) -> i16 {
        self.0
    }

    /// Collapse to the 3-point [`Sentiment`] scale: 1–2 ⇒ `Negative`, 3 ⇒
    /// `Neutral`, 4–5 ⇒ `Positive`.
    ///
    /// This is what keeps the existing `sentiment` column, its trend
    /// aggregation, and every existing consumer working when a client submits
    /// only a rating. Lossy and one-way by design — there is no inverse, since
    /// a `Positive` could have been a 4 or a 5.
    #[must_use]
    pub fn to_sentiment(self) -> Sentiment {
        match self.0 {
            1 | 2 => Sentiment::Negative,
            3 => Sentiment::Neutral,
            _ => Sentiment::Positive,
        }
    }
}

impl TryFrom<i16> for Rating {
    type Error = RatingOutOfRange;

    fn try_from(value: i16) -> Result<Self, Self::Error> {
        Self::new(value).ok_or(RatingOutOfRange(value))
    }
}

impl From<Rating> for i16 {
    fn from(r: Rating) -> Self {
        r.0
    }
}

/// Returned when a value outside `1..=5` is offered as a [`Rating`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RatingOutOfRange(pub i16);

impl std::fmt::Display for RatingOutOfRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "rating must be between 1 and 5; got {}", self.0)
    }
}

impl std::error::Error for RatingOutOfRange {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_whole_valid_range_and_nothing_else() {
        for v in 1..=5 {
            assert_eq!(Rating::new(v).map(Rating::value), Some(v));
        }
        for v in [i16::MIN, -1, 0, 6, 7, i16::MAX] {
            assert!(Rating::new(v).is_none(), "{v} should be rejected");
        }
    }

    #[test]
    fn derives_the_documented_sentiment_buckets() {
        let got: Vec<Sentiment> = (1..=5)
            .map(|v| Rating::new(v).unwrap().to_sentiment())
            .collect();
        assert_eq!(
            got,
            vec![
                Sentiment::Negative,
                Sentiment::Negative,
                Sentiment::Neutral,
                Sentiment::Positive,
                Sentiment::Positive,
            ]
        );
    }

    #[test]
    fn every_rating_derives_some_sentiment() {
        // The derivation is total over the valid range — no rating is
        // unmappable, which is what lets the API always populate `sentiment`.
        for r in Rating::ALL {
            let _ = r.to_sentiment();
        }
        assert_eq!(Rating::ALL.len(), (Rating::MAX - Rating::MIN + 1) as usize);
    }

    #[test]
    fn json_form_is_a_plain_integer() {
        let r = Rating::new(4).unwrap();
        assert_eq!(serde_json::to_string(&r).unwrap(), "4");
        let parsed: Rating = serde_json::from_str("2").unwrap();
        assert_eq!(parsed, Rating::new(2).unwrap());
    }

    #[test]
    fn json_rejects_out_of_range() {
        assert!(serde_json::from_str::<Rating>("0").is_err());
        assert!(serde_json::from_str::<Rating>("6").is_err());
    }

    #[test]
    fn ordering_follows_the_scale() {
        assert!(Rating::new(1).unwrap() < Rating::new(5).unwrap());
    }
}
