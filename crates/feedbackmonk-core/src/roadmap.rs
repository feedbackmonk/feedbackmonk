//! Roadmap domain types — Contracts C13 + C14 of the P2 plan.
//!
//! Mirrors the schemas in `migrations/00006_roadmap_items.sql` +
//! `migrations/00007_roadmap_votes.sql`. The DB-side form is `kebab-case`
//! for status (`in-progress`, `wontfix`) and `lowercase` for voter mode
//! (`jwt`, `anon`) — matching the CHECK constraints.
//!
//! No DB access, no async — pure data + value-construction helpers.
//! The DB-touching layer lives in `feedbackmonk-repository::roadmap_items` +
//! `roadmap_votes`; the HTTP layer in `feedbackmonk-api::handlers::roadmap`.
//!
//! Lineage:
//!   FR-FBR-11 (public roadmap)
//!   FR-FBR-13 (voting + aggregator)
//!   Contracts C13 + C14 (P2 plan §Interface Contracts)
//!   docs/planning/handoffs/p2-fanout-contracts.md §C13 + §C14

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// RoadmapItem + RoadmapItemStatus  (Contract C13)
// ---------------------------------------------------------------------------

/// Roadmap-item status. Five variants. The DB form (and JSON form) is
/// kebab-case, matching the `roadmap_items.status` CHECK constraint in
/// migration `00006_roadmap_items.sql`.
///
/// State machine (admin-managed; no audit-history table for v1):
/// - `Considering → Planned → InProgress → Shipped` (forward path)
/// - any → `WontFix` (close)
/// - `WontFix → Considering` (re-open via admin edit)
/// - `Shipped` is terminal
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RoadmapItemStatus {
    #[default]
    Considering,
    Planned,
    // `kebab-case` produces `in-progress` for this variant — matches the
    // schema CHECK constraint byte-for-byte.
    InProgress,
    Shipped,
    // `kebab-case` would produce `wont-fix` for this variant, but Contract
    // C13 + migration 00006 both store `wontfix` (no hyphen). Explicit
    // rename keeps DB <-> JSON byte-equivalent.
    #[serde(rename = "wontfix")]
    WontFix,
}

impl RoadmapItemStatus {
    /// DB-side string form. Must match the migration 00006 CHECK constraint
    /// byte-for-byte.
    #[must_use]
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Considering => "considering",
            Self::Planned => "planned",
            Self::InProgress => "in-progress",
            Self::Shipped => "shipped",
            Self::WontFix => "wontfix",
        }
    }

    /// Lenient parser — unknown values fall back to `Considering`. The
    /// CHECK constraint guarantees only the five canonical values are ever
    /// stored, so this branch should be unreachable in production. Mirrors
    /// `FeedbackStatus::from_db_str`'s policy.
    #[must_use]
    pub fn from_db_str(s: &str) -> Self {
        match s {
            "planned" => Self::Planned,
            "in-progress" => Self::InProgress,
            "shipped" => Self::Shipped,
            "wontfix" => Self::WontFix,
            _ => Self::Considering,
        }
    }

    /// True if this status should be visible on the unauthenticated public
    /// roadmap endpoint. v1 returns all five — there is no separate "draft"
    /// state. Kept as a method so a future "hide wontfix from public" knob
    /// has one chokepoint to flip.
    #[must_use]
    pub fn is_public_visible(self) -> bool {
        true
    }

    /// Iterator over all variants in stable display order (for UI rendering
    /// and for tests that round-trip every variant).
    pub fn all() -> &'static [Self] {
        &[
            Self::Considering,
            Self::Planned,
            Self::InProgress,
            Self::Shipped,
            Self::WontFix,
        ]
    }
}

/// A row of `roadmap_items` (Contract C13). The vote-count aggregate lives
/// on response shapes in the API layer, not on this row type — the cache
/// joins it in at read time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoadmapItem {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub slug: String,
    pub title: String,
    pub body: String,
    pub status: RoadmapItemStatus,
    pub origin_feedback_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub created_by: Uuid,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// RoadmapVote + RoadmapVoterMode  (Contract C14)
// ---------------------------------------------------------------------------

/// How the `voter_id` was obtained — either from a verified JWT `sub` claim
/// (auth mode) or from `AnonGate::token_hash(ip, cookie, project_id)`
/// (anon mode). Matches the `roadmap_votes.voter_mode` CHECK constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoadmapVoterMode {
    Jwt,
    Anon,
}

impl RoadmapVoterMode {
    #[must_use]
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Jwt => "jwt",
            Self::Anon => "anon",
        }
    }

    /// Lenient parser — unknown values fall back to `Anon` (the more
    /// conservative default; an unrecognized voter that we still want to
    /// rate-limit lands in the anon bucket). Schema CHECK enforces the
    /// two canonical values so the fallback is defense-in-depth.
    #[must_use]
    pub fn from_db_str(s: &str) -> Self {
        match s {
            "jwt" => Self::Jwt,
            _ => Self::Anon,
        }
    }
}

/// A row of `roadmap_votes` (Contract C14).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoadmapVote {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub item_id: Uuid,
    pub voter_id: String,
    pub voter_mode: RoadmapVoterMode,
    pub cast_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Tests — enum round-trip + serde shape
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roadmap_item_status_db_strings_round_trip() {
        for s in RoadmapItemStatus::all() {
            assert_eq!(RoadmapItemStatus::from_db_str(s.as_db_str()), *s);
        }
    }

    #[test]
    fn roadmap_item_status_default_is_considering() {
        assert_eq!(RoadmapItemStatus::default(), RoadmapItemStatus::Considering);
    }

    #[test]
    fn roadmap_item_status_unknown_db_str_falls_back_to_considering() {
        assert_eq!(
            RoadmapItemStatus::from_db_str("not-a-status"),
            RoadmapItemStatus::Considering
        );
    }

    #[test]
    fn roadmap_item_status_json_is_kebab_case() {
        assert_eq!(
            serde_json::to_string(&RoadmapItemStatus::InProgress).unwrap(),
            r#""in-progress""#
        );
        assert_eq!(
            serde_json::to_string(&RoadmapItemStatus::WontFix).unwrap(),
            r#""wontfix""#
        );
        let parsed: RoadmapItemStatus = serde_json::from_str(r#""in-progress""#).unwrap();
        assert_eq!(parsed, RoadmapItemStatus::InProgress);
    }

    #[test]
    fn roadmap_item_status_is_public_visible_for_all_v1() {
        for s in RoadmapItemStatus::all() {
            assert!(s.is_public_visible(), "{s:?} should be public in v1");
        }
    }

    #[test]
    fn roadmap_voter_mode_db_strings_round_trip() {
        for m in [RoadmapVoterMode::Jwt, RoadmapVoterMode::Anon] {
            assert_eq!(RoadmapVoterMode::from_db_str(m.as_db_str()), m);
        }
    }

    #[test]
    fn roadmap_voter_mode_unknown_falls_back_to_anon() {
        assert_eq!(RoadmapVoterMode::from_db_str("oauth"), RoadmapVoterMode::Anon);
    }

    #[test]
    fn roadmap_voter_mode_json_is_lowercase() {
        assert_eq!(
            serde_json::to_string(&RoadmapVoterMode::Jwt).unwrap(),
            r#""jwt""#
        );
        assert_eq!(
            serde_json::to_string(&RoadmapVoterMode::Anon).unwrap(),
            r#""anon""#
        );
    }
}
