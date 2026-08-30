//! Tenant-facing hosting settings — subdomain + custom domains (FR-FBR-32/33).
//!
//! ```text
//! domains_router(state):
//!   GET    /api/v1/admin/hosting              -- subdomain + claimed domains + CNAME target
//!   PUT    /api/v1/admin/hosting/subdomain    -- set/clear the tenant subdomain
//!   POST   /api/v1/admin/hosting/domains      -- claim a custom domain (TIER-GATED)
//!   DELETE /api/v1/admin/hosting/domains/{id} -- release a claimed domain
//! ```
//!
//! Every route is behind `AdminSession`. Merged WITHOUT `.layer(cors)` — this is
//! an admin surface, never a browser embed — and wrapped in `bind_admin_routes`
//! so it is unreachable on a tenant or custom host (DEC-FBR-13: admin lives on
//! exactly one origin).
//!
//! ## Where the paid feature is actually enforced
//!
//! `POST …/domains` consults `tier_quotas(tier).custom_domain` **before** the
//! first write and returns **402** when the tier does not carry it. That is the
//! claim-time gate. The issuance-time gate lives in
//! `public_site::tls_authorize` — a tenant who downgrades keeps their row but
//! stops getting certificates. Two gates, because a claim check alone would let
//! a tenant claim on Pro, downgrade, and keep serving forever.
//!
//! The marketing site has advertised "Custom domain" since P3 with nothing
//! behind it (DEC-FBR-08 listed it OUT, "wire up post-launch"). This module is
//! what makes the pricing card true.
//!
//! ## What a tenant may NOT claim
//!
//! Any host the platform owns — the admin host, the apex, or anything under the
//! wildcard root. Without that refusal a tenant could claim
//! `app.feedbackmonk.com` and have host resolution hand their content the admin
//! origin, which is precisely the property DEC-FBR-13 buys.
//!
//! Lineage: FR-FBR-32/33 · DEC-FBR-13 · DEC-FBR-IMPL-27/29 · Contract C33 ·
//! migration `00030_tenant_hosting.sql`.

use std::sync::Arc;

use axum::extract::{FromRef, Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use feedbackmonk_core::hosting::{
    normalize_host, validate_subdomain_label, DomainKind, DomainStatus, SubdomainError,
};
use feedbackmonk_core::{tier_quotas, ResourceKind, Tier};
use feedbackmonk_repository::{DomainRepo, RepoError, TenantDomain};

use crate::auth::session::AdminSession;
use crate::error::ApiError;
use crate::hosting::HostConfig;
use crate::state::AppState;

/// Sub-state for the hosting admin surface.
///
/// Carries `AppState` so `AdminSession` (which needs the session secret and the
/// tenant repo) still extracts, plus the domain repo and hosting config. The
/// `FromRef` impl below is what lets the session extractor reach through.
#[derive(Clone)]
pub struct DomainAdminState {
    pub app: AppState,
    pub domains: Arc<dyn DomainRepo>,
    pub config: HostConfig,
}

impl FromRef<DomainAdminState> for AppState {
    fn from_ref(st: &DomainAdminState) -> Self {
        st.app.clone()
    }
}

pub fn domains_router(state: DomainAdminState) -> Router {
    Router::new()
        .route("/api/v1/admin/hosting", get(get_hosting))
        .route("/api/v1/admin/hosting/subdomain", axum::routing::put(put_subdomain))
        .route("/api/v1/admin/hosting/domains", post(claim_domain))
        .route(
            "/api/v1/admin/hosting/domains/:domain_id",
            axum::routing::delete(release_domain),
        )
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Wire shapes (Contract C33)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct DomainResponse {
    pub id: Uuid,
    pub domain: String,
    pub status: &'static str,
    pub created_at: DateTime<Utc>,
    pub verified_at: Option<DateTime<Utc>>,
}

impl From<TenantDomain> for DomainResponse {
    fn from(d: TenantDomain) -> Self {
        Self {
            id: d.id,
            domain: d.domain,
            status: d.status.as_db_str(),
            created_at: d.created_at,
            verified_at: d.verified_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct HostingResponse {
    /// Current pricing tier. Present so the UI can render the shared
    /// `UpgradePrompt` (which keys its copy off the tier) without a second
    /// round-trip to `/admin/tier`.
    pub tier: Tier,
    pub subdomain: Option<String>,
    /// The tenant's canonical public host, once they have a subdomain and the
    /// deployment has a root domain configured.
    pub public_host: Option<String>,
    /// Exactly what to put on the right-hand side of the customer's CNAME
    /// record. `None` when the deployment offers no subdomains, in which case
    /// custom domains are not offered either.
    pub cname_target: Option<String>,
    /// Whether this tenant's tier includes custom domains. The UI uses it to
    /// show an upgrade prompt instead of a form; the server enforces
    /// independently on claim and on certificate issuance.
    pub custom_domain_available: bool,
    pub domains: Vec<DomainResponse>,
}

#[derive(Debug, Deserialize)]
pub struct SubdomainRequest {
    /// `None` clears the subdomain (the tenant's public host goes away).
    pub subdomain: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ClaimDomainRequest {
    pub domain: String,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn get_hosting(
    State(st): State<DomainAdminState>,
    session: AdminSession,
) -> Result<Json<HostingResponse>, ApiError> {
    let subdomain = st.app.tenants.get_subdomain(&session.scope).await?;
    let domains = st.domains.list_for_tenant(&session.scope).await?;
    let tier = st.app.tenants.get_tier(&session.scope).await?;

    let public_host = match (&subdomain, &st.config.root_domain) {
        (Some(label), Some(root)) => Some(format!("{label}.{root}")),
        _ => None,
    };

    Ok(Json(HostingResponse {
        tier,
        subdomain,
        cname_target: public_host.clone(),
        public_host,
        custom_domain_available: tier_quotas(tier).custom_domain,
        domains: domains
            .into_iter()
            // Admin aliases are operator-managed infrastructure, not a tenant
            // setting. Showing them here would invite a tenant to delete the
            // record that keeps a first-party link alive.
            .filter(|d| d.kind == DomainKind::Public)
            .map(DomainResponse::from)
            .collect(),
    }))
}

async fn put_subdomain(
    State(st): State<DomainAdminState>,
    session: AdminSession,
    Json(req): Json<SubdomainRequest>,
) -> Result<Json<HostingResponse>, ApiError> {
    let normalised = match req.subdomain {
        None => None,
        Some(raw) => {
            let label = raw.trim().to_ascii_lowercase();
            if label.is_empty() {
                None
            } else {
                validate_subdomain_label(&label).map_err(|e| match e {
                    // Reserved is well-formed-but-unavailable — the same shape
                    // as "already taken", hence 409 rather than 400.
                    SubdomainError::Reserved => ApiError::Conflict(e.as_message().to_string()),
                    other => ApiError::BadRequest(other.as_message().to_string()),
                })?;
                Some(label)
            }
        }
    };

    st.app
        .tenants
        .set_subdomain(&session.scope, normalised.as_deref())
        .await
        .map_err(|e| match e {
            RepoError::Conflict => {
                ApiError::Conflict("that subdomain is already taken".to_string())
            }
            other => ApiError::from(other),
        })?;

    get_hosting(State(st), session).await
}

async fn claim_domain(
    State(st): State<DomainAdminState>,
    session: AdminSession,
    Json(req): Json<ClaimDomainRequest>,
) -> Result<(StatusCode, Json<DomainResponse>), ApiError> {
    // --- tier gate (FR-FBR-33), BEFORE the first write ---------------------
    let tier = st.app.tenants.get_tier(&session.scope).await?;
    let quotas = tier_quotas(tier);
    if !quotas.custom_domain {
        return Err(ApiError::TierCapExceeded {
            tier,
            resource: ResourceKind::Project,
            current: 0,
            limit: 0,
            upgrade_hint: "Custom domains are available on the Pro tier and above."
                .to_string(),
        });
    }

    let Some(domain) = normalize_host(&req.domain) else {
        return Err(ApiError::BadRequest("domain is not a valid hostname".into()));
    };
    // A bare label cannot be CNAMEd and would collide with subdomain resolution.
    if !domain.contains('.') || domain.starts_with('[') {
        return Err(ApiError::BadRequest(
            "domain must be a fully-qualified hostname, e.g. feedback.example.com".into(),
        ));
    }
    if st.config.is_platform_host(&domain) {
        return Err(ApiError::BadRequest(
            "that hostname belongs to the platform and cannot be claimed".into(),
        ));
    }

    let claimed = st
        .domains
        .claim(&session.scope, &domain, DomainKind::Public)
        .await
        .map_err(|e| match e {
            // Deliberately does not say by whom: a claim collision must not
            // report another tenant's holdings.
            RepoError::Conflict => {
                ApiError::Conflict("that domain is already registered".to_string())
            }
            other => ApiError::from(other),
        })?;

    debug_assert_eq!(claimed.status, DomainStatus::Pending);
    Ok((StatusCode::CREATED, Json(DomainResponse::from(claimed))))
}

async fn release_domain(
    State(st): State<DomainAdminState>,
    session: AdminSession,
    Path(domain_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    // The repo scopes the DELETE by tenant_id, so another tenant's id is
    // reported as NotFound — no existence oracle.
    st.domains.delete(&session.scope, domain_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
