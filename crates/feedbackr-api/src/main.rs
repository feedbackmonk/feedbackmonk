//! Stage 1 placeholder binary. Reads FEEDBACKR_PORT (default 14304) and binds
//! axum on 127.0.0.1. Stage 2 Worker A and Worker B add the real router tree.
//!
//! See docs/operations/LOCAL_DEV.md for dev environment setup.

use std::net::SocketAddr;

use axum::{routing::get, Router};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port: u16 = std::env::var("FEEDBACKR_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(14304);
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();

    let app = Router::new().route("/", get(|| async { "feedbackr-api stage1 placeholder" }));

    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("feedbackr-api listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
