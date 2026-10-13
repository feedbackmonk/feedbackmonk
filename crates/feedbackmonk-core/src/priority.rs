//! Aggregate priority of the ranking pool — a regularized, weighted
//! Bradley–Terry fit over the "a over b" pairs users' priority lists state
//! (FR-FBR-52/53/54, DEC-FBR-18).
//!
//! The types and [`fit_priority`]'s signature were frozen by the `contracts`
//! piece of the end-user-prioritization plan; the tests are
//! `crates/feedbackmonk-core/tests/priority.rs`.
//!
//! The rules (spec `docs/specs/end-user-prioritization.md`):
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
//! **The model.** Item `i` has strength `p_i = exp(θ_i)`, and
//! `P(i over j) = p_i / (p_i + p_j)`. The prior is `prior_strength`
//! pseudo-comparisons per item against a fixed virtual item of strength 1
//! (θ = 0), half won and half lost: it pulls toward θ = 0 and anchors the
//! scale. The MAP estimate comes from Hunter's (2004) MM iteration, which
//! raises the posterior at every step. `std_error` is the square root of the
//! diagonal of the inverse observed information (the Hessian of the negative
//! log-posterior in θ). Items no preference touches never enter the fit. Every
//! sum runs over items in `K` order, so the arithmetic, and so the result, is
//! the same on every run.
//!
//! The owner sees the result beside votes and not-needed counts, never merged
//! with them (FR-FBR-55): this module knows nothing of votes, by design.

use std::collections::BTreeSet;

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
#[allow(clippy::cast_precision_loss)] // k is a list length, far below 2^52
pub fn preference_weight(k: usize) -> f64 {
    if k < 2 {
        0.0
    } else {
        2.0 / k as f64
    }
}

/// One stated preference, "`winner` over `loser`", with its weight
/// ([`preference_weight`] of the number of pool items in the list).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preference<K> {
    pub winner: K,
    pub loser: K,
    pub weight: f64,
}

/// The pool items of `list`, in its order, each once (first placement wins).
fn placed_in_pool<'a, K: Ord>(pool: &BTreeSet<&K>, list: &'a [K]) -> Vec<&'a K> {
    let mut seen: BTreeSet<&K> = BTreeSet::new();
    list.iter()
        .filter(|k| pool.contains(k) && seen.insert(*k))
        .collect()
}

/// The preferences ONE user's `list` (top first) states over `pool`
/// (FR-FBR-52/53): for every pair of pool items in the list, "higher over
/// lower", each weighing [`preference_weight`]`(k)`, where `k` is the number of
/// distinct pool items in the list. Items outside the pool are dropped before
/// `k` is counted; a repeated item counts at its first placement. It reads one
/// list and nothing else, so no other user's list can change these weights.
/// [`fit_priority`] builds its input from exactly this rule.
#[must_use]
pub fn stated_preferences<K>(pool: &[K], list: &[K]) -> Vec<Preference<K>>
where
    K: Clone + Ord,
{
    let pool: BTreeSet<&K> = pool.iter().collect();
    let placed = placed_in_pool(&pool, list);
    let weight = preference_weight(placed.len());
    let mut out = Vec::new();
    for (i, hi) in placed.iter().enumerate() {
        for lo in &placed[i + 1..] {
            out.push(Preference {
                winner: (*hi).clone(),
                loser: (*lo).clone(),
                weight,
            });
        }
    }
    out
}

/// Fit the aggregate priority of `pool` from users' ordered `lists`
/// (FR-FBR-52..54). Each inner list is ONE user's order, top first; lists carry
/// no user identity (FR-FBR-56). Deterministic for a given input.
///
/// A non-positive or non-finite `prior_strength` is raised to `1e-6`: with no
/// prior, an item that only ever wins has no finite strength.
///
/// Cost is O(n²) memory and O(n³) time in the number of touched items (the
/// uncertainty needs the inverse information matrix).
#[must_use]
#[allow(clippy::needless_range_loop)] // index-parallel matrices read clearer by index
pub fn fit_priority<K>(pool: &[K], lists: &[Vec<K>], config: &PriorityFitConfig) -> PriorityFit<K>
where
    K: Clone + Ord,
{
    // The pool in K order; an item's index is its position here.
    let mut keys: Vec<&K> = pool.iter().collect();
    keys.sort();
    keys.dedup();
    let n = keys.len();
    let pool_set: BTreeSet<&K> = keys.iter().copied().collect();

    // wins[i][j]: total weight of "i over j". rankers[i]: lists placing i.
    let mut wins = vec![vec![0.0_f64; n]; n];
    let mut rankers = vec![0_u32; n];
    for list in lists {
        let placed: Vec<usize> = placed_in_pool(&pool_set, list)
            .into_iter()
            .filter_map(|k| keys.binary_search(&k).ok())
            .collect();
        let weight = preference_weight(placed.len());
        for (a, &hi) in placed.iter().enumerate() {
            rankers[hi] = rankers[hi].saturating_add(1);
            for &lo in &placed[a + 1..] {
                wins[hi][lo] += weight;
            }
        }
    }

    // Touched items enter the fit; the rest are unranked (FR-FBR-52/54).
    let touched: Vec<usize> = (0..n)
        .filter(|&i| (0..n).any(|j| wins[i][j] > 0.0 || wins[j][i] > 0.0))
        .collect();
    let unranked: Vec<UnrankedItem<K>> = (0..n)
        .filter(|i| touched.binary_search(i).is_err())
        .map(|i| UnrankedItem {
            key: keys[i].clone(),
            rankers: rankers[i],
        })
        .collect();

    // The sub-problem over touched items: w[a][b] wins, c[a][b] comparisons.
    let m = touched.len();
    let w: Vec<Vec<f64>> = touched
        .iter()
        .map(|&i| touched.iter().map(|&j| wins[i][j]).collect())
        .collect();
    let c: Vec<Vec<f64>> = (0..m)
        .map(|a| (0..m).map(|b| w[a][b] + w[b][a]).collect())
        .collect();
    let prior = if config.prior_strength.is_finite() && config.prior_strength > 0.0 {
        config.prior_strength
    } else {
        1e-6
    };
    let theta = fit_theta(&w, &c, prior, config);
    let std_error = standard_errors(&theta, &c, prior);

    let mut ranked: Vec<RankedItem<K>> = (0..m)
        .map(|a| RankedItem {
            key: keys[touched[a]].clone(),
            strength: theta[a],
            std_error: std_error[a],
            rankers: rankers[touched[a]],
        })
        .collect();
    // Strongest first. Strengths compare at 1e-9 resolution, so two items the
    // data cannot tell apart order by key rather than by rounding noise.
    ranked.sort_by(|x, y| {
        quantize(y.strength)
            .cmp(&quantize(x.strength))
            .then_with(|| x.key.cmp(&y.key))
    });

    PriorityFit { ranked, unranked }
}

#[allow(clippy::cast_possible_truncation)] // |θ| is bounded far below 9e9
fn quantize(s: f64) -> i64 {
    (s * 1e9).round() as i64
}

/// MAP log-strengths by the MM iteration (Hunter 2004), the prior being
/// `prior` comparisons against a virtual item of strength 1, half of them won.
#[allow(clippy::needless_range_loop)]
fn fit_theta(w: &[Vec<f64>], c: &[Vec<f64>], prior: f64, config: &PriorityFitConfig) -> Vec<f64> {
    let m = w.len();
    let won: Vec<f64> = w
        .iter()
        .map(|row| row.iter().sum::<f64>() + prior / 2.0)
        .collect();
    let mut p = vec![1.0_f64; m];
    for _ in 0..config.max_iterations {
        let next: Vec<f64> = (0..m)
            .map(|i| {
                let mut denom = prior / (p[i] + 1.0);
                for j in 0..m {
                    if c[i][j] > 0.0 {
                        denom += c[i][j] / (p[i] + p[j]);
                    }
                }
                won[i] / denom
            })
            .collect();
        let delta = (0..m)
            .map(|i| (next[i].ln() - p[i].ln()).abs())
            .fold(0.0_f64, f64::max);
        p = next;
        if delta < config.tolerance {
            break;
        }
    }
    p.iter().map(|x| x.ln()).collect()
}

/// `sqrt(diag(H⁻¹))`, with `H` the Hessian of the negative log-posterior in θ.
/// The prior term makes `H` strictly diagonally dominant, so it is symmetric
/// positive definite and its Cholesky factorization always exists.
#[allow(clippy::needless_range_loop)]
fn standard_errors(theta: &[f64], c: &[Vec<f64>], prior: f64) -> Vec<f64> {
    let m = theta.len();
    // Fisher information of one comparison between strengths a and b.
    let info = |a: f64, b: f64| {
        let pr = 1.0 / (1.0 + (b - a).exp());
        pr * (1.0 - pr)
    };
    let mut h = vec![vec![0.0_f64; m]; m];
    for i in 0..m {
        h[i][i] = prior * info(theta[i], 0.0);
        for j in 0..m {
            if i != j && c[i][j] > 0.0 {
                let v = c[i][j] * info(theta[i], theta[j]);
                h[i][i] += v;
                h[i][j] -= v;
            }
        }
    }
    // Cholesky: H = L Lᵀ.
    let mut l = vec![vec![0.0_f64; m]; m];
    for i in 0..m {
        for j in 0..=i {
            let s: f64 = (0..j).map(|k| l[i][k] * l[j][k]).sum();
            if i == j {
                l[i][i] = (h[i][i] - s).max(f64::MIN_POSITIVE).sqrt();
            } else {
                l[i][j] = (h[i][j] - s) / l[j][j];
            }
        }
    }
    // H⁻¹ = L⁻ᵀ L⁻¹, so (H⁻¹)_ii is the squared norm of column i of L⁻¹,
    // which forward substitution of L x = e_i yields.
    let mut var = vec![0.0_f64; m];
    for col in 0..m {
        let mut x = vec![0.0_f64; m];
        for r in col..m {
            let rhs = if r == col { 1.0 } else { 0.0 };
            let s: f64 = (col..r).map(|k| l[r][k] * x[k]).sum();
            x[r] = (rhs - s) / l[r][r];
        }
        var[col] = x.iter().map(|v| v * v).sum();
    }
    var.into_iter().map(f64::sqrt).collect()
}
