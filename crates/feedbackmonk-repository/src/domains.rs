//! Host → tenant resolution and the custom-domain registry (FR-FBR-32 / FR-FBR-33).
//!
//! Two audiences with deliberately different disciplines:
//!
//! - **`resolve_host`** — the *pre-authentication* boundary. It takes an
//!   attacker-controlled `Host` header and answers "which tenant, if any, owns
//!   this hostname". It is the ONLY host→tenant lookup in the system, and it is
//!   allow-listed in `multi-tenant-isolation-check` for the same reason
//!   `ProjectRepo::open_for_submission` is: it *mints* a binding from untrusted
//!   input rather than consuming a scope. Everything that follows a resolution
//!   is scope-disciplined again.
//!
//! - **everything else** — ordinary `&TenantScope`-first tenant-facing methods
//!   for managing a tenant's own subdomain and claimed domains.
//!
//! ## What resolution does NOT do
//!
//! Resolving a host grants nothing. A `HostBinding` says "requests on this host
//! belong to tenant T"; it is `feedbackmonk-api::hosting` that turns that into
//! the *restriction* that a request on T's host may not touch another tenant's
//! projects (DEC-FBR-IMPL-28). Reading this module alone would leave the
//! impression that the isolation lives here — it does not, and the
//! `host-tenant-binding` oracle exists because that gap is invisible at runtime:
//! a missing binding renders perfectly correct pages.
//!
//! Lineage: FR-FBR-32/33 · DEC-FBR-13 / DEC-FBR-14 · DEC-FBR-IMPL-27/28/29 ·
//! migration `00030_tenant_hosting.sql` · DEC-FBR-03 (sole query path).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use feedbackmonk_core::hosting::{DomainKind, DomainStatus};

use crate::error::{RepoError, Result};
use crate::scope::TenantScope;

/// How a hostname maps onto the platform.
///
/// `AdminAlias` is intentionally a *separate variant* rather than a flag on
/// `Tenant`: the API must be structurally unable to serve tenant content on an
/// alias, and a bool would let a `match` arm fall through to the tenant path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostBinding {
    /// `{label}.{root_domain}` — the tenant's default public surface.
    TenantSubdomain { tenant_id: Uuid },
    /// A customer-controlled hostname pointed at us by CNAME (FR-FBR-33,
    /// `kind='public'`).
    CustomDomain { tenant_id: Uuid, domain_id: Uuid },
    /// An operator-registered first-party admin hostname (`kind='admin_alias'`).
    /// Answered with a 301 to the canonical admin host — never with the admin
    /// SPA, never with a session cookie (DEC-FBR-IMPL-27).
    AdminAlias { tenant_id: Uuid },
}

impl HostBinding {
    /// The owning tenant, for every variant.
    #[must_use]
    pub fn tenant_id(self) -> Uuid {
        match self {
            Self::TenantSubdomain { tenant_id }
            | Self::CustomDomain { tenant_id, .. }
            | Self::AdminAlias { tenant_id } => tenant_id,
        }
    }

    /// Whether this host may serve tenant-scoped *public* content.
    ///
    /// False for `AdminAlias`, which only ever redirects.
    #[must_use]
    pub fn serves_public_content(self) -> bool {
        matches!(self, Self::TenantSubdomain { .. } | Self::CustomDomain { .. })
    }
}

/// A row in the custom-domain registry, as shown to the owning tenant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantDomain {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub domain: String,
    pub kind: DomainKind,
    pub status: DomainStatus,
    pub created_at: DateTime<Utc>,
    pub verified_at: Option<DateTime<Utc>>,
}

#[async_trait]
pub trait DomainRepo: Send + Sync {
    /// Resolve a **normalised** hostname to its owning tenant, if any.
    ///
    /// `host` MUST already have passed through
    /// `feedbackmonk_core::hosting::normalize_host`; this method does not
    /// re-normalise, because a second, differently-spelled normalisation is
    /// exactly how a comparison guard develops a bypass.
    ///
    /// `root_domain` is the configured wildcard root (e.g. `feedbackmonk.com`).
    /// Pass `None` to disable subdomain resolution entirely — which is what a
    /// self-host deployment does, and why this whole subsystem is inert there.
    ///
    /// Returns `Ok(None)` for an unknown host. That is not an error: the apex,
    /// the marketing site and a stray IP all land here legitimately.
    ///
    /// **Allow-listed** in `.claude/project-oracles/multi-tenant-isolation-check/allowlist.toml`
    /// under rationale "pre-authentication boundary".
    ///
    /// # Errors
    /// Propagates database failures.
    async fn resolve_host(&self, host: &str, root_domain: Option<&str>)
        -> Result<Option<HostBinding>>;

    /// Every domain claimed by the tenant in `scope`.
    ///
    /// # Errors
    /// Propagates database failures.
    async fn list_for_tenant(&self, scope: &TenantScope) -> Result<Vec<TenantDomain>>;

    /// Claim `domain` for the tenant in `scope`.
    ///
    /// `kind` is a parameter rather than hard-coded to `Public` so the operator
    /// path can register an admin alias through the same audited chokepoint;
    /// the tenant-facing handler passes `Public` unconditionally, and the tier
    /// gate lives there (a repository is the wrong place to price a feature).
    ///
    /// # Errors
    /// `RepoError::Conflict` if the domain is already claimed by anyone — the
    /// `UNIQUE(domain)` constraint is what makes host resolution single-valued,
    /// so a collision is a genuine conflict, not an overwrite.
    async fn claim(
        &self,
        scope: &TenantScope,
        domain: &str,
        kind: DomainKind,
    ) -> Result<TenantDomain>;

    /// Release a domain the tenant in `scope` owns.
    ///
    /// # Errors
    /// `RepoError::NotFound` if the id is unknown **or belongs to another
    /// tenant** — the scope predicate is in the SQL, so a cross-tenant delete
    /// is indistinguishable from a missing row and leaks nothing.
    async fn delete(&self, scope: &TenantScope, domain_id: Uuid) -> Result<()>;

    /// Mark a claimed domain `active` the first time traffic is observed on it.
    ///
    /// Scope-bound even though the caller reaches it from a pre-auth path: the
    /// TLS-authorisation handler already resolves the owning tenant, so it can
    /// mint the scope via `TenantRepo::scope_for` and keep this method on the
    /// ordinary scope-disciplined side of the repository. One fewer
    /// `multi-tenant-isolation-check` allow-list entry, and a stray
    /// `domain_id` can no longer flip a row belonging to someone else.
    ///
    /// Idempotent and best-effort: it runs on the request path, so callers
    /// log-and-continue on error rather than failing the request.
    ///
    /// # Errors
    /// Propagates database failures.
    async fn mark_active(&self, scope: &TenantScope, domain_id: Uuid) -> Result<()>;
}

#[derive(Clone)]
pub struct SqlxDomainRepo {
    pool: PgPool,
}

impl SqlxDomainRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl DomainRepo for SqlxDomainRepo {
    async fn resolve_host(
        &self,
        host: &str,
        root_domain: Option<&str>,
    ) -> Result<Option<HostBinding>> {
        // A custom domain wins over a subdomain interpretation. It cannot
        // actually be both (a claimed domain under our own root would have to
        // pass the reserved-label and uniqueness checks first), but ordering the
        // lookups makes that non-ambiguity explicit rather than incidental.
        let row = sqlx::query!(
            r#"
            SELECT id, tenant_id, kind, status
            FROM tenant_domains
            WHERE domain = $1
            "#,
            host
        )
        .fetch_optional(&self.pool)
        .await?;

        if let Some(r) = row {
            return Ok(Some(match DomainKind::from_db_str(&r.kind)? {
                DomainKind::AdminAlias => HostBinding::AdminAlias {
                    tenant_id: r.tenant_id,
                },
                DomainKind::Public => HostBinding::CustomDomain {
                    tenant_id: r.tenant_id,
                    domain_id: r.id,
                },
            }));
        }

        // Subdomain resolution is only attempted when a root domain is
        // configured. `None` here is the self-host posture: no root, no
        // resolution, no behaviour change (DEC-FBR-IMPL-28 part 3).
        let Some(root) = root_domain else {
            return Ok(None);
        };
        let Some(label) = feedbackmonk_core::hosting::subdomain_label_of(host, root) else {
            return Ok(None);
        };

        let tenant = sqlx::query!("SELECT id FROM tenants WHERE subdomain = $1", label)
            .fetch_optional(&self.pool)
            .await?;

        Ok(tenant.map(|t| HostBinding::TenantSubdomain { tenant_id: t.id }))
    }

    async fn list_for_tenant(&self, scope: &TenantScope) -> Result<Vec<TenantDomain>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, tenant_id, domain, kind, status, created_at, verified_at
            FROM tenant_domains
            WHERE tenant_id = $1
            ORDER BY created_at ASC
            "#,
            scope.tenant_id()
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|r| {
                Ok(TenantDomain {
                    id: r.id,
                    tenant_id: r.tenant_id,
                    domain: r.domain,
                    kind: DomainKind::from_db_str(&r.kind)?,
                    status: DomainStatus::from_db_str(&r.status)?,
                    created_at: r.created_at,
                    verified_at: r.verified_at,
                })
            })
            .collect()
    }

    async fn claim(
        &self,
        scope: &TenantScope,
        domain: &str,
        kind: DomainKind,
    ) -> Result<TenantDomain> {
        let row = sqlx::query!(
            r#"
            INSERT INTO tenant_domains (tenant_id, domain, kind)
            VALUES ($1, $2, $3)
            RETURNING id, tenant_id, domain, kind, status, created_at, verified_at
            "#,
            scope.tenant_id(),
            domain,
            kind.as_db_str()
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.is_unique_violation() => RepoError::Conflict,
            other => RepoError::Sqlx(other),
        })?;

        Ok(TenantDomain {
            id: row.id,
            tenant_id: row.tenant_id,
            domain: row.domain,
            kind: DomainKind::from_db_str(&row.kind)?,
            status: DomainStatus::from_db_str(&row.status)?,
            created_at: row.created_at,
            verified_at: row.verified_at,
        })
    }

    async fn delete(&self, scope: &TenantScope, domain_id: Uuid) -> Result<()> {
        // tenant_id is in the WHERE clause, not checked afterwards: a
        // cross-tenant delete matches zero rows and is reported as NotFound,
        // which is the same answer an unknown id gets (no existence oracle).
        let result = sqlx::query!(
            "DELETE FROM tenant_domains WHERE id = $1 AND tenant_id = $2",
            domain_id,
            scope.tenant_id()
        )
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(RepoError::NotFound);
        }
        Ok(())
    }

    async fn mark_active(&self, scope: &TenantScope, domain_id: Uuid) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE tenant_domains
            SET status = 'active', verified_at = COALESCE(verified_at, now())
            WHERE id = $1 AND tenant_id = $2 AND status <> 'active'
            "#,
            domain_id,
            scope.tenant_id()
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
