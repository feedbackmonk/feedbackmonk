//! `POST /api/v1/projects/{project_id}/feedback/{feedback_id}/attachments`
//! — multipart attachment upload (Gap #1, GUIDE §6 frozen contract).
//!
//! ## Contract (ratified ALPHA1 ↔ ALPHA2 in collab-20260602-123000)
//!
//! - `multipart/form-data`.
//! - Image parts: field name **`files[]`** (repeatable, ≤4 total per feedback),
//!   each ≤5 MB, `Content-Type` ∈ {`image/png`,`image/jpeg`,`image/webp`}
//!   (enforced by BOTH the part header AND a magic-byte sniff).
//! - Log parts: text fields **`service_log`** / **`console_log`** (optional,
//!   0..1 each). Each is PII-scrubbed via the canonical `feedbackmonk-tracing`
//!   20-pattern chokepoint (`scrub_log_for_storage`) BEFORE the bytes reach the
//!   object store — raw log text is NEVER persisted (FR-FBR-10).
//! - `:feedback_id` is the public `FB-XXXXXX` short code; it must already exist
//!   in the project scope (else `404`).
//! - Response `200`: bare JSON array `[{"attachment_id","kind","url"}, …]`.
//!
//! ## Status codes
//!
//! `404` unknown project/feedback · `413` a file >5 MB or >4 images ·
//! `415` disallowed/forged image MIME · `400` malformed multipart or an empty
//! upload (no files and no logs).
//!
//! ## Auth model
//!
//! Like the public submission endpoint (DEC-FBR-04 / DEC-PODS-001) this route is
//! public and scoped by `(project_id, feedback short_code)` via
//! `open_for_submission`. The widget calls it immediately after submission with
//! the returned `feedback_id` (auth-mode Bearer or anonymous cookie). Binding
//! the upload to the submitter's identity is a hardening follow-up tracked in
//! the completion notes; v1 parity matches the frozen contract.
//!
//! ## Isolation oracle
//!
//! No SQL here — every attachment read/write goes through `AttachmentRepo`
//! (`multi-tenant-isolation-check` Probe A clean by construction). Scope is
//! minted by `open_for_submission` and threaded into every repo call.

use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Multipart, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use uuid::Uuid;

use feedbackmonk_repository::{AttachmentKind, AttachmentRepo, NewAttachment, ProjectRepo};

use crate::error::ApiError;
use crate::storage::ObjectStore;

/// State for the attachment upload sub-router. Intentionally SEPARATE from the
/// global `AppState` so adding attachments costs zero edits to the many
/// `AppState { … }` construction sites across the codebase + tests (a
/// same-branch PODS courtesy — see collab-20260602-123000). Built in `main.rs`
/// from the existing project repo + a fresh attachment repo + the env-selected
/// object store.
#[derive(Clone)]
pub struct AttachmentState {
    /// For `open_for_submission` (pre-auth project-scope mint, DEC-PODS-001).
    pub projects: Arc<dyn ProjectRepo>,
    /// Attachment metadata repository (migration 00009).
    pub attachments: Arc<dyn AttachmentRepo>,
    /// Object store for attachment bytes (local FS / S3-compatible).
    pub storage: Arc<dyn ObjectStore>,
}

/// Max images per feedback (GUIDE §6).
const MAX_IMAGES: i64 = 4;
/// Max bytes for a single image (GUIDE §6: ≤5 MB).
const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
/// Whole-request body cap: 4 images × 5 MB + log headroom. Protects against
/// OOM from an oversized multipart body (axum returns 413 above this).
const MAX_UPLOAD_BYTES: usize = 25 * 1024 * 1024;
/// Defensive cap on a single captured log part (post-scrub stored as text).
const MAX_LOG_BYTES: usize = 1024 * 1024;

/// Scrub raw captured log text through the canonical `feedbackmonk-tracing`
/// 20-pattern PII chokepoint and return the bytes to persist. This is the SOLE
/// transform applied to log text before storage — the `attachment_pii_corpus`
/// Task-Zero fixture asserts on exactly this function's output. Reusing
/// `feedbackmonk_tracing::scrub` (not a second scrub path) keeps the
/// `pii-scrub-audit` Probe A clean.
#[must_use]
pub fn scrub_log_for_storage(raw: &str) -> Vec<u8> {
    feedbackmonk_tracing::scrub(raw).into_bytes()
}

/// A pending image part read from the multipart body.
struct PendingImage {
    bytes: Vec<u8>,
    content_type: &'static str,
    ext: &'static str,
}

/// `POST …/feedback/{feedback_id}/attachments`.
#[allow(clippy::too_many_lines)] // linear parse → validate → persist; clearer as one flow.
pub async fn upload(
    State(state): State<AttachmentState>,
    Path((project_id, feedback_id)): Path<(Uuid, String)>,
    mut multipart: Multipart,
) -> Result<Response, ApiError> {
    // ----- 1. Scope + feedback resolution ---------------------------------
    let scope = state.projects.open_for_submission(project_id).await?;
    let feedback_uuid = state
        .attachments
        .resolve_feedback_uuid(&scope, &feedback_id)
        .await?; // RepoError::NotFound → 404

    // ----- 2. Parse multipart ---------------------------------------------
    let mut images: Vec<PendingImage> = Vec::new();
    let mut service_log: Option<String> = None;
    let mut console_log: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::BadRequest(format!("malformed multipart: {e}")))?
    {
        let name = field.name().unwrap_or("").to_string();
        let declared_ct = field.content_type().map(str::to_string);

        match name.as_str() {
            "files[]" => {
                let data = field
                    .bytes()
                    .await
                    .map_err(|e| ApiError::BadRequest(format!("could not read file part: {e}")))?;
                if data.len() > MAX_IMAGE_BYTES {
                    return Err(ApiError::PayloadTooLarge(format!(
                        "image exceeds {MAX_IMAGE_BYTES} bytes"
                    )));
                }
                let ct = declared_ct.unwrap_or_default();
                match validate_image(&ct, &data) {
                    Ok((content_type, ext)) => images.push(PendingImage {
                        bytes: data.to_vec(),
                        content_type,
                        ext,
                    }),
                    Err(resp) => return Ok(resp),
                }
            }
            "service_log" => {
                let text = read_log_field(field).await?;
                service_log = Some(text);
            }
            "console_log" => {
                let text = read_log_field(field).await?;
                console_log = Some(text);
            }
            _ => {
                // Drain unknown fields so the stream advances.
                let _ = field.bytes().await;
            }
        }
    }

    // ----- 3. Validate the upload as a whole ------------------------------
    let n_logs = usize::from(service_log.is_some()) + usize::from(console_log.is_some());
    if images.is_empty() && n_logs == 0 {
        return Err(ApiError::BadRequest(
            "upload must contain at least one file or log part".into(),
        ));
    }

    // ≤4 images per feedback (existing + this request).
    let existing_images = state.attachments.count_images(&scope, feedback_uuid).await?;
    let incoming = i64::try_from(images.len()).unwrap_or(i64::MAX);
    if existing_images + incoming > MAX_IMAGES {
        return Err(ApiError::PayloadTooLarge(format!(
            "too many images: {existing_images} already attached + {incoming} new exceeds the {MAX_IMAGES}-image limit"
        )));
    }

    // ----- 4. Persist: store bytes, then insert metadata rows -------------
    let mut out: Vec<serde_json::Value> = Vec::with_capacity(images.len() + n_logs);

    for img in images {
        let att_uuid = Uuid::new_v4();
        let key = format!("attachments/{project_id}/{feedback_uuid}/{att_uuid}.{}", img.ext);
        let url = state
            .storage
            .put(&key, img.content_type, &img.bytes)
            .await
            .map_err(|e| ApiError::Internal(format!("attachment storage failed: {e}")))?;
        let id = state
            .attachments
            .insert(
                &scope,
                feedback_uuid,
                &NewAttachment {
                    kind: AttachmentKind::Image,
                    storage_key: &key,
                    url: &url,
                    content_type: img.content_type,
                    byte_size: i64::try_from(img.bytes.len()).unwrap_or(i64::MAX),
                },
            )
            .await?;
        out.push(json!({ "attachment_id": id.to_string(), "kind": "image", "url": url }));
    }

    for (raw, kind) in [
        (service_log, AttachmentKind::ServiceLog),
        (console_log, AttachmentKind::ConsoleLog),
    ] {
        let Some(raw) = raw else { continue };
        // PII-scrub BEFORE persist — the canonical chokepoint, never raw text.
        let scrubbed = scrub_log_for_storage(&raw);
        let att_uuid = Uuid::new_v4();
        let key = format!(
            "attachments/{project_id}/{feedback_uuid}/{att_uuid}-{}.log",
            kind.as_str()
        );
        let url = state
            .storage
            .put(&key, "text/plain", &scrubbed)
            .await
            .map_err(|e| ApiError::Internal(format!("attachment storage failed: {e}")))?;
        let id = state
            .attachments
            .insert(
                &scope,
                feedback_uuid,
                &NewAttachment {
                    kind,
                    storage_key: &key,
                    url: &url,
                    content_type: "text/plain",
                    byte_size: i64::try_from(scrubbed.len()).unwrap_or(i64::MAX),
                },
            )
            .await?;
        out.push(json!({ "attachment_id": id.to_string(), "kind": kind.as_str(), "url": url }));
    }

    Ok((StatusCode::OK, Json(out)).into_response())
}

/// Read a log text field with a defensive size cap.
async fn read_log_field(field: axum::extract::multipart::Field<'_>) -> Result<String, ApiError> {
    let text = field
        .text()
        .await
        .map_err(|e| ApiError::BadRequest(format!("could not read log part: {e}")))?;
    if text.len() > MAX_LOG_BYTES {
        return Err(ApiError::PayloadTooLarge(format!(
            "log part exceeds {MAX_LOG_BYTES} bytes"
        )));
    }
    Ok(text)
}

/// Validate an image part: the declared MIME must be in the allowlist AND the
/// leading bytes must match that type (magic-byte sniff). Returns
/// `(canonical_content_type, file_extension)` on success, or a `415` response.
// reason: the Err variant is an axum `Response` (large by nature); boxing it
// would ripple into the call site and the `.unwrap_err()` test assertions for
// no benefit, since the value is constructed and returned in one place.
#[allow(clippy::result_large_err)]
fn validate_image(declared_ct: &str, bytes: &[u8]) -> Result<(&'static str, &'static str), Response> {
    // Some clients append `; charset=…` — match on the media type prefix.
    let media = declared_ct.split(';').next().unwrap_or("").trim();
    let (content_type, ext, magic_ok): (&'static str, &'static str, bool) = match media {
        "image/png" => (
            "image/png",
            "png",
            bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]),
        ),
        "image/jpeg" => ("image/jpeg", "jpg", bytes.starts_with(&[0xFF, 0xD8, 0xFF])),
        "image/webp" => (
            "image/webp",
            "webp",
            bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        ),
        other => return Err(unsupported_media_type(&format!(
            "image content-type {other:?} not allowed (png/jpeg/webp only)"
        ))),
    };
    if !magic_ok {
        return Err(unsupported_media_type(&format!(
            "file bytes do not match declared content-type {content_type}"
        )));
    }
    Ok((content_type, ext))
}

/// Build a `415 Unsupported Media Type` response with the canonical `ApiError`
/// body shape (`{"error": "<msg>"}`). Built inline rather than adding a 415
/// variant to the shared `ApiError` enum (keeps `error.rs` untouched).
fn unsupported_media_type(msg: &str) -> Response {
    (StatusCode::UNSUPPORTED_MEDIA_TYPE, Json(json!({ "error": msg }))).into_response()
}

/// Attachment upload subtree. Merged into the main router by `main.rs`.
/// Raises the body limit to `MAX_UPLOAD_BYTES` so 4×5 MB images fit.
pub fn attachments_router(state: AttachmentState) -> axum::Router {
    axum::Router::new()
        .route(
            "/api/v1/projects/:project_id/feedback/:feedback_id/attachments",
            axum::routing::post(upload),
        )
        .layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_image_accepts_png_with_magic() {
        let png = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00];
        let (ct, ext) = validate_image("image/png", &png).unwrap();
        assert_eq!(ct, "image/png");
        assert_eq!(ext, "png");
    }

    #[test]
    fn validate_image_accepts_jpeg_and_webp() {
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0x00];
        assert_eq!(validate_image("image/jpeg", &jpeg).unwrap().1, "jpg");
        let mut webp = b"RIFF\0\0\0\0WEBP".to_vec();
        webp.extend_from_slice(b"more");
        assert_eq!(validate_image("image/webp", &webp).unwrap().1, "webp");
    }

    #[test]
    fn validate_image_tolerates_charset_suffix() {
        let png = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        assert!(validate_image("image/png; charset=binary", &png).is_ok());
    }

    #[test]
    fn validate_image_rejects_disallowed_mime() {
        let gif = b"GIF89a___".to_vec();
        let resp = validate_image("image/gif", &gif).unwrap_err();
        assert_eq!(resp.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[test]
    fn validate_image_rejects_forged_magic() {
        // Declared PNG but bytes are not a PNG → 415 (magic mismatch).
        let not_png = b"\xFF\xD8\xFFnotpng".to_vec();
        let resp = validate_image("image/png", &not_png).unwrap_err();
        assert_eq!(resp.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[test]
    fn scrub_log_for_storage_strips_pii() {
        let raw = "user mark@example.com from 10.0.0.1";
        let out = String::from_utf8(scrub_log_for_storage(raw)).unwrap();
        assert!(!out.contains("mark@example.com"));
        assert!(!out.contains("10.0.0.1"));
        assert!(out.contains("[email]"));
        assert!(out.contains("[ip]"));
    }
}
