//! Host → tenant resolution and **binding** for the public surface (FR-FBR-32).
//!
//! ## The distinction this whole module exists for
//!
//! *Resolution* answers "which tenant owns this hostname". *Binding* is the
//! restriction that follows: on a host that resolves to tenant T, no public
//! route may touch another tenant's data. DEC-FBR-13 chose subdomains over
//! paths for exactly one decisive reason — origin isolation for the
//! user-generated-content surface — and **resolution alone does not deliver
//! it**. Without binding, `a.feedbackmonk.com/api/v1/projects/{B's id}/board`
//! still renders tenant B's user-submitted text on tenant A's origin. Every
//! page looks right. Nothing errors. The isolation is fictional.
//!
//! That is why the guard is a *layer over the existing routers* rather than a
//! new route family: it cannot be forgotten per-handler, and the
//! `host-tenant-binding` Verification Oracle asserts from `build_app` that each
//! public router still carries it — the same anti-treadmill shape
//! `public-route-ceiling` uses for the rate-limit floor.
//!
//! ## Inert when unconfigured
//!
//! With neither `FEEDBACKMONK_ROOT_DOMAIN` nor `FEEDBACKMONK_ADMIN_HOST` set,
//! [`HostConfig::is_enabled`] is false, [`resolve`] short-circuits to
//! [`HostScope::Unbound`], and every guard is a pass-through. This is not a
//! convenience: FR-FBR-17 self-host runs one tenant on an arbitrary hostname
//! (often a bare IP), and a subdomain scheme it never opted into must not
//! change a single response there.
//!
//! Lineage: FR-FBR-32/33 · DEC-FBR-13 · DEC-FBR-IMPL-27 (admin alias 301) /
//! -28 (binding, additive, inert) · migration `00030_tenant_hosting.sql`.

use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, HeaderValue, Request, StatusCode, Uri};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use serde_json::json;
use uuid::Uuid;

use feedbackmonk_core::hosting::{normalize_host, subdomain_label_of};
use feedbackmonk_repository::{DomainRepo, HostBinding, ProjectRepo};

/// Hosting configuration read once at startup.
///
/// Both fields are optional and independently so: an operator may pin the admin
/// host without offering tenant subdomains (locking admin to one origin is
/// useful on its own), or configure a root domain while still serving admin
/// from it.
#[derive(Clone, Debug, Default)]
pub struct HostConfig {
    /// Wildcard root for tenant subdomains, e.g. `feedbackmonk.com`. Normalised.
    pub root_domain: Option<String>,
    /// The single host that serves the admin console, e.g. `app.feedbackmonk.com`.
    /// Normalised. DEC-FBR-13: admin lives here and nowhere else.
    pub admin_host: Option<String>,
    /// Whether `X-Forwarded-Host` may override the `Host` header.
    ///
    /// Deliberately derived from the EXISTING `FEEDBACKMONK_TRUSTED_PROXY_HOPS`
    /// switch rather than given its own knob: an operator who has already
    /// declared "there is a trusted proxy in front of me" has made exactly the
    /// statement this needs, and a second, independently-settable trust flag is
    /// a way for the two to disagree.
    pub trust_forwarded_host: bool,
}

impl HostConfig {
    /// Read from the environment. Unset/blank values stay `None`.
    #[must_use]
    pub fn from_env(trusted_proxy_hops: usize) -> Self {
        let read = |key: &str| {
            std::env::var(key)
                .ok()
                .and_then(|v| normalize_host(&v))
                .filter(|v| !v.is_empty())
        };
        Self {
            root_domain: read("FEEDBACKMONK_ROOT_DOMAIN"),
            admin_host: read("FEEDBACKMONK_ADMIN_HOST"),
            trust_forwarded_host: trusted_proxy_hops > 0,
        }
    }

    /// True when any host-aware behaviour should apply at all.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.root_domain.is_some() || self.admin_host.is_some()
    }

    /// Whether `host` is a name the platform owns (the admin host, the apex, or
    /// anything under the wildcard root).
    ///
    /// Used to refuse a custom-domain claim for a name we control — otherwise a
    /// tenant could claim `app.feedbackmonk.com` and have the resolver hand
    /// their content the admin origin.
    #[must_use]
    pub fn is_platform_host(&self, host: &str) -> bool {
        if self.admin_host.as_deref() == Some(host) {
            return true;
        }
        match self.root_domain.as_deref() {
            Some(root) => host == root || host.ends_with(&format!(".{root}")),
            None => false,
        }
    }
}

/// What the request's hostname means, attached to every request the guards see.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostScope {
    /// Host-awareness is off, or the host carries no tenant meaning. Behaves
    /// exactly like the pre-FR-FBR-32 world.
    Unbound,
    /// The canonical admin host.
    Admin,
    /// A tenant's public surface — their subdomain, or a custom domain they own.
    Tenant {
        tenant_id: Uuid,
        /// `Some` for a custom domain (FR-FBR-33), `None` for the subdomain.
        domain_id: Option<Uuid>,
    },
    /// An operator-registered admin alias: 301 to the admin host, nothing else
    /// (DEC-FBR-IMPL-27).
    AdminAlias,
}

impl HostScope {
    /// The bound tenant, when the host names one.
    #[must_use]
    pub fn tenant_id(self) -> Option<Uuid> {
        match self {
            Self::Tenant { tenant_id, .. } => Some(tenant_id),
            _ => None,
        }
    }
}

/// Everything the host layer needs, as its own state type.
///
/// A sub-state rather than new `AppState` fields, following the established
/// `AttachmentState` / `MeFeedbackDataState` / `AccountRecoveryState` pattern —
/// two new required fields would otherwise have to be threaded through every
/// `AppState { … }` literal in the workspace.
#[derive(Clone)]
pub struct HostState {
    pub domains: Arc<dyn DomainRepo>,
    pub projects: Arc<dyn ProjectRepo>,
    pub config: HostConfig,
}

/// Extract the effective hostname from request headers.
///
/// `X-Forwarded-Host` is consulted **only** when a trusted proxy is declared;
/// otherwise it is an attacker-settable header and honouring it would make the
/// whole binding scheme bypassable with one curl flag.
#[must_use]
pub fn effective_host(headers: &axum::http::HeaderMap, cfg: &HostConfig) -> Option<String> {
    if cfg.trust_forwarded_host {
        if let Some(fwd) = headers.get("x-forwarded-host").and_then(|v| v.to_str().ok()) {
            // A proxy chain may append; the FIRST entry is the original client-
            // facing host.
            let first = fwd.split(',').next().unwrap_or(fwd);
            if let Some(h) = normalize_host(first) {
                return Some(h);
            }
        }
    }
    headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .and_then(normalize_host)
}

/// Resolve a hostname to its [`HostScope`].
///
/// Resolution order is load-bearing:
/// 1. **admin host first** — structurally, so that even a (rejected) custom
///    domain row spelling the admin host could never hand a tenant that origin;
/// 2. the domain registry (custom domains + admin aliases);
/// 3. the wildcard root's single-level label.
///
/// # Errors
/// Propagates repository failures. Callers treat an error as "unresolved"
/// rather than failing the request open.
pub async fn resolve(
    domains: &dyn DomainRepo,
    cfg: &HostConfig,
    host: &str,
) -> Result<HostScope, feedbackmonk_repository::RepoError> {
    if !cfg.is_enabled() {
        return Ok(HostScope::Unbound);
    }
    if cfg.admin_host.as_deref() == Some(host) {
        return Ok(HostScope::Admin);
    }
    // Skip the DB entirely for a host that can carry no tenant meaning: the
    // apex, and anything under the root that is not a single-level label.
    let root = cfg.root_domain.as_deref();
    let could_be_subdomain = root.is_some_and(|r| subdomain_label_of(host, r).is_some());
    let could_be_custom = root.is_none_or(|r| host != r && !host.ends_with(&format!(".{r}")));
    if !could_be_subdomain && !could_be_custom {
        return Ok(HostScope::Unbound);
    }

    match domains.resolve_host(host, root).await? {
        Some(HostBinding::TenantSubdomain { tenant_id }) => Ok(HostScope::Tenant {
            tenant_id,
            domain_id: None,
        }),
        Some(HostBinding::CustomDomain {
            tenant_id,
            domain_id,
        }) => Ok(HostScope::Tenant {
            tenant_id,
            domain_id: Some(domain_id),
        }),
        Some(HostBinding::AdminAlias { .. }) => Ok(HostScope::AdminAlias),
        None => Ok(HostScope::Unbound),
    }
}

/// Pull the `{project_id}` out of an `/api/v1/projects/{project_id}/…` path.
///
/// A direct path scan rather than an axum path extractor because this runs as a
/// `Router::layer`, ahead of any per-route extraction, and because a scan is
/// what the oracle can verify statically. A segment that is not a UUID yields
/// `None` and the request proceeds — the handler's own extractor will reject it.
#[must_use]
pub fn project_id_in_path(path: &str) -> Option<Uuid> {
    let mut segments = path.split('/').filter(|s| !s.is_empty());
    while let Some(seg) = segments.next() {
        if seg == "projects" {
            return segments.next().and_then(|s| Uuid::parse_str(s).ok());
        }
    }
    None
}

/// Wrap a **public** router with host resolution + tenant binding.
///
/// This is the single marker the `host-tenant-binding` oracle greps for. Every
/// public router in `build_app` must be passed through it; adding a public route
/// without it is the regression the oracle exists to catch.
///
/// Behaviour, in order:
/// 1. resolve the host and stash [`HostScope`] as a request extension so
///    handlers (notably `public_site`) can read it;
/// 2. an admin alias 301s to the canonical admin host, carrying path and query;
/// 3. on a tenant-bound host, a `project_id` in the path that belongs to a
///    **different** tenant yields **404** — not 403, so a cross-tenant probe
///    cannot learn whether the project exists (same posture as
///    `public-board-moderation-gate`).
pub fn bind_public_routes(router: Router, state: HostState) -> Router {
    router.layer(from_fn_with_state(state, public_host_binding))
}

/// Wrap an **admin** router so it is served on the canonical admin host only.
///
/// The rule is "deny on a tenant-bound host", not "allow only on the admin
/// host". That distinction keeps IP-addressed health probes, `docker compose`
/// self-host and local development working untouched, while still guaranteeing
/// the property DEC-FBR-13 asks for: a tenant subdomain or a customer's own
/// domain can never reach admin, and no admin session cookie is ever issued on
/// one — because the request 404s before the handler runs.
pub fn bind_admin_routes(router: Router, state: HostState) -> Router {
    router.layer(from_fn_with_state(state, admin_host_binding))
}

async fn public_host_binding(
    State(st): State<HostState>,
    mut req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let scope = resolve_for_request(&st, req.headers()).await;
    req.extensions_mut().insert(scope);

    if scope == HostScope::AdminAlias {
        return redirect_to_admin(&st.config, req.uri());
    }

    if let Some(tenant_id) = scope.tenant_id() {
        if let Some(project_id) = project_id_in_path(req.uri().path()) {
            // `open_for_submission` is the existing pre-auth project→tenant
            // resolver; reusing it keeps the number of ways to learn a
            // project's owner at one.
            match st.projects.open_for_submission(project_id).await {
                Ok(project_scope) if project_scope.tenant_id() == tenant_id => {}
                Ok(_) => {
                    tracing::warn!(
                        %project_id,
                        bound_tenant = %tenant_id,
                        "cross-tenant request refused by host binding (FR-FBR-32)"
                    );
                    return not_found();
                }
                // Unknown project: 404 here rather than letting the handler
                // answer, so the two paths are indistinguishable from outside.
                Err(_) => return not_found(),
            }
        }
    }

    next.run(req).await
}

async fn admin_host_binding(
    State(st): State<HostState>,
    mut req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let scope = resolve_for_request(&st, req.headers()).await;
    req.extensions_mut().insert(scope);

    match scope {
        // A tenant surface must never serve admin, and an alias only redirects.
        HostScope::Tenant { .. } => not_found(),
        HostScope::AdminAlias => redirect_to_admin(&st.config, req.uri()),
        HostScope::Admin | HostScope::Unbound => next.run(req).await,
    }
}

// Takes `&HeaderMap` rather than `&Request`: `axum::body::Body` is `Send` but
// NOT `Sync`, so holding a `&Request<Body>` across an `.await` would make this
// future `!Send` and the middleware unusable as a tower `Service`.
async fn resolve_for_request(st: &HostState, headers: &axum::http::HeaderMap) -> HostScope {
    if !st.config.is_enabled() {
        return HostScope::Unbound;
    }
    let Some(host) = effective_host(headers, &st.config) else {
        return HostScope::Unbound;
    };
    match resolve(st.domains.as_ref(), &st.config, &host).await {
        Ok(scope) => scope,
        Err(e) => {
            // Fail CLOSED for binding purposes: an unresolvable host is
            // `Unbound`, which for the ADMIN guard means "allowed" — but an
            // admin alias or tenant host that failed to resolve would have been
            // denied anyway, and treating a transient DB error as a tenant
            // binding would 404 legitimate traffic. Logged so it is visible.
            tracing::error!(error = %e, "host resolution failed; treating host as unbound");
            HostScope::Unbound
        }
    }
}

fn redirect_to_admin(cfg: &HostConfig, uri: &Uri) -> Response {
    let Some(admin_host) = cfg.admin_host.as_deref() else {
        // An alias with no admin host configured has nowhere to go. 404 rather
        // than redirect to a guess.
        return not_found();
    };
    let path_and_query = uri
        .path_and_query()
        .map_or_else(|| "/".to_string(), ToString::to_string);
    let location = format!("https://{admin_host}{path_and_query}");
    match HeaderValue::from_str(&location) {
        Ok(value) => (
            StatusCode::MOVED_PERMANENTLY,
            [(header::LOCATION, value)],
        )
            .into_response(),
        Err(_) => not_found(),
    }
}

fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "not found" })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;

    fn cfg() -> HostConfig {
        HostConfig {
            root_domain: Some("feedbackmonk.com".into()),
            admin_host: Some("app.feedbackmonk.com".into()),
            trust_forwarded_host: false,
        }
    }

    #[test]
    fn disabled_config_is_not_enabled() {
        assert!(!HostConfig::default().is_enabled());
        assert!(cfg().is_enabled());
    }

    #[test]
    fn platform_hosts_are_recognised() {
        let c = cfg();
        assert!(c.is_platform_host("app.feedbackmonk.com"));
        assert!(c.is_platform_host("feedbackmonk.com"));
        assert!(c.is_platform_host("acme.feedbackmonk.com"));
        assert!(!c.is_platform_host("feedback.gitcellar.com"));
        // The near-miss a naive `ends_with` would wrongly claim.
        assert!(!c.is_platform_host("notfeedbackmonk.com"));
    }

    #[test]
    fn forwarded_host_ignored_without_a_trusted_proxy() {
        let mut h = HeaderMap::new();
        h.insert(header::HOST, HeaderValue::from_static("acme.feedbackmonk.com"));
        h.insert(
            "x-forwarded-host",
            HeaderValue::from_static("evil.feedbackmonk.com"),
        );
        // Without a declared trusted proxy the spoofable header must not win.
        assert_eq!(
            effective_host(&h, &cfg()).as_deref(),
            Some("acme.feedbackmonk.com")
        );
    }

    #[test]
    fn forwarded_host_honoured_behind_a_trusted_proxy() {
        let mut h = HeaderMap::new();
        h.insert(header::HOST, HeaderValue::from_static("internal.railway"));
        h.insert(
            "x-forwarded-host",
            HeaderValue::from_static("acme.feedbackmonk.com, proxy.internal"),
        );
        let mut c = cfg();
        c.trust_forwarded_host = true;
        assert_eq!(
            effective_host(&h, &c).as_deref(),
            Some("acme.feedbackmonk.com")
        );
    }

    #[test]
    fn host_header_is_normalised() {
        let mut h = HeaderMap::new();
        h.insert(header::HOST, HeaderValue::from_static("ACME.Feedbackmonk.com:8443"));
        assert_eq!(
            effective_host(&h, &cfg()).as_deref(),
            Some("acme.feedbackmonk.com")
        );
    }

    #[test]
    fn project_id_extracted_from_public_paths() {
        let id = Uuid::new_v4();
        for path in [
            format!("/api/v1/projects/{id}/board"),
            format!("/api/v1/projects/{id}/feedback"),
            format!("/api/v1/projects/{id}/board/items/FB-ABC123/vote"),
            format!("/api/v1/projects/{id}/widget-config"),
            format!("/api/v1/projects/{id}"),
        ] {
            assert_eq!(project_id_in_path(&path), Some(id), "path {path}");
        }
    }

    #[test]
    fn paths_without_a_project_segment_yield_none() {
        assert_eq!(project_id_in_path("/api/v1/public/site"), None);
        assert_eq!(project_id_in_path("/health"), None);
        assert_eq!(project_id_in_path("/api/v1/projects"), None);
        // A non-UUID segment is not our problem — the handler rejects it.
        assert_eq!(project_id_in_path("/api/v1/projects/not-a-uuid/board"), None);
    }

    #[test]
    fn host_scope_tenant_id_only_for_tenant_hosts() {
        let t = Uuid::new_v4();
        assert_eq!(
            HostScope::Tenant {
                tenant_id: t,
                domain_id: None
            }
            .tenant_id(),
            Some(t)
        );
        assert_eq!(HostScope::Admin.tenant_id(), None);
        assert_eq!(HostScope::AdminAlias.tenant_id(), None);
        assert_eq!(HostScope::Unbound.tenant_id(), None);
    }
}
