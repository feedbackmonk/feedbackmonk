//! CORS behavior for the public credentialed widget endpoints (submission +
//! attachments). Verifies the fix for the GitCellar customer-#1 embed blocker:
//! the cross-origin browser preflight must succeed (not `405`) and the
//! credentialed response must echo the *specific* origin + allow credentials.
//!
//! These tests exercise the exported [`public_cors_layer`] directly over a
//! trivial `POST` route. The CORS layer short-circuits the preflight before any
//! handler runs, so no database/state is required — the layer's policy is the
//! unit under test.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::routing::post;
use axum::Router;
use tower::ServiceExt;

use feedbackmonk_api::public_cors_layer;

const ALLOWED: &str = "https://gitcellar.com";
const DISALLOWED: &str = "https://evil.example";
const ROUTE: &str = "/api/v1/projects/abc/feedback";

fn app() -> Router {
    Router::new()
        .route(ROUTE, post(|| async { "ok" }))
        .layer(public_cors_layer(&[ALLOWED.to_string()]))
}

/// The browser preflight (`OPTIONS` + `Access-Control-Request-Method`) from an
/// allowed origin must be answered by the CORS layer with the `Access-Control-*`
/// headers — NOT the `405` a bare `POST`-only route would return. This is the
/// exact symptom that blocked GitCellar's widget.
#[tokio::test]
async fn preflight_from_allowed_origin_is_handled_not_405() {
    let req = Request::builder()
        .method(Method::OPTIONS)
        .uri(ROUTE)
        .header("origin", ALLOWED)
        .header("access-control-request-method", "POST")
        .header("access-control-request-headers", "content-type, authorization")
        .body(Body::empty())
        .unwrap();

    let resp = app().oneshot(req).await.unwrap();

    assert_ne!(
        resp.status(),
        StatusCode::METHOD_NOT_ALLOWED,
        "preflight must not 405"
    );
    assert!(
        resp.status().is_success(),
        "preflight should be 2xx, got {}",
        resp.status()
    );

    let h = resp.headers();
    assert_eq!(
        h.get("access-control-allow-origin").unwrap(),
        ALLOWED,
        "must echo the specific origin"
    );
    assert_eq!(
        h.get("access-control-allow-credentials").unwrap(),
        "true"
    );

    let methods = h
        .get("access-control-allow-methods")
        .unwrap()
        .to_str()
        .unwrap()
        .to_ascii_uppercase();
    assert!(methods.contains("POST"), "allow-methods must include POST");

    let allow_headers = h
        .get("access-control-allow-headers")
        .unwrap()
        .to_str()
        .unwrap()
        .to_ascii_lowercase();
    assert!(allow_headers.contains("content-type"));
    assert!(allow_headers.contains("authorization"));
}

/// The actual credentialed request (anonymous path uses `credentials:
/// "include"`) must come back with `Access-Control-Allow-Origin` echoing the
/// *specific* origin — never `*`, which browsers reject when credentials are
/// involved — plus `Access-Control-Allow-Credentials: true`.
#[tokio::test]
async fn credentialed_request_echoes_specific_origin_never_wildcard() {
    let req = Request::builder()
        .method(Method::POST)
        .uri(ROUTE)
        .header("origin", ALLOWED)
        .body(Body::empty())
        .unwrap();

    let resp = app().oneshot(req).await.unwrap();

    let acao = resp
        .headers()
        .get("access-control-allow-origin")
        .expect("allow-origin present for allowed origin")
        .to_str()
        .unwrap();
    assert_eq!(acao, ALLOWED);
    assert_ne!(acao, "*", "credentialed responses must not use the wildcard");
    assert_eq!(
        resp.headers()
            .get("access-control-allow-credentials")
            .unwrap(),
        "true"
    );
}

/// A preflight from a non-allowlisted origin gets no `Access-Control-Allow-Origin`
/// header, so the browser blocks it. This is the "rejection for a disallowed
/// one" half of the contract.
#[tokio::test]
async fn preflight_from_disallowed_origin_gets_no_allow_origin() {
    let req = Request::builder()
        .method(Method::OPTIONS)
        .uri(ROUTE)
        .header("origin", DISALLOWED)
        .header("access-control-request-method", "POST")
        .body(Body::empty())
        .unwrap();

    let resp = app().oneshot(req).await.unwrap();

    assert!(
        resp.headers().get("access-control-allow-origin").is_none(),
        "disallowed origin must not receive an allow-origin header"
    );
}

/// An empty allowlist (the default when `FEEDBACKMONK_CORS_ORIGINS` is unset)
/// allows no cross-origin origin — the secure default.
#[tokio::test]
async fn empty_allowlist_blocks_all_cross_origin() {
    let app = Router::new()
        .route(ROUTE, post(|| async { "ok" }))
        .layer(public_cors_layer(&[]));

    let req = Request::builder()
        .method(Method::OPTIONS)
        .uri(ROUTE)
        .header("origin", ALLOWED)
        .header("access-control-request-method", "POST")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert!(resp.headers().get("access-control-allow-origin").is_none());
}
