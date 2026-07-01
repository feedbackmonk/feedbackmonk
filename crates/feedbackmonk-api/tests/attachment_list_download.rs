//! Phase A A2 — attachment LIST + tenant-scoped DOWNLOAD integration fixture.
//!
//! Exercises the real `attachments_router` (upload → list → download) against
//! Postgres via `sqlx::test` + a per-test `LocalFsStorage`, mirroring the
//! `me_feedback_delete.rs` harness style (router `oneshot`, repo-seeded data).
//!
//! Invariants asserted here (each a named test):
//!   1. `list_returns_uploaded_set_without_storage_key` — the list endpoint
//!      returns exactly the uploaded attachments (`kind` / `content_type` /
//!      `byte_size` / `url` / `created_at`), oldest-first, and the wire JSON never
//!      contains the object-store `storage_key`.
//!   2. `download_round_trips_bytes_and_content_type` — the download endpoint
//!      returns the exact uploaded bytes with the stored `Content-Type` and a
//!      `Content-Disposition: inline` filename.
//!   3. `cross_scope_attachment_ids_404` — a cross-project or cross-feedback
//!      `attachment_id` is 404: another scope's bytes are NEVER served (the
//!      tenant-isolation invariant, DEC-FBR-03).
//!   4. `unknown_feedback_or_attachment_404` — unknown feedback short code and
//!      unknown attachment id both 404.

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::Router;
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use feedbackmonk_api::storage::{LocalFsStorage, ObjectStore};
use feedbackmonk_api::{attachments_router, AttachmentState};
use feedbackmonk_core::FeedbackKind;
use feedbackmonk_repository::{
    FeedbackRepo, ProjectRepo, ProjectScope, SqlxAttachmentRepo, SqlxFeedbackRepo,
    SqlxProjectRepo, SqlxTenantRepo, TenantRepo,
};

// ----- Test wiring ------------------------------------------------------------

const BOUNDARY: &str = "fbm-test-boundary-7f3a";

/// Build the attachments app over a fresh temp-dir `LocalFsStorage`.
fn build_app(pool: &PgPool) -> Router {
    let tmp = std::env::temp_dir().join(format!("fbm-att-ld-{}", Uuid::new_v4()));
    let storage: Arc<dyn ObjectStore> =
        Arc::new(LocalFsStorage::new(tmp, "http://test.local/attachments"));
    attachments_router(AttachmentState {
        projects: Arc::new(SqlxProjectRepo::new(pool.clone())),
        attachments: Arc::new(SqlxAttachmentRepo::new(pool.clone())),
        storage,
    })
}

/// Seed a tenant + project + one anonymous feedback row; returns
/// `(project_id, feedback short_code, project scope)`.
async fn seed_project_and_feedback(
    pool: &PgPool,
    email: &str,
    slug: &str,
) -> (Uuid, String, ProjectScope) {
    let trepo = SqlxTenantRepo::new(pool.clone());
    let prepo = SqlxProjectRepo::new(pool.clone());
    let t = trepo.create(email, "hash").await.unwrap();
    let tscope = trepo.scope_for(t.id).await.unwrap();
    let p = prepo.create(&tscope, "Proj", slug).await.unwrap();
    let pscope = prepo.open(&tscope, p.id).await.unwrap();
    let fb = seed_feedback(pool, &pscope, 1).await;
    (p.id, fb, pscope)
}

/// Seed one more anonymous feedback row in an existing scope.
async fn seed_feedback(pool: &PgPool, pscope: &ProjectScope, salt: u8) -> String {
    let frepo = SqlxFeedbackRepo::new(pool.clone());
    let fb = frepo
        .submit_anonymous(pscope, &[salt; 32], None, "body", None, FeedbackKind::Bug)
        .await
        .unwrap();
    fb.as_str().to_string()
}

/// Valid PNG magic + a distinguishing payload tag.
fn png_bytes(tag: u8) -> Vec<u8> {
    let mut v = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    v.extend_from_slice(&[tag; 24]);
    v
}

/// Build a `multipart/form-data` body with image file parts + text log parts.
fn multipart_body(files: &[(&str, &[u8])], logs: &[(&str, &str)]) -> Vec<u8> {
    let mut body = Vec::new();
    for (filename, bytes) in files {
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"files[]\"; \
                 filename=\"{filename}\"\r\nContent-Type: image/png\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    for (name, text) in logs {
        body.extend_from_slice(
            format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n")
                .as_bytes(),
        );
        body.extend_from_slice(text.as_bytes());
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

fn attachments_path(project_id: Uuid, fb: &str) -> String {
    format!("/api/v1/projects/{project_id}/feedback/{fb}/attachments")
}

/// Upload via the real multipart endpoint; returns the response JSON array.
async fn upload(app: &Router, project_id: Uuid, fb: &str, body: Vec<u8>) -> Value {
    let req = Request::builder()
        .method("POST")
        .uri(attachments_path(project_id, fb))
        .header(
            "content-type",
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(body))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "upload must succeed");
    body_to_json(resp.into_body()).await
}

async fn get(app: &Router, path: &str) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn body_to_json(body: Body) -> Value {
    let bytes = to_bytes(body, 1024 * 1024).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

// ----- Invariant 1: list mirrors the uploaded set, storage_key never leaks ----

#[sqlx::test(migrations = "../../migrations")]
async fn list_returns_uploaded_set_without_storage_key(pool: PgPool) {
    let app = build_app(&pool);
    let (project_id, fb, _scope) = seed_project_and_feedback(&pool, "list@example.com", "p-list").await;

    let img1 = png_bytes(0xAA);
    let img2 = png_bytes(0xBB);
    // PII-free log text so the stored byte_size equals the input length.
    let log_text = "widget booted; render ok";
    upload(
        &app,
        project_id,
        &fb,
        multipart_body(&[("a.png", &img1), ("b.png", &img2)], &[("service_log", log_text)]),
    )
    .await;

    let resp = get(&app, &attachments_path(project_id, &fb)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let raw = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    let raw_text = String::from_utf8(raw.to_vec()).unwrap();
    assert!(
        !raw_text.contains("storage_key"),
        "object-store key leaked into the list wire shape: {raw_text}"
    );

    let items: Value = serde_json::from_str(&raw_text).unwrap();
    let items = items.as_array().unwrap();
    assert_eq!(items.len(), 3, "list must return exactly the uploaded set");

    // Two images with the exact uploaded sizes + one scrubbed service log.
    let images: Vec<&Value> = items.iter().filter(|i| i["kind"] == "image").collect();
    assert_eq!(images.len(), 2);
    for img in &images {
        assert_eq!(img["content_type"], "image/png");
        let size = img["byte_size"].as_i64().unwrap();
        assert!(
            size == i64::try_from(img1.len()).unwrap()
                || size == i64::try_from(img2.len()).unwrap()
        );
        assert!(img["url"].as_str().unwrap().starts_with("http://test.local/attachments/"));
    }
    let log = items.iter().find(|i| i["kind"] == "service_log").unwrap();
    assert_eq!(log["content_type"], "text/plain");
    assert_eq!(log["byte_size"], i64::try_from(log_text.len()).unwrap());

    // Every item carries the full public shape, ordered oldest-first.
    let mut created: Vec<&str> = Vec::new();
    for item in items {
        assert!(item["attachment_id"].as_str().unwrap().parse::<Uuid>().is_ok());
        created.push(item["created_at"].as_str().unwrap());
    }
    let mut sorted = created.clone();
    sorted.sort_unstable();
    assert_eq!(created, sorted, "list must be ordered by created_at ascending");
}

// ----- Invariant 2: download round-trips bytes + content type ------------------

#[sqlx::test(migrations = "../../migrations")]
async fn download_round_trips_bytes_and_content_type(pool: PgPool) {
    let app = build_app(&pool);
    let (project_id, fb, _scope) = seed_project_and_feedback(&pool, "dl@example.com", "p-dl").await;

    let img = png_bytes(0xC7);
    let log_text = "no pii here; just a boot line";
    let uploaded = upload(
        &app,
        project_id,
        &fb,
        multipart_body(&[("shot.png", &img)], &[("console_log", log_text)]),
    )
    .await;
    let img_id = uploaded[0]["attachment_id"].as_str().unwrap();
    let log_id = uploaded[1]["attachment_id"].as_str().unwrap();

    // Image: exact bytes + image/png + inline disposition with a filename.
    let resp = get(&app, &format!("{}/{img_id}", attachments_path(project_id, &fb))).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()["content-type"], "image/png");
    let disposition = resp.headers()["content-disposition"].to_str().unwrap().to_string();
    assert!(disposition.starts_with("inline; filename=\""), "got: {disposition}");
    assert!(disposition.contains(".png"));
    let bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(bytes.as_ref(), img.as_slice(), "download must return the exact uploaded bytes");

    // Log: text/plain + the scrubbed (here: identical, PII-free) text.
    let resp = get(&app, &format!("{}/{log_id}", attachments_path(project_id, &fb))).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()["content-type"], "text/plain");
    let bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(bytes.as_ref(), log_text.as_bytes());
}

// ----- Invariant 3: cross-scope / cross-feedback ids never serve bytes ---------

#[sqlx::test(migrations = "../../migrations")]
async fn cross_scope_attachment_ids_404(pool: PgPool) {
    let app = build_app(&pool);
    let (p1, fb1, scope1) = seed_project_and_feedback(&pool, "xa@example.com", "p-xa").await;
    let (p2, fb2, _scope2) = seed_project_and_feedback(&pool, "xb@example.com", "p-xb").await;

    let img = png_bytes(0xEE);
    let uploaded = upload(&app, p1, &fb1, multipart_body(&[("a.png", &img)], &[])).await;
    let att1 = uploaded[0]["attachment_id"].as_str().unwrap();

    // Cross-project download: tenant B's feedback + tenant A's attachment id.
    let resp = get(&app, &format!("{}/{att1}", attachments_path(p2, &fb2))).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "cross-project attachment id must 404");

    // Cross-project feedback resolution: A's short code under project B.
    let resp = get(&app, &attachments_path(p2, &fb1)).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "cross-project short code must 404");
    let resp = get(&app, &format!("{}/{att1}", attachments_path(p2, &fb1))).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Cross-feedback within the OWNING project: a second feedback row must not
    // address the first row's attachment.
    let fb1_extra = seed_feedback(&pool, &scope1, 9).await;
    let resp = get(&app, &format!("{}/{att1}", attachments_path(p1, &fb1_extra))).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "cross-feedback attachment id must 404");

    // B's own list is clean (never inherits A's rows).
    let resp = get(&app, &attachments_path(p2, &fb2)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let items = body_to_json(resp.into_body()).await;
    assert!(items.as_array().unwrap().is_empty());

    // Owner sanity: the attachment stays fetchable through its OWN chain.
    let resp = get(&app, &format!("{}/{att1}", attachments_path(p1, &fb1))).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

// ----- Invariant 4: unknown identifiers → 404 -----------------------------------

#[sqlx::test(migrations = "../../migrations")]
async fn unknown_feedback_or_attachment_404(pool: PgPool) {
    let app = build_app(&pool);
    let (project_id, fb, _scope) = seed_project_and_feedback(&pool, "unk@example.com", "p-unk").await;

    // Unknown feedback short code: list + download both 404.
    let resp = get(&app, &attachments_path(project_id, "FB-NOPE99")).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let resp = get(
        &app,
        &format!("{}/{}", attachments_path(project_id, "FB-NOPE99"), Uuid::new_v4()),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Known feedback, unknown attachment id → 404.
    let resp = get(&app, &format!("{}/{}", attachments_path(project_id, &fb), Uuid::new_v4())).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Unknown project id → 404 (open_for_submission NotFound).
    let resp = get(&app, &attachments_path(Uuid::new_v4(), &fb)).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
