#![allow(clippy::unused_async)] // axum handlers must be async even when the body has no .await branch

//! Host-rooted public discovery + the edge's TLS authorisation seam (FR-FBR-32/33).
//!
//! ```text
//! public_site_router(state):
//!   GET /api/v1/public/site                    -- who lives on this host?
//!   GET /api/v1/public/tls-authorize?domain=   -- may the edge issue a cert for this host?
//! ```
//!
//! ## Why a discovery endpoint instead of host-rooted route copies
//!
//! `{tenant}.feedbackmonk.com` needs to serve a board without a `project_id` in
//! the URL. The cheap-looking answer — a parallel family of host-rooted routes
//! (`/api/v1/board`, `/api/v1/roadmap`, …) — would fork every public handler and
//! double the surface the moderation, privacy and rate-limit guards must cover.
//! Instead the SPA asks *once* which tenant and projects this host serves, then
//! uses the existing, already-hardened `project_id` routes. One new endpoint,
//! zero forked handlers.
//!
//! **Privacy**: the payload is project metadata only — id, slug, display name,
//! board-enabled. No tenant email, no counts, no feedback, nothing about other
//! tenants. Same posture as `widget-config` (Contract C12), which is likewise
//! public and likewise carries brand metadata and nothing more.
//!
//! ## The TLS seam (DEC-FBR-IMPL-29)
//!
//! `tls-authorize` is the `on_demand_tls.ask` endpoint of the edge proxy. The
//! edge asks before requesting a certificate for an unfamiliar SNI; a 404 means
//! no issuance is attempted. Two things hang off it:
//!
//! - **The tier gate becomes infrastructural.** A domain whose owning tenant no
//!   longer carries `tier_quotas().custom_domain` stops getting a certificate.
//!   FR-FBR-33 is then enforced by the thing that actually serves traffic, not
//!   only by a check in a claim handler that already ran.
//! - **Unbounded-issuance abuse becomes structurally impossible.** On-demand TLS
//!   with no `ask` endpoint lets anyone who points DNS at you burn your ACME
//!   rate limit.
//!
//! The response body is empty by design: the status IS the answer, and a body
//! would be an oracle for which domains exist.
//!
//! Lineage: FR-FBR-32/33 · DEC-FBR-13 · DEC-FBR-IMPL-29 · Contract C32/C33.

use std::sync::Arc;

use axum::extract::{Extension, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use feedbackmonk_core::{hosting::normalize_host, tier_quotas};
use feedbackmonk_core::hosting::DomainKind;
use feedbackmonk_repository::{DomainRepo, ProjectRepo, TenantRepo};

use crate::error::ApiError;
use crate::hosting::{HostConfig, HostScope};

/// State for the public host-rooted surface. Its own type (not `AppState`) so
/// the domain repo does not have to thread through every `AppState { … }`
/// literal — the `AttachmentState` / `MeFeedbackDataState` precedent.
#[derive(Clone)]
pub struct PublicSiteState {
    pub tenants: Arc<dyn TenantRepo>,
    pub projects: Arc<dyn ProjectRepo>,
    pub domains: Arc<dyn DomainRepo>,
    pub config: HostConfig,
}

pub fn public_site_router(state: PublicSiteState) -> Router {
    Router::new()
        .route("/api/v1/public/site", get(get_site))
        .route("/api/v1/public/tls-authorize", get(tls_authorize))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Wire shapes (Contract C32)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct SiteProject {
    pub project_id: Uuid,
    pub slug: String,
    pub name: String,
    pub public_board_enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SiteResponse {
    pub tenant_id: Uuid,
    /// The tenant's own subdomain label, when they have one. Lets the SPA build
    /// canonical links even when the visitor arrived on a custom domain.
    pub subdomain: Option<String>,
    /// True when this request arrived on the tenant's own (custom) domain rather
    /// than their `{subdomain}.{root}` host.
    pub custom_domain: bool,
    pub projects: Vec<SiteProject>,
}

// ---------------------------------------------------------------------------
// GET /api/v1/public/site
// ---------------------------------------------------------------------------

/// Describe the tenant this host belongs to.
///
/// 404 on any host that is not tenant-bound — the apex, the admin host, an
/// unknown name, or any host at all when host-awareness is switched off. A
/// caller therefore cannot use this endpoint to enumerate tenants: it only ever
/// reports the one whose host they already reached.
async fn get_site(
    State(st): State<PublicSiteState>,
    Extension(scope): Extension<HostScope>,
) -> Result<Json<SiteResponse>, ApiError> {
    let HostScope::Tenant {
        tenant_id,
        domain_id,
    } = scope
    else {
        return Err(ApiError::NotFound);
    };

    // The host binding is the authenticated fact here: the request reached a
    // hostname this tenant owns. `scope_for` is the allow-listed bridge from a
    // verified tenant id to a `TenantScope`.
    let tenant_scope = st.tenants.scope_for(tenant_id).await?;
    let subdomain = st.tenants.get_subdomain(&tenant_scope).await?;
    let projects = st.projects.list_for_tenant(&tenant_scope).await?;

    let mut out = Vec::with_capacity(projects.len());
    for p in projects {
        let project_scope = st.projects.open(&tenant_scope, p.id).await?;
        let settings = st.projects.get_board_settings(&project_scope).await?;
        out.push(SiteProject {
            project_id: p.id,
            slug: p.slug,
            name: p.name,
            public_board_enabled: settings.public_board_enabled,
        });
    }

    Ok(Json(SiteResponse {
        tenant_id,
        subdomain,
        custom_domain: domain_id.is_some(),
        projects: out,
    }))
}

// ---------------------------------------------------------------------------
// GET /api/v1/public/tls-authorize
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct TlsAuthorizeQuery {
    pub domain: String,
}

/// The edge proxy's `on_demand_tls.ask` seam (DEC-FBR-IMPL-29).
///
/// 200 ⇒ issue a certificate for this host. 404 ⇒ do not. Empty body either way.
///
/// Authorised hosts: the canonical admin host, any host under the configured
/// wildcard root that resolves to a tenant, an operator-registered admin alias,
/// and a claimed **public** custom domain whose owning tenant still carries the
/// `custom_domain` tier capability.
async fn tls_authorize(
    State(st): State<PublicSiteState>,
    Query(q): Query<TlsAuthorizeQuery>,
) -> StatusCode {
    let Some(host) = normalize_host(&q.domain) else {
        return StatusCode::NOT_FOUND;
    };
    if !st.config.is_enabled() {
        // No hosting configuration means no managed certificates. Refusing is
        // the safe answer: a misconfigured edge should issue nothing rather
        // than everything.
        return StatusCode::NOT_FOUND;
    }
    if st.config.admin_host.as_deref() == Some(host.as_str()) {
        return StatusCode::OK;
    }

    match st
        .domains
        .resolve_host(&host, st.config.root_domain.as_deref())
        .await
    {
        Ok(Some(binding)) => {
            // A tenant subdomain or an admin alias needs no tier check — those
            // hosts are ours, and a wildcard certificate usually covers the
            // former anyway.
            let needs_tier_check = matches!(
                binding,
                feedbackmonk_repository::HostBinding::CustomDomain { .. }
            );
            if !needs_tier_check {
                return StatusCode::OK;
            }
            match tenant_may_hold_custom_domain(&st, binding.tenant_id()).await {
                Ok(true) => {
                    // The edge only asks after a real TLS handshake arrived for
                    // this SNI, which means DNS already points here — the
                    // strongest activation signal available without a resolver.
                    if let feedbackmonk_repository::HostBinding::CustomDomain {
                        tenant_id,
                        domain_id,
                    } = binding
                    {
                        match st.tenants.scope_for(tenant_id).await {
                            Ok(scope) => {
                                if let Err(e) = st.domains.mark_active(&scope, domain_id).await {
                                    tracing::warn!(error = %e, "failed to mark custom domain active");
                                }
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "failed to scope tenant for activation");
                            }
                        }
                    }
                    StatusCode::OK
                }
                Ok(false) => {
                    tracing::info!(
                        domain = %host,
                        "TLS issuance refused: tenant tier no longer carries custom_domain (FR-FBR-33)"
                    );
                    StatusCode::NOT_FOUND
                }
                Err(e) => {
                    tracing::error!(error = %e, "tls-authorize tier lookup failed");
                    StatusCode::NOT_FOUND
                }
            }
        }
        Ok(None) => StatusCode::NOT_FOUND,
        Err(e) => {
            tracing::error!(error = %e, "tls-authorize host resolution failed");
            StatusCode::NOT_FOUND
        }
    }
}

/// Does this tenant's current tier include the custom-domain capability?
///
/// Shared by `tls_authorize` and the claim handler so the paid feature has one
/// definition, read from `tier_quotas()` (Contract C19) rather than restated.
///
/// # Errors
/// Propagates repository failures.
pub async fn tenant_may_hold_custom_domain(
    st: &PublicSiteState,
    tenant_id: Uuid,
) -> Result<bool, ApiError> {
    let scope = st.tenants.scope_for(tenant_id).await?;
    let tier = st.tenants.get_tier(&scope).await?;
    Ok(tier_quotas(tier).custom_domain)
}

/// Kinds a tenant may claim for themselves.
///
/// `admin_alias` is operator-only (DEC-FBR-IMPL-27): it exists to keep
/// first-party admin links working across the DEC-FBR-14 migration, is not
/// sold, and must not be reachable from a tenant-facing handler.
#[must_use]
pub const fn tenant_claimable_kind() -> DomainKind {
    DomainKind::Public
}
