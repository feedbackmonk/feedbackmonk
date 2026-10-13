//! Aggregate priority of the ranking pool — a regularized, weighted
//! Bradley–Terry fit over the "a over b" pairs users' priority lists state
//! (FR-FBR-52/53/54, DEC-FBR-18).
//!
//! **Contract-frozen signature, stub body.** The `contracts` piece of the
//! end-user-prioritization plan freezes the types and [`fit_priority`]'s
//! signature here; the `priority-engine` piece writes the body and its tests
//! (`crates/feedbackmonk-core/tests/priority*.rs`). Until then the stub reports
//! every pool item as unranked, which is a valid (if uninformative) answer and
//! keeps every caller compiling.
//!
//! The rules the body must implement (spec `docs/specs/end-user-prioritization.md`):
//!
//! - **Stated pairs only (FR-FBR-52).** A list `[a, b, c]` states `a>b`, `a>c`,
//!   `b>c`. An item absent from a list states nothing for or against. A list of
//!   fewer than two pool items states nothing. Items not in `pool` are ignored
//!   (the caller passes lists already restricted to the pool, but the fit must
//!   not trust that).
//! - **Linear influence (FR-FBR-53).** Each pair from a user who placed `k`
//!   pool items weighs `2 / k`, so the user's total is `k − 1`. No user's weight
//!   depends on other users' list lengths. [`preference_weight`] is that rule.
//! - **Regularized fit (FR-FBR-54).** A prior pulls items with few comparisons
//!   toward neutral strength, so one comparison cannot put an item at either
//!   extreme. The same input always gives the same order (ties broken by `K`'s
//!   `Ord`). An item no preference touches is reported in
//!   [`PriorityFit::unranked`], never given a strength.
//!
//! The owner sees the result beside votes and not-needed counts, never merged
//! with them (FR-FBR-55): this module knows nothing of votes, by design.

use serde::{Deserialize, Serialize};

/// Tunables for [`fit_priority`]. `Default` is what the owner console uses.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PriorityFitConfig {
    /// Strength of the prior toward neutral (in pseudo-comparisons against a
    /// virtual neutral item). Larger pulls sparse items harder toward 0.
    pub prior_strength: f64,
    /// Iteration cap for the fit.
    pub max_iterations: u32,
    /// Convergence tolerance on the largest strength change per iteration.
    pub tolerance: f64,
}

impl Default for PriorityFitConfig {
    fn default() -> Self {
        Self {
            prior_strength: 1.0,
            max_iterations: 500,
            tolerance: 1e-9,
        }
    }
}

/// One item with a fitted strength (FR-FBR-54/55).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RankedItem<K> {
    pub key: K,
    /// Log-strength; 0 is neutral. Only the ORDER is shown to the owner.
    pub strength: f64,
    /// Standard error of `strength` from the fit — the owner's uncertainty
    /// indicator (FR-FBR-55).
    pub std_error: f64,
    /// Number of users who placed this item in their list (FR-FBR-55).
    pub rankers: u32,
}

/// A pool item no stated preference touches (FR-FBR-52/54): unranked, with no
/// strength. `rankers` may still be 1 or more (a user whose list holds only
/// this item placed it, but stated no pair).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnrankedItem<K> {
    pub key: K,
    pub rankers: u32,
}

/// The fit's result: `ranked` strongest-first, `unranked` in `K` order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriorityFit<K> {
    pub ranked: Vec<RankedItem<K>>,
    pub unranked: Vec<UnrankedItem<K>>,
}

/// The weight of each preference from a user who placed `k` pool items
/// (FR-FBR-53): `2 / k`, so their `k(k−1)/2` pairs total `k − 1`. Zero for
/// `k < 2` (such a list states no pair).
#[must_use]
pub fn preference_weight(k: usize) -> f64 {
    // Contract stub: the priority-engine piece implements FR-FBR-53 here.
    let _ = k;
    0.0
}

/// Fit the aggregate priority of `pool` from users' ordered `lists`
/// (FR-FBR-52..54). Each inner list is ONE user's order, top first; lists carry
/// no user identity (FR-FBR-56). Deterministic for a given input.
#[must_use]
pub fn fit_priority<K>(pool: &[K], lists: &[Vec<K>], config: &PriorityFitConfig) -> PriorityFit<K>
where
    K: Clone + Ord,
{
    // Contract stub: the priority-engine piece replaces this body. Until then
    // every pool item is reported unranked and nothing is given a strength.
    let _ = (lists, config);
    let mut keys: Vec<K> = pool.to_vec();
    keys.sort();
    keys.dedup();
    PriorityFit {
        ranked: Vec::new(),
        unranked: keys
            .into_iter()
            .map(|key| UnrankedItem { key, rankers: 0 })
            .collect(),
    }
}
