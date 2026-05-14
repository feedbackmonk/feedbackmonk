//! Tier-cap predicate repository — Contract C17 (P3 Stage 1).
//!
//! The single authoritative check `check_tier_quota(scope, resource)` that
//! every domain-write handler under `crates/feedbackmonk-api/src/handlers/`
//! consults BEFORE its first INSERT. Reads `tenants.tier` plus the live
//! count for the resource, matches against `tier_quotas(tier).limit_for(...)`,
//! and returns `QuotaStatus` (`allowed: bool`). Handlers map
//! `allowed = false` to `ApiError::TierCapExceeded` per Contract C18.
//!
//! Sibling read-only `get_tier_status(scope)` returns the current tier +
//! quotas + live usage in one round; the admin `GET /api/v1/admin/tier`
//! endpoint (P3 Stage 1 handler) renders this as `TierStatus` JSON.
//!
//! Lineage:
//! - FR-FBR-14 (commercial-gate: caps + footer)
//! - DEC-FBR-03 (pricing tier matrix; load-bearing)
//! - Contract C17 (P3 plan §Interface Contracts) — predicate signature
//! - Contract C18 (Stage 1→Stage 2 freeze) — error-body shape
//! - Contract C19 (Stage 1→Stage 2 freeze) — `tier_quotas()` static config
//!
//! Probandurgy:
//! - `tier-enforcement-status` oracle Probe A enforces caller coverage.
//! - `multi-tenant-isolation-check` oracle's allowlist exempts the
//!   `SqlxTierQuotaRepo::new` constructor (structural-mirror entry; no
//!   DB access). Every trait method takes `&TenantScope` as its first
//!   non-`&self` arg — Probe B compliance is direct, no allowlist needed.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use feedbackmonk_core::{tier_quotas, ResourceKind, Tier, TierQuotas};

use crate::error::Result;
use crate::scope::TenantScope;
use crate::tenants::{SqlxTenantRepo, TenantRepo};

/// Default volume window for `ResourceKind::FeedbackInRollingMonth`
/// (DEC-FBR-03 implicit choice + P3 plan §Deferred Decisions row "rolling
/// 30-day vs calendar month"). Rolling-window semantics match Plausible /
/// standard `SaaS` pattern.
pub const ROLLING_FEEDBACK_WINDOW_DAYS: i64 = 30;

/// Cap-check result. `allowed = false` is the signal handlers map to
/// `ApiError::TierCapExceeded`. `current` and `limit` populate the
/// structured error body per Contract C18.
///
/// `limit: Option<i64>` mirrors the tier table — `None` is "unlimited"
/// (Pro/`SelfHost` projects; `SelfHost` feedback). `allowed` is `false` only
/// when `limit = Some(n)` AND `current >= n`; an unlimited resource
/// always allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuotaStatus {
    pub tier: Tier,
    pub resource: ResourceKind,
    pub current: i64,
    pub limit: Option<i64>,
    pub allowed: bool,
}

/// Aggregate read for the admin `GET /api/v1/admin/tier` endpoint
/// (Contract C17). Returns current tier + static quotas + live usage in
/// one repository call; no write side-effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TierStatus {
    pub tier: Tier,
    pub quotas: TierQuotas,
    pub usage: TierUsage,
}

/// Live usage snapshot for `TierStatus`. `period_start` is the rolling
/// window's left boundary at read time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TierUsage {
    pub projects: i64,
    pub feedback_monthly: i64,
    pub period_start: DateTime<Utc>,
}

/// Tier-cap predicate. Single trait owns the surface; both methods
/// are scope-bound (multi-tenant-isolation Probe B compliance).
#[async_trait]
pub trait TierQuotaRepo: Send + Sync {
    /// Predicate: would creating a NEW unit of `resource` under `scope`
    /// be allowed by the tenant's current tier? Reads tier + live count;
    /// computes `current >= limit` semantics.
    async fn check_tier_quota(
        &self,
        scope: &TenantScope,
        resource: ResourceKind,
    ) -> Result<QuotaStatus>;

    /// Aggregate read for the admin tier-status endpoint. No side effects.
    async fn get_tier_status(&self, scope: &TenantScope) -> Result<TierStatus>;
}

/// `sqlx`-backed implementation. Constructor allowlisted in
/// `.claude/oracles/multi-tenant-isolation-check/allowlist.toml` as a
/// structural-mirror entry (no DB access; identical shape to
/// `SqlxFeedbackRepo::new` and siblings).
#[derive(Clone)]
pub struct SqlxTierQuotaRepo {
    tenants: SqlxTenantRepo,
}

impl SqlxTierQuotaRepo {
    /// Construct from a shared pool. Stores a `SqlxTenantRepo` handle
    /// rather than the raw pool so all tier-cap counting goes through
    /// the repository layer (no raw SQL in this file).
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self {
            tenants: SqlxTenantRepo::new(pool),
        }
    }
}

#[async_trait]
impl TierQuotaRepo for SqlxTierQuotaRepo {
    async fn check_tier_quota(
        &self,
        scope: &TenantScope,
        resource: ResourceKind,
    ) -> Result<QuotaStatus> {
        let tier = self.tenants.get_tier(scope).await?;
        let quotas = tier_quotas(tier);
        let limit = quotas.limit_for(resource);
        let current = match resource {
            ResourceKind::Project => self.tenants.count_projects(scope).await?,
            ResourceKind::FeedbackInRollingMonth => {
                self.tenants
                    .count_feedback_in_window(scope, ROLLING_FEEDBACK_WINDOW_DAYS)
                    .await?
            }
        };
        let allowed = match limit {
            None => true,
            Some(cap) => current < cap,
        };
        Ok(QuotaStatus {
            tier,
            resource,
            current,
            limit,
            allowed,
        })
    }

    async fn get_tier_status(&self, scope: &TenantScope) -> Result<TierStatus> {
        let tier = self.tenants.get_tier(scope).await?;
        let quotas = tier_quotas(tier);
        let projects = self.tenants.count_projects(scope).await?;
        let feedback_monthly = self
            .tenants
            .count_feedback_in_window(scope, ROLLING_FEEDBACK_WINDOW_DAYS)
            .await?;
        let now = Utc::now();
        let period_start =
            now - chrono::Duration::days(ROLLING_FEEDBACK_WINDOW_DAYS);
        Ok(TierStatus {
            tier,
            quotas,
            usage: TierUsage {
                projects,
                feedback_monthly,
                period_start,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feedback::{FeedbackRepo, SqlxFeedbackRepo};
    use crate::projects::SqlxProjectRepo;
    use feedbackmonk_core::FeedbackKind;
    use sqlx::PgPool;

    async fn set_tier(pool: &PgPool, tenant_id: uuid::Uuid, tier_str: &str) {
        sqlx::query!(
            "UPDATE tenants SET tier = $2 WHERE id = $1",
            tenant_id,
            tier_str,
        )
        .execute(pool)
        .await
        .unwrap();
    }

    async fn seed_tenant_with_tier(pool: &PgPool, email: &str, tier_str: &str) -> TenantScope {
        let trepo = SqlxTenantRepo::new(pool.clone());
        let t = trepo.create(email, "h").await.unwrap();
        set_tier(pool, t.id, tier_str).await;
        trepo.scope_for(t.id).await.unwrap()
    }

    // ---- Project resource ---------------------------------------------------

    #[sqlx::test(migrations = "../../migrations")]
    async fn free_project_under_cap_is_allowed(pool: PgPool) {
        let repo = SqlxTierQuotaRepo::new(pool.clone());
        let scope = seed_tenant_with_tier(&pool, "free-under@example.com", "free").await;
        let status = repo
            .check_tier_quota(&scope, ResourceKind::Project)
            .await
            .unwrap();
        assert!(status.allowed, "Free + zero projects must allow");
        assert_eq!(status.tier, Tier::Free);
        assert_eq!(status.current, 0);
        assert_eq!(status.limit, Some(1));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn free_project_at_cap_is_blocked(pool: PgPool) {
        use crate::projects::ProjectRepo;
        let repo = SqlxTierQuotaRepo::new(pool.clone());
        let projects = SqlxProjectRepo::new(pool.clone());
        let scope = seed_tenant_with_tier(&pool, "free-at-cap@example.com", "free").await;
        // Seed the cap-line: 1 project for Free.
        projects.create(&scope, "P1", "p1").await.unwrap();
        let status = repo
            .check_tier_quota(&scope, ResourceKind::Project)
            .await
            .unwrap();
        assert!(!status.allowed, "Free + 1 project must block");
        assert_eq!(status.current, 1);
        assert_eq!(status.limit, Some(1));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn pro_project_unlimited(pool: PgPool) {
        use crate::projects::ProjectRepo;
        let repo = SqlxTierQuotaRepo::new(pool.clone());
        let projects = SqlxProjectRepo::new(pool.clone());
        let scope = seed_tenant_with_tier(&pool, "pro@example.com", "pro").await;
        // Seed 5 projects — well past Free + Starter caps.
        for i in 0..5 {
            projects.create(&scope, "P", &format!("p{i}")).await.unwrap();
        }
        let status = repo
            .check_tier_quota(&scope, ResourceKind::Project)
            .await
            .unwrap();
        assert!(status.allowed, "Pro + 5 projects must allow (unlimited)");
        assert_eq!(status.limit, None);
        assert_eq!(status.current, 5);
    }

    // ---- Feedback rolling-window resource -----------------------------------

    #[sqlx::test(migrations = "../../migrations")]
    async fn free_feedback_just_under_cap_is_allowed(pool: PgPool) {
        use crate::projects::ProjectRepo;
        let repo = SqlxTierQuotaRepo::new(pool.clone());
        let projects = SqlxProjectRepo::new(pool.clone());
        let feedback = SqlxFeedbackRepo::new(pool.clone());
        let scope = seed_tenant_with_tier(&pool, "free-fb-49@example.com", "free").await;
        let p = projects.create(&scope, "P", "p").await.unwrap();
        let pscope = projects.open(&scope, p.id).await.unwrap();

        // Seed 49 submissions — Free cap is 50.
        for i in 0..49 {
            feedback
                .submit_anonymous(
                    &pscope,
                    &[u8::try_from(i % 251).unwrap(); 32],
                    None,
                    &format!("fb {i}"),
                    FeedbackKind::Other,
                )
                .await
                .unwrap();
        }

        let status = repo
            .check_tier_quota(&scope, ResourceKind::FeedbackInRollingMonth)
            .await
            .unwrap();
        assert!(status.allowed, "Free at 49/50 must still allow");
        assert_eq!(status.current, 49);
        assert_eq!(status.limit, Some(50));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn free_feedback_at_cap_is_blocked(pool: PgPool) {
        use crate::projects::ProjectRepo;
        let repo = SqlxTierQuotaRepo::new(pool.clone());
        let projects = SqlxProjectRepo::new(pool.clone());
        let feedback = SqlxFeedbackRepo::new(pool.clone());
        let scope = seed_tenant_with_tier(&pool, "free-fb-50@example.com", "free").await;
        let p = projects.create(&scope, "P", "p").await.unwrap();
        let pscope = projects.open(&scope, p.id).await.unwrap();

        for i in 0..50 {
            feedback
                .submit_anonymous(
                    &pscope,
                    &[u8::try_from(i % 251).unwrap(); 32],
                    None,
                    &format!("fb {i}"),
                    FeedbackKind::Other,
                )
                .await
                .unwrap();
        }

        let status = repo
            .check_tier_quota(&scope, ResourceKind::FeedbackInRollingMonth)
            .await
            .unwrap();
        assert!(!status.allowed, "Free at 50/50 must block");
        assert_eq!(status.current, 50);
        assert_eq!(status.limit, Some(50));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn self_host_feedback_unlimited(pool: PgPool) {
        let repo = SqlxTierQuotaRepo::new(pool.clone());
        let scope = seed_tenant_with_tier(&pool, "sh@example.com", "self_host").await;
        let status = repo
            .check_tier_quota(&scope, ResourceKind::FeedbackInRollingMonth)
            .await
            .unwrap();
        assert!(status.allowed);
        assert_eq!(status.tier, Tier::SelfHost);
        assert_eq!(status.limit, None);
    }

    // ---- get_tier_status aggregate read -------------------------------------

    #[sqlx::test(migrations = "../../migrations")]
    async fn get_tier_status_returns_quotas_and_usage(pool: PgPool) {
        use crate::projects::ProjectRepo;
        let repo = SqlxTierQuotaRepo::new(pool.clone());
        let projects = SqlxProjectRepo::new(pool.clone());
        let scope = seed_tenant_with_tier(&pool, "ts@example.com", "starter").await;
        projects.create(&scope, "P", "p").await.unwrap();

        let status = repo.get_tier_status(&scope).await.unwrap();
        assert_eq!(status.tier, Tier::Starter);
        // Static quota matches Contract C19 Starter row.
        assert_eq!(status.quotas.projects_per_org, Some(3));
        assert_eq!(status.quotas.monthly_feedback_volume, Some(500));
        assert_eq!(status.quotas.footer_text, None);
        // Live usage.
        assert_eq!(status.usage.projects, 1);
        assert_eq!(status.usage.feedback_monthly, 0);
        // period_start is ~30 days before now.
        let now = Utc::now();
        let elapsed = now - status.usage.period_start;
        assert!(elapsed.num_days() >= 29 && elapsed.num_days() <= 31);
    }
}
