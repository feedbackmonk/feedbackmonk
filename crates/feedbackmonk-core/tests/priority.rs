//! The aggregate-priority fit (FR-FBR-52..54, DEC-FBR-18).
//!
//! Every test name starts with `priority_` so the plan's proof,
//! `cargo test -p feedbackmonk-core -- priority`, selects exactly these.

use feedbackmonk_core::priority::{stated_preferences, Preference};
use feedbackmonk_core::{fit_priority, preference_weight, PriorityFit, PriorityFitConfig};

fn cfg() -> PriorityFitConfig {
    PriorityFitConfig::default()
}

fn lists(raw: &[&[&'static str]]) -> Vec<Vec<&'static str>> {
    raw.iter().map(|l| l.to_vec()).collect()
}

fn strength(fit: &PriorityFit<&'static str>, key: &str) -> f64 {
    fit.ranked
        .iter()
        .find(|r| r.key == key)
        .unwrap_or_else(|| panic!("{key} is not ranked: {fit:?}"))
        .strength
}

fn order(fit: &PriorityFit<&'static str>) -> Vec<&'static str> {
    fit.ranked.iter().map(|r| r.key).collect()
}

fn unranked(fit: &PriorityFit<&'static str>) -> Vec<&'static str> {
    fit.unranked.iter().map(|u| u.key).collect()
}

// ---- FR-FBR-52: only stated preferences count ----

#[test]
fn priority_list_states_every_ordered_pair_and_nothing_else() {
    let pool = ["a", "b", "c", "d"];
    let prefs = stated_preferences(&pool, &["a", "b", "c"]);
    let pairs: Vec<(&str, &str)> = prefs.iter().map(|p| (p.winner, p.loser)).collect();
    assert_eq!(pairs, vec![("a", "b"), ("a", "c"), ("b", "c")]);
    assert!(
        prefs.iter().all(|p| p.winner != "d" && p.loser != "d"),
        "an item absent from the list states nothing for or against"
    );
}

#[test]
fn priority_items_outside_the_pool_are_ignored() {
    let pool = ["a", "c"];
    let prefs = stated_preferences(&pool, &["a", "x", "c"]);
    assert_eq!(
        prefs,
        vec![Preference {
            winner: "a",
            loser: "c",
            weight: 1.0
        }],
        "x is not in the pool: k is 2, one pair of weight 2/2"
    );
}

#[test]
fn priority_unranked_item_gets_no_strength() {
    let pool = ["a", "b", "c"];
    let fit = fit_priority(&pool, &lists(&[&["a", "b"], &["b", "a"], &["a", "b"]]), &cfg());
    assert_eq!(unranked(&fit), vec!["c"]);
    assert_eq!(fit.unranked[0].rankers, 0);
    assert!(
        fit.ranked.iter().all(|r| r.key != "c"),
        "c must not be given a strength: {fit:?}"
    );
    assert_eq!(order(&fit), vec!["a", "b"]);
}

#[test]
fn priority_one_item_list_moves_nothing() {
    let pool = ["a", "b", "c", "x"];
    let base = lists(&[&["a", "b", "c"], &["b", "c"]]);
    let mut with_singletons = base.clone();
    with_singletons.push(vec!["c"]);
    with_singletons.push(vec!["x"]);
    with_singletons.push(vec!["x"]);

    assert!(stated_preferences(&pool, &["c"]).is_empty());

    let before = fit_priority(&pool, &base, &cfg());
    let after = fit_priority(&pool, &with_singletons, &cfg());
    assert_eq!(order(&before), order(&after));
    for r in &before.ranked {
        assert_eq!(r.strength, strength(&after, r.key), "{} moved", r.key);
    }
    // x was placed, but only alone: it is unranked, and its rankers still show.
    let x = after.unranked.iter().find(|u| u.key == "x").expect("x unranked");
    assert_eq!(x.rankers, 2);
    let c = after.ranked.iter().find(|r| r.key == "c").expect("c ranked");
    assert_eq!(c.rankers, 3, "the one-item list still counts as a ranker");
}

#[test]
fn priority_rankers_counts_users_not_mentions() {
    let pool = ["a", "b"];
    // A duplicate inside one list is one placement, not two.
    let fit = fit_priority(&pool, &lists(&[&["a", "b", "a"], &["b", "a"]]), &cfg());
    for r in &fit.ranked {
        assert_eq!(r.rankers, 2, "{}", r.key);
    }
}

// ---- FR-FBR-53: linear influence ----

#[test]
fn priority_total_per_user_weight_is_k_minus_one() {
    assert_eq!(preference_weight(0), 0.0);
    assert_eq!(preference_weight(1), 0.0);
    let pool: Vec<u32> = (0..60).collect();
    for k in 2..=60usize {
        let list: Vec<u32> = (0..k as u32).rev().collect();
        let prefs = stated_preferences(&pool, &list);
        assert_eq!(prefs.len(), k * (k - 1) / 2, "k={k}");
        let total: f64 = prefs.iter().map(|p| p.weight).sum();
        assert!(
            (total - (k as f64 - 1.0)).abs() < 1e-9,
            "k={k}: total weight {total}"
        );
        assert!(prefs.iter().all(|p| p.weight == preference_weight(k)));
    }
}

#[test]
fn priority_user_weight_is_invariant_to_other_users_list_lengths() {
    // A user's preferences are a function of their own list alone ...
    let pool: Vec<u32> = (0..30).collect();
    let mine = vec![3, 1, 2];
    let alone = stated_preferences(&pool, &mine);
    assert!((alone.iter().map(|p| p.weight).sum::<f64>() - 2.0).abs() < 1e-9);

    // ... and in the fit, other users ranking more items elsewhere does not
    // change what this user's items get.
    let short = vec![mine.clone(), vec![10, 11]];
    let long = vec![mine.clone(), (10..30).collect::<Vec<u32>>()];
    let a = fit_priority(&pool, &short, &cfg());
    let b = fit_priority(&pool, &long, &cfg());
    for key in &mine {
        let sa = a.ranked.iter().find(|r| r.key == *key).unwrap().strength;
        let sb = b.ranked.iter().find(|r| r.key == *key).unwrap().strength;
        assert!((sa - sb).abs() < 1e-6, "item {key}: {sa} vs {sb}");
    }
}

#[test]
fn priority_k20_ranker_does_not_outweigh_twenty_k2_rankers_on_a_shared_pair() {
    let items: Vec<String> = (0..20).map(|i| format!("i{i:02}")).collect();
    let pool = items.clone();
    // One thorough user places x = i00 directly above y = i01 ...
    let mut all = vec![items.clone()];
    // ... twenty users each say only "y over x".
    for _ in 0..20 {
        all.push(vec![items[1].clone(), items[0].clone()]);
    }
    let fit = fit_priority(&pool, &all, &cfg());
    let pos = |k: &str| fit.ranked.iter().position(|r| r.key == k).unwrap();
    assert!(
        pos("i01") < pos("i00"),
        "twenty k=2 rankers (weight 20) must beat one k=20 ranker (weight 0.1): {:?}",
        fit.ranked.iter().map(|r| &r.key).collect::<Vec<_>>()
    );
}

// ---- FR-FBR-54: regularized, deterministic fit ----

#[test]
fn priority_one_comparison_does_not_reach_an_extreme() {
    let pool = ["a", "b"];
    let once = fit_priority(&pool, &lists(&[&["a", "b"]]), &cfg());
    let sa = strength(&once, "a");
    let sb = strength(&once, "b");
    assert!(sa.is_finite() && sb.is_finite());
    assert!(sa > 0.0 && sb < 0.0, "a over b: {sa} {sb}");
    assert!(sa.abs() < 1.0 && sb.abs() < 1.0, "pulled toward neutral: {sa} {sb}");

    // Ten users saying the same move it further: one comparison is not the extreme.
    let ten: Vec<Vec<&str>> = (0..10).map(|_| vec!["a", "b"]).collect();
    let many = fit_priority(&pool, &ten, &cfg());
    assert!(strength(&many, "a") > sa + 0.5, "{} vs {sa}", strength(&many, "a"));
    // And the uncertainty shrinks with evidence.
    let se = |f: &PriorityFit<&'static str>| f.ranked.iter().find(|r| r.key == "a").unwrap().std_error;
    assert!(se(&once).is_finite() && se(&once) > 0.0);
    assert!(se(&many) < se(&once));
}

#[test]
fn priority_fixed_input_gives_identical_order_on_repeated_runs() {
    let pool = ["e", "d", "c", "b", "a", "f"];
    let input = lists(&[
        &["a", "b", "c"],
        &["c", "a"],
        &["b", "d", "e", "a"],
        &["e", "d"],
        &["d", "c"],
        &["f"],
    ]);
    let first = fit_priority(&pool, &input, &cfg());
    assert_eq!(first.ranked.len(), 5);
    for _ in 0..20 {
        assert_eq!(fit_priority(&pool, &input, &cfg()), first);
    }
    // The pool's order does not matter either.
    let mut shuffled = pool;
    shuffled.reverse();
    assert_eq!(order(&fit_priority(&shuffled, &input, &cfg())), order(&first));
}

#[test]
fn priority_symmetric_items_tie_break_by_key() {
    let pool = ["b", "a"];
    let fit = fit_priority(&pool, &lists(&[&["a", "b"], &["b", "a"]]), &cfg());
    assert_eq!(order(&fit), vec!["a", "b"]);
}

#[test]
fn priority_ranked_is_strongest_first() {
    let pool = ["a", "b", "c"];
    let fit = fit_priority(
        &pool,
        &lists(&[&["c", "b", "a"], &["c", "b", "a"], &["c", "a"]]),
        &cfg(),
    );
    assert_eq!(order(&fit), vec!["c", "b", "a"]);
    assert!(fit.ranked.windows(2).all(|w| w[0].strength >= w[1].strength));
}
