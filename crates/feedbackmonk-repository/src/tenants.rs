//! Tenant repository -- the only place tenant rows are created or read.
//!
//! `create` and `find_by_email` are the documented pre-authentication exceptions
//! to the `&TenantScope`-first-arg discipline (no scope can exist before a
//! tenant is identified). Both are listed in
//! `.claude/oracles/multi-tenant-isolation-check/allowlist.toml` with rationale.

use async_trait::async_trait;
use sqlx::PgPool;

use feedbackmonk_core::{tier_quotas, Tenant, Tier, WidgetBrand};

use crate::error::{RepoError, Result};
use crate::scope::TenantScope;

#[async_trait]
pub trait TenantRepo: Send + Sync {
    // allowlisted-pre-auth: signup creates the tenant before any scope exists.
    async fn create(&self, email: &str, password_hash: &str) -> Result<Tenant>;

    // allowlisted-pre-auth: login lookup runs before password verification.
    async fn find_by_email(&self, email: &str) -> Result<Option<Tenant>>;

    async fn get(&self, scope: &TenantScope) -> Result<Tenant>;

    async fn mark_verified(&self, scope: &TenantScope) -> Result<()>;

    /// Mint a `TenantScope` for the tenant identified by `id`. This is the
    /// SOLE bridge from a raw `Uuid` to a `TenantScope`; callers must have
    /// already authenticated the bearer (e.g. validated a session cookie or
    /// verified a password). Stage 2 Worker A wraps this behind login/session
    /// handlers; Stage 1 exposes it for the test harness.
    async fn scope_for(&self, id: uuid::Uuid) -> Result<TenantScope>;

    /// Read the email-template brand parameters for `scope` (Contract C10).
    ///
    /// We add `get_brand(&scope)` rather than widening `find_by_email` to
    /// include brand fields: `find_by_email` is allow-listed as a pre-auth
    /// exception (no scope exists at lookup time), and exposing brand
    /// columns through it would unnecessarily widen the pre-auth surface.
    /// `get_brand` requires a `&TenantScope`, so the multi-tenant-isolation
    /// invariant holds the same shape as the rest of the post-auth surface.
    async fn get_brand(&self, scope: &TenantScope) -> Result<EmailTenantBrand>;

    /// Update the brand parameters for `scope`. Stage 2 Worker A wires a
    /// PATCH endpoint on top of this; Stage 1 ships only the repo surface.
    async fn update_brand(&self, scope: &TenantScope, brand: &EmailTenantBrand) -> Result<()>;

    /// Read the widget runtime brand surface for `scope` (Contract C12).
    ///
    /// Sibling of `get_brand`, smaller payload: the fields the embeddable
    /// widget needs. Resolution layers the per-tenant override columns
    /// (migration 00012) on top of the tier default (DEC-FBR-IMPL-11/12):
    /// - `footer_text`: `footer_text_override` NULL ⇒ `tier_quotas(tier).footer_text`
    ///   (FR-FBR-14 default for external Free tenants); `Some("")` ⇒ suppressed
    ///   (`None`); `Some(text)` ⇒ custom. `tier_quotas()` itself is unchanged.
    /// - `primary_color` / `logo_url` / `theme` / `footer_url`: passed through
    ///   from the override columns (NULL ⇒ widget falls back to its CSS / hard
    ///   default).
    async fn get_widget_brand(&self, scope: &TenantScope) -> Result<WidgetBrand>;

    /// Read the raw per-tenant widget brand override columns for `scope`
    /// (migration 00012). Unlike `get_widget_brand`, this does NOT resolve
    /// against the tier default — it returns the stored override values
    /// verbatim, for the ops endpoint to display/confirm current overrides.
    async fn get_widget_brand_override(
        &self,
        scope: &TenantScope,
    ) -> Result<WidgetBrandOverride>;

    /// Replace the per-tenant widget brand override columns for `scope`
    /// (migration 00012). Full-replace (PUT) semantics: every override column
    /// is set to the supplied value, `None` clearing it. Written ONLY via the
    /// ops mutation endpoint (DEC-FBR-IMPL-11) — never tenant-self-serve, so
    /// external Free tenants cannot strip the FR-FBR-14 badge.
    async fn set_widget_brand_override(
        &self,
        scope: &TenantScope,
        over: &WidgetBrandOverride,
    ) -> Result<()>;

    /// Production tier writer (DEC-FBR-IMPL-11). Sets `tenants.tier` for
    /// `scope`. Scope-bound (multi-tenant-isolation compliant) — the ops
    /// handler mints the scope via `scope_for` after validating the ops token.
    /// Complements DEC-FBR-DEFER-01: Polar will be the *self-service* tier
    /// writer when billing lands; this is the *operator* path that supersedes
    /// the SQL-only workflow of `docs/operations/TIER_OVERRIDE.md`. Distinct
    /// from the test-only `set_tier_for_test`.
    async fn set_tier(&self, scope: &TenantScope, tier: Tier) -> Result<()>;

    /// Read the pricing tier for `scope` (P3 Stage 1, Contract C17).
    ///
    /// Reads `tenants.tier` and parses via strict `Tier::from_db_str`. The
    /// schema CHECK constraint (`tenants_tier_check`, migration 00008)
    /// guarantees only canonical values reach this path — an unknown value
    /// indicates DB tampering and surfaces as `RepoError::Sqlx` via the
    /// `TierParseError` -> `sqlx::Error::Decode` mapping in the impl.
    async fn get_tier(&self, scope: &TenantScope) -> Result<Tier>;

    /// Count the projects owned by `scope` (P3 Stage 1, Contract C17).
    /// Backs the `Tier::Free.projects_per_org = Some(1)` cap check.
    async fn count_projects(&self, scope: &TenantScope) -> Result<i64>;

    /// Count feedback rows submitted to ANY project owned by `scope`
    /// within the rolling `window_days` day window (P3 Stage 1, Contract C17).
    /// Backs the `monthly_feedback_volume` cap check; window is parameterised
    /// so tests can vary it cheaply.
    async fn count_feedback_in_window(
        &self,
        scope: &TenantScope,
        window_days: i64,
    ) -> Result<i64>;
}

/// Tenant email-template brand parameters (Contract C10).
///
/// `sender_display_name` is COMPUTED (`"{brand_name} via feedbackmonk"`) and
/// therefore lives in the constructor below, not in the DB columns. All
/// other fields map 1:1 onto migration 00005's columns.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EmailTenantBrand {
    pub brand_name: String,
    pub email_subject_prefix: String,
    pub support_email: String,
    pub unsubscribe_url: Option<String>,
    pub footer_signature: String,
    pub sender_display_name: String,
}

impl EmailTenantBrand {
    /// Build from raw DB column values; derives `sender_display_name`.
    #[must_use]
    pub fn from_db(
        brand_name: String,
        email_subject_prefix: String,
        support_email: String,
        unsubscribe_url: Option<String>,
        footer_signature: String,
    ) -> Self {
        let sender_display_name = format!("{brand_name} via feedbackmonk");
        Self {
            brand_name,
            email_subject_prefix,
            support_email,
            unsubscribe_url,
            footer_signature,
            sender_display_name,
        }
    }
}

/// Per-tenant widget brand override (migration 00012; DEC-FBR-IMPL-11/12).
///
/// Each field maps 1:1 to a nullable `tenants` column. `None` = no override
/// for that field (fall through to the tier default for `footer_text_override`,
/// or the widget's own CSS/hardcoded default for the rest). `footer_text_override
/// = Some("")` is the explicit-suppress sentinel (widget renders no footer);
/// `Some(text)` is custom footer copy. Written only via the ops endpoint.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WidgetBrandOverride {
    pub footer_text_override: Option<String>,
    pub footer_url: Option<String>,
    pub theme: Option<String>,
    pub primary_color: Option<String>,
    pub logo_url: Option<String>,
}

#[derive(Clone)]
pub struct SqlxTenantRepo {
    pool: PgPool,
}

impl SqlxTenantRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Test-only direct tier override. Writes `tenants.tier` for the
    /// given `tenant_id` WITHOUT going through any `TenantScope` (the
    /// caller is asserting they want the change applied regardless of
    /// authentication state). Used by integration tests in
    /// `feedbackmonk-api/src/handlers/admin_tier.rs::tests` and
    /// elsewhere to seed tenants at specific tiers. Allowlisted as an
    /// inherent method in
    /// `.claude/oracles/multi-tenant-isolation-check/allowlist.toml`
    /// (no `&TenantScope` first arg — pre-test boundary, not pre-auth).
    ///
    /// **NOT** intended for production code. The production path for
    /// tier writes will be the Polar webhook receiver (DEC-FBR-DEFER-01,
    /// deferred); the operator-runbook path is direct SQL per
    /// `docs/operations/TIER_OVERRIDE.md`.
    pub async fn set_tier_for_test(
        &self,
        tenant_id: uuid::Uuid,
        tier_db_str: &str,
    ) -> Result<()> {
        sqlx::query!(
            "UPDATE tenants SET tier = $2, updated_at = now() WHERE id = $1",
            tenant_id,
            tier_db_str,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[async_trait]
impl TenantRepo for SqlxTenantRepo {
    async fn create(&self, email: &str, password_hash: &str) -> Result<Tenant> {
        // Brand-column defaults are derived from the email local-part the
        // same way migration 00005 backfilled existing rows: keeps NEW signups
        // (post-00005) and OLD signups (pre-00005) byte-identical. Tenants
        // can override later via `update_brand`.
        let local_part = email.split('@').next().unwrap_or("admin");
        let footer = format!("— The {local_part} team");
        let row = sqlx::query!(
            r#"
            INSERT INTO tenants (
                email, password_hash,
                brand_name, email_subject_prefix, support_email, footer_signature
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id, email, password_hash, verified_at, tier, created_at, updated_at
            "#,
            email,
            password_hash,
            local_part,
            local_part,
            email,
            footer,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.is_unique_violation() => RepoError::Conflict,
            other => RepoError::Sqlx(other),
        })?;

        Ok(Tenant {
            id: row.id,
            email: row.email,
            password_hash: row.password_hash,
            verified_at: row.verified_at,
            tier: row.tier,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }

    async fn find_by_email(&self, email: &str) -> Result<Option<Tenant>> {
        let row = sqlx::query!(
            r#"
            SELECT id, email, password_hash, verified_at, tier, created_at, updated_at
            FROM tenants WHERE email = $1
            "#,
            email
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| Tenant {
            id: r.id,
            email: r.email,
            password_hash: r.password_hash,
            verified_at: r.verified_at,
            tier: r.tier,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }))
    }

    async fn get(&self, scope: &TenantScope) -> Result<Tenant> {
        let row = sqlx::query!(
            r#"
            SELECT id, email, password_hash, verified_at, tier, created_at, updated_at
            FROM tenants WHERE id = $1
            "#,
            scope.tenant_id()
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(RepoError::NotFound)?;

        Ok(Tenant {
            id: row.id,
            email: row.email,
            password_hash: row.password_hash,
            verified_at: row.verified_at,
            tier: row.tier,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }

    async fn mark_verified(&self, scope: &TenantScope) -> Result<()> {
        sqlx::query!(
            "UPDATE tenants SET verified_at = now(), updated_at = now() WHERE id = $1",
            scope.tenant_id()
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn scope_for(&self, id: uuid::Uuid) -> Result<TenantScope> {
        let exists = sqlx::query!("SELECT id FROM tenants WHERE id = $1", id)
            .fetch_optional(&self.pool)
            .await?;
        if exists.is_none() {
            return Err(RepoError::NotFound);
        }
        Ok(TenantScope::new(id))
    }

    async fn get_brand(&self, scope: &TenantScope) -> Result<EmailTenantBrand> {
        let row = sqlx::query!(
            r#"
            SELECT brand_name, email_subject_prefix, support_email,
                   unsubscribe_url, footer_signature
            FROM tenants
            WHERE id = $1
            "#,
            scope.tenant_id(),
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(RepoError::NotFound)?;

        Ok(EmailTenantBrand::from_db(
            row.brand_name,
            row.email_subject_prefix,
            row.support_email,
            row.unsubscribe_url,
            row.footer_signature,
        ))
    }

    async fn update_brand(&self, scope: &TenantScope, brand: &EmailTenantBrand) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE tenants
            SET brand_name           = $2,
                email_subject_prefix = $3,
                support_email        = $4,
                unsubscribe_url      = $5,
                footer_signature     = $6,
                updated_at           = now()
            WHERE id = $1
            "#,
            scope.tenant_id(),
            brand.brand_name,
            brand.email_subject_prefix,
            brand.support_email,
            brand.unsubscribe_url,
            brand.footer_signature,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_widget_brand(&self, scope: &TenantScope) -> Result<WidgetBrand> {
        // DEC-FBR-IMPL-11/12: resolve the per-tenant override columns
        // (migration 00012) over the tier default. `tier_quotas()` is the
        // unchanged footer SSOT (oracle Probe B still pins its shape); the
        // override is a layer above it.
        let row = sqlx::query!(
            r#"
            SELECT tier, footer_text_override, footer_url,
                   widget_theme, widget_primary_color, widget_logo_url
            FROM tenants WHERE id = $1
            "#,
            scope.tenant_id(),
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(RepoError::NotFound)?;

        let tier = Tier::from_db_str(&row.tier).map_err(|e| {
            tracing::error!(
                tenant_id = %scope.tenant_id(),
                bad_tier = %e.0,
                "tenants.tier column violates CHECK constraint (data corruption)"
            );
            // Map to NotFound to avoid leaking the bad value; the operator
            // log carries the diagnostic. Schema CHECK makes this practically
            // unreachable, but Defense-in-Depth is the point.
            RepoError::NotFound
        })?;

        // Footer resolution (FR-FBR-14 default preserved when override is NULL):
        //   NULL       → tier default (Some("powered by feedbackmonk") on Free)
        //   Some("")   → explicit suppress → None (widget renders no footer)
        //   Some(text) → custom footer text
        let footer_text = match row.footer_text_override {
            None => tier_quotas(tier).footer_text.map(str::to_string),
            Some(s) if s.is_empty() => None,
            Some(s) => Some(s),
        };

        Ok(WidgetBrand {
            // NULL ⇒ widget uses its WCAG-AA-safe #2563eb CSS default.
            primary_color: row.widget_primary_color,
            logo_url: row.widget_logo_url,
            footer_text,
            // NULL ⇒ widget defaults the badge href to https://feedbackmonk.com.
            footer_url: row.footer_url,
            // NULL ⇒ widget resolves 'auto'.
            theme: row.widget_theme,
        })
    }

    async fn get_widget_brand_override(
        &self,
        scope: &TenantScope,
    ) -> Result<WidgetBrandOverride> {
        let row = sqlx::query!(
            r#"
            SELECT footer_text_override, footer_url,
                   widget_theme, widget_primary_color, widget_logo_url
            FROM tenants WHERE id = $1
            "#,
            scope.tenant_id(),
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(RepoError::NotFound)?;

        Ok(WidgetBrandOverride {
            footer_text_override: row.footer_text_override,
            footer_url: row.footer_url,
            theme: row.widget_theme,
            primary_color: row.widget_primary_color,
            logo_url: row.widget_logo_url,
        })
    }

    async fn set_widget_brand_override(
        &self,
        scope: &TenantScope,
        over: &WidgetBrandOverride,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE tenants
            SET footer_text_override = $2,
                footer_url           = $3,
                widget_theme         = $4,
                widget_primary_color = $5,
                widget_logo_url      = $6,
                updated_at           = now()
            WHERE id = $1
            "#,
            scope.tenant_id(),
            over.footer_text_override,
            over.footer_url,
            over.theme,
            over.primary_color,
            over.logo_url,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn set_tier(&self, scope: &TenantScope, tier: Tier) -> Result<()> {
        sqlx::query!(
            "UPDATE tenants SET tier = $2, updated_at = now() WHERE id = $1",
            scope.tenant_id(),
            tier.as_db_str(),
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_tier(&self, scope: &TenantScope) -> Result<Tier> {
        let row = sqlx::query!(
            r#"SELECT tier FROM tenants WHERE id = $1"#,
            scope.tenant_id(),
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(RepoError::NotFound)?;
        Tier::from_db_str(&row.tier).map_err(|e| {
            tracing::error!(
                tenant_id = %scope.tenant_id(),
                bad_tier = %e.0,
                "tenants.tier column violates CHECK constraint (data corruption)"
            );
            RepoError::NotFound
        })
    }

    async fn count_projects(&self, scope: &TenantScope) -> Result<i64> {
        let row = sqlx::query!(
            r#"SELECT COUNT(*) AS "count!" FROM projects WHERE tenant_id = $1"#,
            scope.tenant_id(),
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.count)
    }

    async fn count_feedback_in_window(
        &self,
        scope: &TenantScope,
        window_days: i64,
    ) -> Result<i64> {
        // Rolling-window semantics: `accepted_at > now() - interval`. Tests
        // can drive window_days = 0 to assert "the row I just inserted
        // counts" without waiting wall-clock time.
        let row = sqlx::query!(
            r#"
            SELECT COUNT(*) AS "count!"
            FROM feedback
            WHERE tenant_id = $1
              AND accepted_at > now() - make_interval(days => $2::int)
            "#,
            scope.tenant_id(),
            i32::try_from(window_days).unwrap_or(i32::MAX),
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::PgPool;

    #[sqlx::test(migrations = "../../migrations")]
    async fn create_then_find_by_email_round_trip(pool: PgPool) {
        let repo = SqlxTenantRepo::new(pool);
        let t = repo.create("alice@example.com", "argon2-hash-stub").await.unwrap();
        assert_eq!(t.email, "alice@example.com");
        assert!(t.verified_at.is_none());

        let found = repo.find_by_email("alice@example.com").await.unwrap();
        assert_eq!(found.unwrap().id, t.id);

        let missing = repo.find_by_email("nobody@example.com").await.unwrap();
        assert!(missing.is_none());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn create_duplicate_email_yields_conflict(pool: PgPool) {
        let repo = SqlxTenantRepo::new(pool);
        repo.create("dup@example.com", "h").await.unwrap();
        let err = repo.create("dup@example.com", "h2").await.unwrap_err();
        assert!(matches!(err, RepoError::Conflict));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn scope_for_unknown_tenant_returns_not_found(pool: PgPool) {
        let repo = SqlxTenantRepo::new(pool);
        let err = repo.scope_for(uuid::Uuid::new_v4()).await.unwrap_err();
        assert!(matches!(err, RepoError::NotFound));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn get_brand_returns_backfilled_defaults(pool: PgPool) {
        let repo = SqlxTenantRepo::new(pool.clone());
        let t = repo.create("brand@example.com", "h").await.unwrap();
        let scope = repo.scope_for(t.id).await.unwrap();
        let brand = repo.get_brand(&scope).await.unwrap();
        // Migration 00005's backfill: local-part of email -> brand_name.
        assert_eq!(brand.brand_name, "brand");
        assert_eq!(brand.email_subject_prefix, "brand");
        assert_eq!(brand.support_email, "brand@example.com");
        assert_eq!(brand.unsubscribe_url, None);
        assert!(brand.footer_signature.contains("brand"));
        assert_eq!(brand.sender_display_name, "brand via feedbackmonk");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn update_brand_round_trips(pool: PgPool) {
        let repo = SqlxTenantRepo::new(pool.clone());
        let t = repo.create("update@example.com", "h").await.unwrap();
        let scope = repo.scope_for(t.id).await.unwrap();
        let updated = EmailTenantBrand::from_db(
            "Acme".into(),
            "ACME".into(),
            "help@acme.example".into(),
            Some("https://acme.example/unsub".into()),
            "— The Acme team".into(),
        );
        repo.update_brand(&scope, &updated).await.unwrap();

        let read_back = repo.get_brand(&scope).await.unwrap();
        assert_eq!(read_back.brand_name, "Acme");
        assert_eq!(read_back.email_subject_prefix, "ACME");
        assert_eq!(read_back.support_email, "help@acme.example");
        assert_eq!(
            read_back.unsubscribe_url.as_deref(),
            Some("https://acme.example/unsub")
        );
        assert_eq!(read_back.footer_signature, "— The Acme team");
        assert_eq!(read_back.sender_display_name, "Acme via feedbackmonk");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn get_brand_cross_tenant_negative(pool: PgPool) {
        let repo = SqlxTenantRepo::new(pool.clone());
        let t1 = repo.create("a@example.com", "h").await.unwrap();
        let t2 = repo.create("b@example.com", "h").await.unwrap();
        let scope1 = repo.scope_for(t1.id).await.unwrap();
        let scope2 = repo.scope_for(t2.id).await.unwrap();
        // Each scope sees only its own brand.
        let b1 = repo.get_brand(&scope1).await.unwrap();
        let b2 = repo.get_brand(&scope2).await.unwrap();
        assert_ne!(b1.brand_name, b2.brand_name);
        assert_eq!(b1.brand_name, "a");
        assert_eq!(b2.brand_name, "b");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn mark_verified_sets_timestamp(pool: PgPool) {
        let repo = SqlxTenantRepo::new(pool.clone());
        let t = repo.create("verify@example.com", "h").await.unwrap();
        let scope = repo.scope_for(t.id).await.unwrap();
        repo.mark_verified(&scope).await.unwrap();
        let after = repo.get(&scope).await.unwrap();
        assert!(after.verified_at.is_some());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn get_widget_brand_returns_free_tier_defaults(pool: PgPool) {
        let repo = SqlxTenantRepo::new(pool.clone());
        let t = repo.create("widget@example.com", "h").await.unwrap();
        let scope = repo.scope_for(t.id).await.unwrap();
        let brand = repo.get_widget_brand(&scope).await.unwrap();
        // Fresh tenant: no overrides set → brand fields fall through to
        // widget CSS/hard defaults (None), footer to the Free tier default.
        assert_eq!(brand.primary_color, None);
        assert_eq!(brand.logo_url, None);
        assert_eq!(brand.footer_url, None);
        assert_eq!(brand.theme, None);
        assert_eq!(brand.footer_text.as_deref(), Some("powered by feedbackmonk"));
    }

    // ----- DEC-FBR-IMPL-11/12: per-tenant brand override resolution -----

    #[sqlx::test(migrations = "../../migrations")]
    async fn footer_override_empty_string_suppresses_on_free_tier(pool: PgPool) {
        // FR-FBR-14 decoupling: a Free tenant whose admin set
        // footer_text_override = "" renders NO footer, while quotas stay Free.
        let repo = SqlxTenantRepo::new(pool.clone());
        let t = repo.create("suppress@example.com", "h").await.unwrap();
        let scope = repo.scope_for(t.id).await.unwrap();

        repo.set_widget_brand_override(
            &scope,
            &WidgetBrandOverride {
                footer_text_override: Some(String::new()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let brand = repo.get_widget_brand(&scope).await.unwrap();
        assert_eq!(brand.footer_text, None, "empty override must suppress footer");
        // Tier is untouched — still Free.
        assert_eq!(repo.get_tier(&scope).await.unwrap(), Tier::Free);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn footer_override_custom_text_supersedes_tier_default(pool: PgPool) {
        let repo = SqlxTenantRepo::new(pool.clone());
        let t = repo.create("custom@example.com", "h").await.unwrap();
        let scope = repo.scope_for(t.id).await.unwrap();

        repo.set_widget_brand_override(
            &scope,
            &WidgetBrandOverride {
                footer_text_override: Some("feedback by Acme".into()),
                footer_url: Some("https://acme.example/feedback".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let brand = repo.get_widget_brand(&scope).await.unwrap();
        assert_eq!(brand.footer_text.as_deref(), Some("feedback by Acme"));
        assert_eq!(brand.footer_url.as_deref(), Some("https://acme.example/feedback"));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn theme_color_logo_overrides_pass_through(pool: PgPool) {
        let repo = SqlxTenantRepo::new(pool.clone());
        let t = repo.create("theme@example.com", "h").await.unwrap();
        let scope = repo.scope_for(t.id).await.unwrap();

        repo.set_widget_brand_override(
            &scope,
            &WidgetBrandOverride {
                theme: Some("dark".into()),
                primary_color: Some("#7c3aed".into()),
                logo_url: Some("https://acme.example/logo.svg".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let brand = repo.get_widget_brand(&scope).await.unwrap();
        assert_eq!(brand.theme.as_deref(), Some("dark"));
        assert_eq!(brand.primary_color.as_deref(), Some("#7c3aed"));
        assert_eq!(brand.logo_url.as_deref(), Some("https://acme.example/logo.svg"));
        // Footer untouched → Free tier default still present.
        assert_eq!(brand.footer_text.as_deref(), Some("powered by feedbackmonk"));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn brand_override_round_trips_and_full_replace_clears(pool: PgPool) {
        let repo = SqlxTenantRepo::new(pool.clone());
        let t = repo.create("rt@example.com", "h").await.unwrap();
        let scope = repo.scope_for(t.id).await.unwrap();

        let set = WidgetBrandOverride {
            footer_text_override: Some(String::new()),
            footer_url: Some("https://x.example".into()),
            theme: Some("light".into()),
            primary_color: Some("#123456".into()),
            logo_url: Some("https://x.example/l.png".into()),
        };
        repo.set_widget_brand_override(&scope, &set).await.unwrap();
        assert_eq!(repo.get_widget_brand_override(&scope).await.unwrap(), set);

        // Full-replace (PUT) with all-None clears every override column.
        repo.set_widget_brand_override(&scope, &WidgetBrandOverride::default())
            .await
            .unwrap();
        assert_eq!(
            repo.get_widget_brand_override(&scope).await.unwrap(),
            WidgetBrandOverride::default()
        );
        // Footer falls back to Free tier default after clearing.
        assert_eq!(
            repo.get_widget_brand(&scope).await.unwrap().footer_text.as_deref(),
            Some("powered by feedbackmonk")
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn set_tier_writes_each_canonical_value(pool: PgPool) {
        let repo = SqlxTenantRepo::new(pool.clone());
        let t = repo.create("settier@example.com", "h").await.unwrap();
        let scope = repo.scope_for(t.id).await.unwrap();

        for tier in [Tier::Starter, Tier::Pro, Tier::SelfHost, Tier::Free] {
            repo.set_tier(&scope, tier).await.unwrap();
            assert_eq!(repo.get_tier(&scope).await.unwrap(), tier);
        }
    }

    // ----- P3 Stage 1: tier-aware repo methods (4 sqlx::test) -----

    /// Helper for tier-aware tests: directly UPDATE the tier column.
    /// Lives INSIDE the repository crate so the multi-tenant-isolation
    /// oracle Probe A (which scans OUTSIDE this crate) does not fire.
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

    #[sqlx::test(migrations = "../../migrations")]
    async fn get_tier_reads_each_canonical_value(pool: PgPool) {
        use feedbackmonk_core::Tier;
        let repo = SqlxTenantRepo::new(pool.clone());

        for (db_str, expected) in [
            ("free", Tier::Free),
            ("starter", Tier::Starter),
            ("pro", Tier::Pro),
            ("self_host", Tier::SelfHost),
        ] {
            let t = repo
                .create(&format!("{db_str}@example.com"), "h")
                .await
                .unwrap();
            set_tier(&pool, t.id, db_str).await;
            let scope = repo.scope_for(t.id).await.unwrap();
            let tier = repo.get_tier(&scope).await.unwrap();
            assert_eq!(tier, expected, "{db_str} should parse to {expected:?}");
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn get_widget_brand_flips_footer_per_tier(pool: PgPool) {
        // FR-FBR-14 brand-promise enforcement at the repo layer: Free
        // tenant gets the footer; every paid tier gets None.
        let repo = SqlxTenantRepo::new(pool.clone());

        for (db_str, expect_footer) in [
            ("free", Some("powered by feedbackmonk")),
            ("starter", None),
            ("pro", None),
            ("self_host", None),
        ] {
            let t = repo
                .create(&format!("brand-{db_str}@example.com"), "h")
                .await
                .unwrap();
            set_tier(&pool, t.id, db_str).await;
            let scope = repo.scope_for(t.id).await.unwrap();
            let brand = repo.get_widget_brand(&scope).await.unwrap();
            assert_eq!(
                brand.footer_text.as_deref(),
                expect_footer,
                "{db_str} should produce footer_text = {expect_footer:?}"
            );
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn count_projects_empty_and_non_empty(pool: PgPool) {
        use crate::projects::{ProjectRepo, SqlxProjectRepo};
        let repo = SqlxTenantRepo::new(pool.clone());
        let projects_repo = SqlxProjectRepo::new(pool.clone());

        let t = repo.create("count@example.com", "h").await.unwrap();
        let scope = repo.scope_for(t.id).await.unwrap();

        // Empty case.
        assert_eq!(repo.count_projects(&scope).await.unwrap(), 0);

        // One project.
        projects_repo.create(&scope, "P1", "p1").await.unwrap();
        assert_eq!(repo.count_projects(&scope).await.unwrap(), 1);

        // Second project (Free tier cap would reject; the count itself
        // is what `check_tier_quota` consults, not enforced here).
        projects_repo.create(&scope, "P2", "p2").await.unwrap();
        assert_eq!(repo.count_projects(&scope).await.unwrap(), 2);

        // Cross-tenant isolation: a sibling tenant's count is independent.
        let t2 = repo.create("count2@example.com", "h").await.unwrap();
        let scope2 = repo.scope_for(t2.id).await.unwrap();
        assert_eq!(repo.count_projects(&scope2).await.unwrap(), 0);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn count_feedback_in_window_respects_tenant_scope_and_window(pool: PgPool) {
        use crate::feedback::{FeedbackRepo, SqlxFeedbackRepo};
        use crate::projects::{ProjectRepo, SqlxProjectRepo};
        use feedbackmonk_core::FeedbackKind;

        let trepo = SqlxTenantRepo::new(pool.clone());
        let prepo = SqlxProjectRepo::new(pool.clone());
        let frepo = SqlxFeedbackRepo::new(pool.clone());

        let t = trepo.create("win@example.com", "h").await.unwrap();
        let scope = trepo.scope_for(t.id).await.unwrap();
        let p = prepo.create(&scope, "P", "p").await.unwrap();
        let pscope = prepo.open(&scope, p.id).await.unwrap();

        // Empty.
        assert_eq!(repo_count_in_window(&trepo, &scope, 30).await, 0);

        // Two submissions.
        frepo
            .submit_anonymous(&pscope, &[1u8; 32], None, "one", FeedbackKind::Other)
            .await
            .unwrap();
        frepo
            .submit_anonymous(&pscope, &[2u8; 32], None, "two", FeedbackKind::Other)
            .await
            .unwrap();
        assert_eq!(repo_count_in_window(&trepo, &scope, 30).await, 2);

        // Window=0 means "now() - 0 days" = now → no rows match (rows
        // were inserted in the past microseconds, but > strictly excludes
        // ties so this is 0 on most pg installations). The assertion is
        // robust either way: 0 OR 2 are both acceptable for window=0
        // because the boundary is non-deterministic at sub-second
        // resolution. Test the more useful case: window=365 captures all.
        assert_eq!(repo_count_in_window(&trepo, &scope, 365).await, 2);

        // Cross-tenant isolation.
        let t2 = trepo.create("win2@example.com", "h").await.unwrap();
        let scope2 = trepo.scope_for(t2.id).await.unwrap();
        assert_eq!(repo_count_in_window(&trepo, &scope2, 30).await, 0);
    }

    async fn repo_count_in_window(
        repo: &SqlxTenantRepo,
        scope: &TenantScope,
        days: i64,
    ) -> i64 {
        repo.count_feedback_in_window(scope, days).await.unwrap()
    }
}
