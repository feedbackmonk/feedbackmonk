//! Board access, the ranking pool and a viewer's own board state — the pure
//! domain half of end-user prioritization (FR-FBR-43/46/48/51, DEC-FBR-18..21).
//!
//! No DB, no I/O. The repository (`feedbackmonk-repository::board_signals`)
//! enforces the same rules in SQL; this module is where the rules are named so
//! the API, the repository and the tests all point at one definition.
//!
//! Contract C43 (public wire) / C44 (owner wire):
//! `docs/planning/handoffs/prioritization-contracts.md`.

use serde::{Deserialize, Serialize};

use crate::{FeedbackKind, FeedbackStatus, ModerationStatus};

/// Who may see a project's board (FR-FBR-43, migration 00033). DB and JSON form
/// is `snake_case`, byte-for-byte with the `projects.board_access` CHECK.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoardAccess {
    /// Anyone may read the board and upvote anonymously (today's behaviour).
    #[default]
    Public,
    /// Every board read and write needs a signed-in viewer; no anonymous vote
    /// is accepted.
    SignedIn,
}

impl BoardAccess {
    /// DB string form; matches the 00033 CHECK constraint.
    #[must_use]
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::SignedIn => "signed_in",
        }
    }

    /// Lenient DB parser. An unknown value falls back to `SignedIn`, the SAFE
    /// default: a drifted row shows its board to fewer people, never more.
    #[must_use]
    pub fn from_db_str(s: &str) -> Self {
        match s {
            "public" => Self::Public,
            _ => Self::SignedIn,
        }
    }

    /// Strict parser for request input (`None` on anything unknown).
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "public" => Some(Self::Public),
            "signed_in" => Some(Self::SignedIn),
            _ => None,
        }
    }
}

/// Triage statuses that take an item OUT of the ranking pool (FR-FBR-51,
/// DEC-FBR-19). Promotion to the roadmap sets `duplicate`, so a promoted item
/// leaves the pool by this rule.
pub const RANKING_POOL_EXCLUDED_STATUSES: [FeedbackStatus; 3] = [
    FeedbackStatus::Shipped,
    FeedbackStatus::WontFix,
    FeedbackStatus::Duplicate,
];

/// The ranking-pool predicate (FR-FBR-51): approved, of kind `feature`, and not
/// shipped, wontfix or duplicate. The repository's SQL pool filter is the same
/// rule; `board_signals` repository tests pin the two together.
#[must_use]
pub fn in_ranking_pool(
    moderation: ModerationStatus,
    kind: FeedbackKind,
    status: FeedbackStatus,
) -> bool {
    moderation == ModerationStatus::Approved
        && kind == FeedbackKind::Feature
        && !RANKING_POOL_EXCLUDED_STATUSES.contains(&status)
}

/// The longest a board session may live (FR-FBR-44, DEC-FBR-21): 8 hours from
/// the exchange, never extended. Migration 00034 enforces the same ceiling.
pub const BOARD_SESSION_MAX_AGE_SECS: i64 = 8 * 60 * 60;

/// A signed-in viewer's OWN state for one board item (FR-FBR-46). Carried on
/// board list and detail responses for a signed-in viewer only; an anonymous
/// viewer receives no such object. Never another user's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ViewerItemState {
    /// The viewer has upvoted this item.
    pub upvoted: bool,
    /// The viewer has marked this item not needed (FR-FBR-48).
    pub not_needed: bool,
    /// 0-based position in the viewer's priority list, if the item is in it.
    pub priority_position: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_access_db_round_trip_and_safe_fallback() {
        for a in [BoardAccess::Public, BoardAccess::SignedIn] {
            assert_eq!(BoardAccess::from_db_str(a.as_db_str()), a);
            assert_eq!(BoardAccess::parse(a.as_db_str()), Some(a));
        }
        assert_eq!(BoardAccess::from_db_str("garbage"), BoardAccess::SignedIn);
        assert_eq!(BoardAccess::parse("garbage"), None);
        assert_eq!(BoardAccess::default(), BoardAccess::Public);
        assert_eq!(
            serde_json::to_string(&BoardAccess::SignedIn).unwrap(),
            "\"signed_in\""
        );
    }

    #[test]
    fn ranking_pool_is_approved_open_features_only() {
        use FeedbackStatus as S;
        let open = [S::Submitted, S::Triaged, S::InProgress];
        for s in open {
            assert!(in_ranking_pool(ModerationStatus::Approved, FeedbackKind::Feature, s));
        }
        for s in RANKING_POOL_EXCLUDED_STATUSES {
            assert!(!in_ranking_pool(ModerationStatus::Approved, FeedbackKind::Feature, s));
        }
        for m in [ModerationStatus::Pending, ModerationStatus::Rejected] {
            assert!(!in_ranking_pool(m, FeedbackKind::Feature, S::Submitted));
        }
        for k in [FeedbackKind::Bug, FeedbackKind::Question, FeedbackKind::Other] {
            assert!(!in_ranking_pool(ModerationStatus::Approved, k, S::Submitted));
        }
    }
}
