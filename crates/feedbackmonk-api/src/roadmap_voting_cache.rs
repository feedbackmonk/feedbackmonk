#![allow(clippy::doc_markdown)] // module-doc text names crates/types verbatim without backticks
#![allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)] // limit constants u/i conversions are safe (capped at 50)

//! 60-second in-process voting-cache aggregator — Contract C15.
//!
//! Pattern adapted from `gitcellar-cloud/src/feedback/roadmap_voting.rs`
//! (LazyLock + RwLock + tokio::spawn). Differences from gitcellar:
//!
//! - **State location**: feedbackmonk holds the cache inside `AppState` (typed
//!   handle), not in a `LazyLock` process-singleton. gitcellar used
//!   LazyLock to dodge an AppState-pinning constraint under Read-Only-Tests;
//!   feedbackmonk's AppState is freely mutable mid-arc, so no LazyLock is
//!   needed and tests can construct an empty cache directly.
//!
//! - **Data source**: SQL aggregate via
//!   `RoadmapItemRepo::aggregate_vote_counts` instead of Forge HTTP fan-out.
//!
//! - **Per-project bucketing**: gitcellar served one Cloud Forge repo;
//!   feedbackmonk serves many projects across many tenants. The cache is
//!   keyed by `project_id`. The refresh tick iterates over `known_projects`
//!   (projects we've already cached) and re-aggregates each; cold projects
//!   are warmed lazily by the first read path that asks for them.
//!
//! ## Semantics (Contract C15)
//!
//! - **TTL**: 60 seconds (`VOTING_CACHE_TTL_SECS`).
//! - **Refresh tick**: `spawn_refresh_tick(state) -> JoinHandle<()>` from
//!   `main.rs::main` after `build_state`. Fires immediately, then every 60s.
//! - **Failure mode**: tick failures log WARN, cache retains last good
//!   payload per project. Cold-start failures leave the cache empty —
//!   public endpoints return `items: [], cached_at: null`, never an error.
//! - **Cold-start**: empty cache returns `items: [], cached_at: None`.
//! - **PII scrubbing**: tracing emitted from this module inherits the
//!   workspace-wide `feedbackmonk_tracing` scrubber automatically. No
//!   `tracing_subscriber` setup outside `crates/feedbackmonk-tracing/`.
//!
//! Lineage:
//!   FR-FBR-13 (voting + aggregator)
//!   Contract C15 (P2 plan §Interface Contracts)
//!   docs/planning/handoffs/p2-fanout-contracts.md §C15
//!   READ-ONLY reference: `gitcellar-cloud/src/feedback/roadmap_voting.rs`

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use feedbackmonk_repository::{ProjectRepo, RoadmapItemRepo};
use tokio::sync::RwLock;
use tracing::warn;
use uuid::Uuid;

/// Cache TTL (= refresh-tick interval).
pub const VOTING_CACHE_TTL_SECS: u64 = 60;

/// Default `limit` for top-voted endpoint when callers don't specify.
pub const DEFAULT_TOP_VOTED_LIMIT: usize = 10;

/// Server-side cap on `limit` for the top-voted endpoint.
pub const MAX_TOP_VOTED_LIMIT: usize = 50;

/// One row in the top-voted aggregate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopVotedRow {
    pub item_id: Uuid,
    pub vote_count: i64,
}

/// Per-project cache slot. `last_refreshed_at == None` means the slot was
/// touched (we tried to refresh) but the aggregate query failed; readers
/// distinguish this from "never refreshed" by checking whether the slot
/// exists in `per_project` at all.
#[derive(Debug, Clone, Default)]
pub struct ProjectCacheEntry {
    pub top_voted: Vec<TopVotedRow>,
    pub item_vote_counts: HashMap<Uuid, i64>,
    pub last_refreshed_at: Option<DateTime<Utc>>,
}

/// Mutable cache state, behind `Arc<RwLock<…>>`.
#[derive(Debug, Default)]
pub struct CacheInner {
    pub per_project: HashMap<Uuid, ProjectCacheEntry>,
    /// Latest wall-clock at which the tick completed *any* per-project refresh.
    /// Public read endpoints surface this as `cached_at` for projects that
    /// have been seen at least once.
    pub last_tick_at: Option<DateTime<Utc>>,
}

/// Cloneable handle the AppState carries.
#[derive(Clone, Debug, Default)]
pub struct VotingCache {
    inner: Arc<RwLock<CacheInner>>,
}

impl VotingCache {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Read top-voted for a project. Returns `(items, cached_at)`. Empty
    /// cache for a project returns `(vec![], None)` — never an error.
    /// Caller clamps `limit` to `MAX_TOP_VOTED_LIMIT` at the route layer.
    pub async fn read_top_voted(
        &self,
        project_id: Uuid,
        limit: usize,
    ) -> (Vec<TopVotedRow>, Option<DateTime<Utc>>) {
        let r = self.inner.read().await;
        let Some(slot) = r.per_project.get(&project_id) else {
            return (vec![], None);
        };
        let n = limit.min(slot.top_voted.len());
        (slot.top_voted[..n].to_vec(), slot.last_refreshed_at)
    }

    /// Look up the vote count for a single item out of the cached per-project
    /// aggregate. Returns `0` when the item isn't in cache (which may happen
    /// for very fresh items that landed between ticks).
    pub async fn read_item_count(&self, project_id: Uuid, item_id: Uuid) -> i64 {
        let r = self.inner.read().await;
        r.per_project
            .get(&project_id)
            .and_then(|s| s.item_vote_counts.get(&item_id).copied())
            .unwrap_or(0)
    }

    /// Returns the list of known project IDs — the refresh tick's working
    /// set. New projects join the set lazily (first read path that asks).
    pub async fn known_projects(&self) -> Vec<Uuid> {
        let r = self.inner.read().await;
        r.per_project.keys().copied().collect()
    }

    /// Replace the cached aggregate for a project. Atomic-replace under the
    /// write lock; readers see the prior payload until this returns.
    pub async fn replace_project(&self, project_id: Uuid, entry: ProjectCacheEntry) {
        let mut w = self.inner.write().await;
        w.per_project.insert(project_id, entry);
        w.last_tick_at = Some(Utc::now());
    }

    /// Ensure a project has a cache slot, even an empty one. Used by the
    /// public read path to register a project with the refresh tick — once
    /// in the set, subsequent ticks refresh it.
    pub async fn touch_project(&self, project_id: Uuid) {
        let mut w = self.inner.write().await;
        w.per_project.entry(project_id).or_default();
    }
}

/// Aggregate one project into a `ProjectCacheEntry`. Used by the cache
/// refresh tick AND by the public read path's cold-start warmer.
///
/// Generic over the `RoadmapItemRepo` impl so tests can pass a fake without
/// going through `Arc<dyn …>`.
pub async fn aggregate_project<R: RoadmapItemRepo + ?Sized>(
    item_repo: &R,
    scope: &feedbackmonk_repository::ProjectScope,
    limit: i64,
) -> feedbackmonk_repository::Result<ProjectCacheEntry> {
    let counts = item_repo.aggregate_vote_counts(scope, limit).await?;
    let mut item_vote_counts = HashMap::with_capacity(counts.len());
    let mut top_voted = Vec::with_capacity(counts.len());
    for (item_id, vote_count) in counts {
        item_vote_counts.insert(item_id, vote_count);
        top_voted.push(TopVotedRow { item_id, vote_count });
    }
    Ok(ProjectCacheEntry {
        top_voted,
        item_vote_counts,
        last_refreshed_at: Some(Utc::now()),
    })
}

/// Spawn the long-running refresh tick. Idempotent against multiple calls
/// (each spawn yields a new task — caller is expected to call exactly
/// once at startup and hold or drop the JoinHandle as desired).
///
/// The tick fires immediately (so the first request hits a warm cache for
/// projects that already have a slot) and then every `VOTING_CACHE_TTL_SECS`.
///
/// Failure mode: a per-project refresh error is logged at WARN, the slot
/// retains its prior payload, and the tick continues with the next project.
///
/// Project scope is minted via `ProjectRepo::open_for_submission` — the
/// allowlisted pre-auth boundary (DEC-PODS-001). The tick has no admin
/// session; this is the legitimate scope-minting path for the public
/// roadmap surface.
pub fn spawn_refresh_tick(
    cache: VotingCache,
    project_repo: Arc<dyn ProjectRepo>,
    item_repo: Arc<dyn RoadmapItemRepo>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(VOTING_CACHE_TTL_SECS));
        // First tick fires immediately so cold-start warms quickly. `interval`
        // ticks once on construction, so the loop's first `.tick().await`
        // returns instantly.
        loop {
            interval.tick().await;
            refresh_once(&cache, project_repo.as_ref(), item_repo.as_ref()).await;
        }
    })
}

/// One refresh pass — iterate over `known_projects` and re-aggregate each.
/// Exposed for test fixtures (drive the loop with `tokio::time::pause` and
/// invoke this directly to step the cache forward without sleeping).
pub async fn refresh_once(
    cache: &VotingCache,
    project_repo: &dyn ProjectRepo,
    item_repo: &dyn RoadmapItemRepo,
) {
    let known = cache.known_projects().await;
    for project_id in known {
        match project_repo.open_for_submission(project_id).await {
            Ok(scope) => match aggregate_project(item_repo, &scope, MAX_TOP_VOTED_LIMIT as i64).await {
                Ok(entry) => cache.replace_project(project_id, entry).await,
                Err(e) => warn!(
                    %project_id,
                    error = %e,
                    "roadmap_voting_cache: aggregate failed, keeping prior payload"
                ),
            },
            Err(e) => warn!(
                %project_id,
                error = %e,
                "roadmap_voting_cache: project_scope minting failed; tracking remains for next tick"
            ),
        }
    }
    // Update last_tick_at even when individual project refreshes failed —
    // it represents "tick fired", not "tick succeeded everywhere".
    let mut w = cache.inner.write().await;
    w.last_tick_at = Some(Utc::now());
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: full `aggregate_project` + `spawn_refresh_tick` integration
    // requires a real `ProjectScope` (the constructor is `pub(crate)`
    // inside `feedbackmonk-repository` — DEC-FBR-03 boundary). The repo
    // crate's sqlx::test integration covers `RoadmapItemRepo::aggregate_vote_counts`
    // directly; this test module exercises the cache primitives that don't
    // need a scope. Tick-failure tolerance is exercised by simulating
    // "tick didn't fire" via not calling `replace_project` (see
    // `tick_failure_keeps_last_value`).

    #[tokio::test]
    async fn cold_cache_returns_empty_and_none() {
        let cache = VotingCache::new();
        let pid = Uuid::new_v4();
        let (items, cached_at) = cache.read_top_voted(pid, 10).await;
        assert!(items.is_empty());
        assert!(cached_at.is_none());
        assert_eq!(cache.read_item_count(pid, Uuid::new_v4()).await, 0);
    }

    #[tokio::test]
    async fn replace_then_read_round_trips() {
        let cache = VotingCache::new();
        let pid = Uuid::new_v4();
        let item_a = Uuid::new_v4();
        let item_b = Uuid::new_v4();
        let now = Utc::now();
        let entry = ProjectCacheEntry {
            top_voted: vec![
                TopVotedRow { item_id: item_a, vote_count: 7 },
                TopVotedRow { item_id: item_b, vote_count: 3 },
            ],
            item_vote_counts: HashMap::from([(item_a, 7), (item_b, 3)]),
            last_refreshed_at: Some(now),
        };
        cache.replace_project(pid, entry).await;

        let (items, cached_at) = cache.read_top_voted(pid, 10).await;
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].item_id, item_a);
        assert_eq!(items[0].vote_count, 7);
        assert_eq!(items[1].item_id, item_b);
        assert!(cached_at.is_some());
        // last_refreshed_at must not regress on a single replace.
        assert!(cached_at.unwrap() >= now);

        assert_eq!(cache.read_item_count(pid, item_a).await, 7);
        assert_eq!(cache.read_item_count(pid, item_b).await, 3);
    }

    #[tokio::test]
    async fn read_top_voted_limit_clamps_to_cache_size() {
        let cache = VotingCache::new();
        let pid = Uuid::new_v4();
        cache
            .replace_project(
                pid,
                ProjectCacheEntry {
                    top_voted: vec![TopVotedRow {
                        item_id: Uuid::new_v4(),
                        vote_count: 1,
                    }],
                    item_vote_counts: HashMap::new(),
                    last_refreshed_at: Some(Utc::now()),
                },
            )
            .await;
        let (items, _) = cache.read_top_voted(pid, 100).await;
        assert_eq!(items.len(), 1, "limit clamps to cache len");
    }

    #[tokio::test]
    async fn touch_project_makes_project_visible_to_tick() {
        let cache = VotingCache::new();
        let pid = Uuid::new_v4();
        assert!(cache.known_projects().await.is_empty());
        cache.touch_project(pid).await;
        let known = cache.known_projects().await;
        assert_eq!(known.len(), 1);
        assert!(known.contains(&pid));
    }

    #[tokio::test]
    async fn tick_failure_keeps_last_value() {
        // Simulate a tick failure by populating the cache and then NOT
        // calling replace_project — the payload must persist across the
        // hypothetical failed tick. Real tick failures log WARN and skip
        // the project; this test verifies the read path serves prior data.
        let cache = VotingCache::new();
        let pid = Uuid::new_v4();
        let item = Uuid::new_v4();
        cache
            .replace_project(
                pid,
                ProjectCacheEntry {
                    top_voted: vec![TopVotedRow { item_id: item, vote_count: 5 }],
                    item_vote_counts: HashMap::from([(item, 5)]),
                    last_refreshed_at: Some(Utc::now()),
                },
            )
            .await;
        // Simulate a tick failure by NOT calling replace_project — the
        // payload stays as 5.
        let (items, _) = cache.read_top_voted(pid, 10).await;
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].vote_count, 5);
    }
}
