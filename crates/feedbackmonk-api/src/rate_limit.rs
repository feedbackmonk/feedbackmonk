//! Class-level per-IP rate-limit middleware for the PUBLIC route surface
//! (submission, attachments, board, roadmap).
//!
//! ## Why this exists (scrutiny 2026-07-01, findings P0-2 / P1-2)
//!
//! The fine-grained [`AnonGate`](feedbackmonk_anon::AnonGate) keys its budget on
//! a client-CONTROLLED, unsigned cookie, so rotating/omitting the cookie mints
//! unlimited fresh budgets — and the authenticated (Bearer-present) submit path
//! had NO limiter at all. Placing the limiter per-handler also meant every route
//! added after P0 (voting, attachments) silently inherited the gap.
//!
//! This module moves the abuse *floor* to a router-level middleware applied by
//! default to every public router, keyed on the resolved client IP alone. The
//! per-cookie `AnonGate` remains a *refinement on top* (finer dedup), never the
//! floor. The IP is resolved trusted-proxy-aware
//! ([`resolve_client_ip`](feedbackmonk_anon::resolve_client_ip)) so it is not
//! the raw TCP peer that collapses to the load balancer behind Railway.
//!
//! The `public-route-ceiling` Verification Oracle asserts every public router in
//! `build_app` is wrapped by [`apply_public_rate_limit`] — so a future public
//! route cannot silently re-open the gap.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::{
    extract::{ConnectInfo, State},
    http::{HeaderValue, Request, StatusCode},
    middleware::{from_fn_with_state, Next},
    response::{IntoResponse, Response},
    Json, Router,
};
use feedbackmonk_anon::{resolve_client_ip, IpGate, RateLimitError};
use serde_json::json;

/// Middleware state: the shared per-IP gate + the trusted-proxy hop count.
/// Cloned into every public router's layer (cheap — `IpGate` is `Arc`-backed).
#[derive(Clone)]
pub struct PublicRateLimit {
    gate: IpGate,
    trusted_hops: usize,
}

impl PublicRateLimit {
    #[must_use]
    pub fn new(gate: IpGate, trusted_hops: usize) -> Self {
        Self { gate, trusted_hops }
    }
}

/// Wrap a public router with the class-level per-IP ceiling. This is the single
/// marker the `public-route-ceiling` oracle greps for; every public router in
/// `build_app` must be passed through it.
pub fn apply_public_rate_limit(router: Router, cfg: PublicRateLimit) -> Router {
    router.layer(from_fn_with_state(cfg, public_ip_rate_limit))
}

async fn public_ip_rate_limit(
    State(cfg): State<PublicRateLimit>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    // Peer IP is always present in production (the server uses
    // `into_make_service_with_connect_info::<SocketAddr>`); fall back to a
    // loopback sentinel only if the extension is absent (in-process tests that
    // don't inject ConnectInfo).
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map_or(IpAddr::V4(Ipv4Addr::LOCALHOST), |c| c.0.ip());
    let xff = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok());
    let client_ip = resolve_client_ip(xff, peer, cfg.trusted_hops);

    match cfg.gate.check(client_ip) {
        Ok(()) => next.run(req).await,
        Err(RateLimitError::Exceeded { retry_after_seconds }) => {
            too_many_requests(retry_after_seconds)
        }
    }
}

/// 429 response matching the submit handler's shape (`error` +
/// `retry_after_seconds` + `Retry-After` header).
fn too_many_requests(retry_after_seconds: u64) -> Response {
    let body = Json(json!({
        "error": "RateLimitExceeded",
        "retry_after_seconds": retry_after_seconds,
    }));
    let mut response = (StatusCode::TOO_MANY_REQUESTS, body).into_response();
    if let Ok(v) = HeaderValue::from_str(&retry_after_seconds.to_string()) {
        response.headers_mut().insert("Retry-After", v);
    }
    response
}
