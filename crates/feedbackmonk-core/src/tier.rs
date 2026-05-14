//! Pricing-tier model — Contract C17/C19 (P3 Stage 1).
//!
//! `Tier` is the four-variant enum that drives commercial-cap enforcement
//! (FR-FBR-14): `Free | Starter | Pro | SelfHost`. DB-side string form
//! mirrors DEC-FBR-03 wire values: `'free' | 'starter' | 'pro' | 'self_host'`.
//!
//! `tier_quotas(tier) -> TierQuotas` is the **single source of truth** for
//! per-tier caps + capability flags + footer copy. The
//! `tier-enforcement-status` Verification Oracle Probe B asserts this
//! function's shape against Contract C19 — drift is a code-level invariant
//! violation, not a soft warning.
//!
//! No DB access here (pure data, mirrors `feedbackmonk-core` discipline).
//! The repository layer reads `tenants.tier` as TEXT and converts via
//! `Tier::from_db_str` (mirroring the existing `FeedbackStatus` /
//! `FeedbackKind` conventions). A migration adds a `CHECK (tier IN
//! (...))` constraint at the schema level for defense-in-depth.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Pricing tier. Four variants per DEC-FBR-03.
///
/// DB-side / JSON-side string form is **lowercase, snake_case for SelfHost**
/// per DEC-FBR-03: `"free" | "starter" | "pro" | "self_host"`. Serde
/// rename matches this; the `as_db_str` / `from_db_str` helpers are the
/// canonical conversion path used by `feedbackmonk-repository`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    #[default]
    Free,
    Starter,
    Pro,
    SelfHost,
}

impl Tier {
    /// DB / wire string form. Must match the CHECK constraint added by
    /// migration `00008_tenant_tier_check.sql` byte-for-byte.
    #[must_use]
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Starter => "starter",
            Self::Pro => "pro",
            Self::SelfHost => "self_host",
        }
    }

    /// Strict DB-string parser. Unknown values return `Err`. The CHECK
    /// constraint guarantees only canonical values reach this path; we
    /// surface a hard error rather than silently falling back to Free,
    /// because a Free fallback could mask a corrupted row at security
    /// cost (an unexpectedly-Free Pro tenant would suddenly hit caps).
    pub fn from_db_str(s: &str) -> Result<Self, TierParseError> {
        match s {
            "free" => Ok(Self::Free),
            "starter" => Ok(Self::Starter),
            "pro" => Ok(Self::Pro),
            "self_host" => Ok(Self::SelfHost),
            other => Err(TierParseError(other.to_string())),
        }
    }
}

/// Error returned by `Tier::from_db_str` for unrecognised values. The
/// schema-level `CHECK (tier IN ('free','starter','pro','self_host'))`
/// makes this practically unreachable, but the type is exposed so the
/// repository layer can propagate it cleanly.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown tier value {0:?} — expected one of free|starter|pro|self_host")]
pub struct TierParseError(pub String);

/// Resources that consume tier quota. Used by `check_tier_quota(scope, resource)`
/// in the repository layer.
///
/// - `Project`: counted as `SELECT COUNT(*) FROM projects WHERE tenant_id = ?`
/// - `FeedbackInRollingMonth`: counted as `SELECT COUNT(*) FROM feedback WHERE
///   project_id IN (SELECT id FROM projects WHERE tenant_id = ?)
///   AND accepted_at > now() - interval '30 days'`
///
/// New tier-capped resources require:
/// 1. A new variant here.
/// 2. A new field in `TierQuotas`.
/// 3. A new `count_*` method on `TenantRepo`.
/// 4. A wiring site in the relevant handler (and a `tier-enforcement-status`
///    oracle re-run to confirm coverage).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    /// Per-tenant project count cap.
    Project,
    /// Rolling 30-day feedback-submission volume cap.
    FeedbackInRollingMonth,
}

impl ResourceKind {
    /// Stable wire form. Used in `TierCapExceededBody.resource` per Contract C18.
    #[must_use]
    pub fn as_wire_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::FeedbackInRollingMonth => "feedback_in_rolling_month",
        }
    }
}

/// Static per-tier capability matrix. The fields' values come from
/// **Contract C19** (P3 plan §Interface Contracts) — the
/// `tier-enforcement-status` Verification Oracle Probe B asserts this
/// shape against the canonical token set per variant.
///
/// `Option<i64>` semantics:
/// - `Some(n)` → hard cap at `n` resources.
/// - `None` → unlimited (Pro/SelfHost projects; SelfHost feedback volume).
///
/// `footer_text` semantics (FR-FBR-14 brand promise enforcement):
/// - `Some("powered by feedbackmonk")` on Free tier (load-bearing).
/// - `None` on every paid tier — the widget renders no footer.
///
/// Capability flags (`custom_branding`, `custom_domain`, `eu_residency`)
/// are forward-looking — they gate feature availability in P4+; the
/// flags themselves are part of the C19 canonical shape today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TierQuotas {
    pub projects_per_org: Option<i64>,
    pub monthly_feedback_volume: Option<i64>,
    pub custom_branding: bool,
    pub custom_domain: bool,
    pub eu_residency: bool,
    pub footer_text: Option<&'static str>,
}

impl TierQuotas {
    /// Resolve the cap for a given resource kind. Returns `None` when
    /// the resource is unlimited for this tier.
    #[must_use]
    pub fn limit_for(&self, resource: ResourceKind) -> Option<i64> {
        match resource {
            ResourceKind::Project => self.projects_per_org,
            ResourceKind::FeedbackInRollingMonth => self.monthly_feedback_volume,
        }
    }
}

/// Per-tier static config. **Contract C19** verbatim — any change here
/// requires a `DEC-FBR-*` decision entry and the oracle's Probe B will
/// flag drift.
#[must_use]
pub const fn tier_quotas(tier: Tier) -> TierQuotas {
    match tier {
        Tier::Free => TierQuotas {
            projects_per_org: Some(1),
            monthly_feedback_volume: Some(50),
            custom_branding: false,
            custom_domain: false,
            eu_residency: false,
            footer_text: Some("powered by feedbackmonk"),
        },
        Tier::Starter => TierQuotas {
            projects_per_org: Some(3),
            monthly_feedback_volume: Some(500),
            custom_branding: true,
            custom_domain: false,
            eu_residency: false,
            footer_text: None,
        },
        Tier::Pro => TierQuotas {
            projects_per_org: None,
            monthly_feedback_volume: Some(10000),
            custom_branding: true,
            custom_domain: true,
            eu_residency: true,
            footer_text: None,
        },
        Tier::SelfHost => TierQuotas {
            projects_per_org: None,
            monthly_feedback_volume: None,
            custom_branding: true,
            custom_domain: true,
            eu_residency: true,
            footer_text: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_strings_round_trip() {
        for t in [Tier::Free, Tier::Starter, Tier::Pro, Tier::SelfHost] {
            assert_eq!(Tier::from_db_str(t.as_db_str()).unwrap(), t);
        }
    }

    #[test]
    fn from_db_str_rejects_unknown() {
        let err = Tier::from_db_str("enterprise").unwrap_err();
        assert_eq!(err, TierParseError("enterprise".into()));
    }

    #[test]
    fn default_tier_is_free() {
        // Mirrors migration 00001's `tier TEXT NOT NULL DEFAULT 'free'`.
        assert_eq!(Tier::default(), Tier::Free);
    }

    #[test]
    fn serde_round_trip_uses_snake_case() {
        // Wire form per DEC-FBR-03 + Contract C18: lowercase with
        // snake_case for the multi-word variant.
        let pairs = [
            (Tier::Free, "\"free\""),
            (Tier::Starter, "\"starter\""),
            (Tier::Pro, "\"pro\""),
            (Tier::SelfHost, "\"self_host\""),
        ];
        for (tier, expected) in pairs {
            assert_eq!(serde_json::to_string(&tier).unwrap(), expected);
            assert_eq!(serde_json::from_str::<Tier>(expected).unwrap(), tier);
        }
    }

    #[test]
    fn resource_kind_wire_form() {
        assert_eq!(ResourceKind::Project.as_wire_str(), "project");
        assert_eq!(
            ResourceKind::FeedbackInRollingMonth.as_wire_str(),
            "feedback_in_rolling_month"
        );
    }

    // ------ Contract C19 verbatim assertions (paired with oracle Probe B) ------

    #[test]
    fn c19_free_tier_shape() {
        let q = tier_quotas(Tier::Free);
        assert_eq!(q.projects_per_org, Some(1));
        assert_eq!(q.monthly_feedback_volume, Some(50));
        assert!(!q.custom_branding);
        assert!(!q.custom_domain);
        assert!(!q.eu_residency);
        assert_eq!(q.footer_text, Some("powered by feedbackmonk"));
    }

    #[test]
    fn c19_starter_tier_shape() {
        let q = tier_quotas(Tier::Starter);
        assert_eq!(q.projects_per_org, Some(3));
        assert_eq!(q.monthly_feedback_volume, Some(500));
        assert!(q.custom_branding);
        assert!(!q.custom_domain);
        assert!(!q.eu_residency);
        assert_eq!(q.footer_text, None);
    }

    #[test]
    fn c19_pro_tier_shape() {
        let q = tier_quotas(Tier::Pro);
        assert_eq!(q.projects_per_org, None);
        assert_eq!(q.monthly_feedback_volume, Some(10000));
        assert!(q.custom_branding);
        assert!(q.custom_domain);
        assert!(q.eu_residency);
        assert_eq!(q.footer_text, None);
    }

    #[test]
    fn c19_self_host_tier_shape() {
        let q = tier_quotas(Tier::SelfHost);
        assert_eq!(q.projects_per_org, None);
        assert_eq!(q.monthly_feedback_volume, None);
        assert!(q.custom_branding);
        assert!(q.custom_domain);
        assert!(q.eu_residency);
        assert_eq!(q.footer_text, None);
    }

    #[test]
    fn limit_for_resource_kind() {
        let free = tier_quotas(Tier::Free);
        assert_eq!(free.limit_for(ResourceKind::Project), Some(1));
        assert_eq!(free.limit_for(ResourceKind::FeedbackInRollingMonth), Some(50));

        let pro = tier_quotas(Tier::Pro);
        assert_eq!(pro.limit_for(ResourceKind::Project), None);
        assert_eq!(pro.limit_for(ResourceKind::FeedbackInRollingMonth), Some(10000));

        let self_host = tier_quotas(Tier::SelfHost);
        assert_eq!(self_host.limit_for(ResourceKind::Project), None);
        assert_eq!(self_host.limit_for(ResourceKind::FeedbackInRollingMonth), None);
    }

    #[test]
    fn only_free_tier_carries_footer() {
        // Brand-promise enforcement at the data level: footer is ONLY
        // emitted on Free, NEVER on paid tiers. Oracle Probe B + this
        // unit test form the redundant pair.
        for t in [Tier::Starter, Tier::Pro, Tier::SelfHost] {
            assert_eq!(tier_quotas(t).footer_text, None, "paid tier {t:?} must NOT have footer");
        }
        assert!(tier_quotas(Tier::Free).footer_text.is_some());
    }
}
