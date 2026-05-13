//! `POST   /api/v1/projects/{project_id}/signing-keys`           -- register a key.
//! `DELETE /api/v1/projects/{project_id}/signing-keys/{key_id}`  -- mark inactive.
//!
//! Contract C4: customers generate the Ed25519 keypair themselves and register
//! only the PUBLIC key here (DEC-FBR-04). The public key is exactly 32 raw
//! bytes; the request carries it base64-encoded.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use feedbackr_core::SigningKeyId;

use crate::auth::AdminSession;
use crate::error::ApiError;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct RegisterKeyRequest {
    /// Standard base64-encoded 32-byte raw Ed25519 public key.
    ///
    /// Accepts both `public_key_b64` and `public_key_base64` for the same
    /// field — Contract C4 documents the long name; Stage 2's
    /// implementation initially shipped only the short name; the serde
    /// alias bridges them so external customers reading the spec are not
    /// surprised. The contract-of-record is `public_key_base64`.
    #[serde(alias = "public_key_base64")]
    pub public_key_b64: String,
    pub label: String,
}

#[derive(Debug, Serialize)]
pub struct RegisterKeyResponse {
    pub key_id: Uuid,
    pub label: String,
    pub registered_at: DateTime<Utc>,
}

const LABEL_MAX_LEN: usize = 100;

fn validate_label(label: &str) -> Result<(), ApiError> {
    let trimmed = label.trim();
    if trimmed.is_empty() || trimmed.len() > LABEL_MAX_LEN {
        return Err(ApiError::BadRequest(format!(
            "label must be 1..={LABEL_MAX_LEN} chars after trim"
        )));
    }
    Ok(())
}

/// Decode the supplied base64 to exactly 32 bytes. Defensively rejects the
/// all-zero key (which is the Ed25519 small-subgroup point and not a useful
/// signer; a customer who registers it has misconfigured something).
pub(crate) fn decode_public_key(b64: &str) -> Result<[u8; 32], ApiError> {
    let bytes = STANDARD
        .decode(b64.trim())
        .map_err(|e| ApiError::BadRequest(format!("public_key_b64 is not base64: {e}")))?;
    if bytes.len() != 32 {
        return Err(ApiError::BadRequest(format!(
            "public key must decode to exactly 32 bytes, got {}",
            bytes.len()
        )));
    }
    if bytes.iter().all(|&b| b == 0) {
        return Err(ApiError::BadRequest("public key is all-zero".into()));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

pub async fn register(
    State(state): State<AppState>,
    session: AdminSession,
    Path(project_id): Path<Uuid>,
    Json(req): Json<RegisterKeyRequest>,
) -> Result<(StatusCode, Json<RegisterKeyResponse>), ApiError> {
    validate_label(&req.label)?;
    let public_key = decode_public_key(&req.public_key_b64)?;

    let scope = state.projects.open(&session.scope, project_id).await?;
    let id = state
        .signing_keys
        .register(&scope, &public_key, req.label.trim())
        .await?;

    // The repo's `register` does not return the registered_at timestamp;
    // pull it via list_active. The list is small per project (typically 1-3).
    let now = Utc::now();
    let registered_at = state
        .signing_keys
        .list_active(&scope)
        .await?
        .into_iter()
        .find(|k| k.id == id)
        .map_or(now, |k| k.registered_at);

    Ok((
        StatusCode::CREATED,
        Json(RegisterKeyResponse {
            key_id: id.into_uuid(),
            label: req.label.trim().to_string(),
            registered_at,
        }),
    ))
}

pub async fn deactivate(
    State(state): State<AppState>,
    session: AdminSession,
    Path((project_id, key_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let scope = state.projects.open(&session.scope, project_id).await?;
    state
        .signing_keys
        .deactivate(&scope, SigningKeyId(key_id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_public_key_accepts_32_bytes() {
        let bytes = [7u8; 32];
        let encoded = STANDARD.encode(bytes);
        let out = decode_public_key(&encoded).unwrap();
        assert_eq!(out, bytes);
    }

    #[test]
    fn decode_public_key_rejects_wrong_length() {
        let short = STANDARD.encode([1u8; 16]);
        let long = STANDARD.encode([1u8; 64]);
        assert!(decode_public_key(&short).is_err());
        assert!(decode_public_key(&long).is_err());
    }

    #[test]
    fn decode_public_key_rejects_all_zero() {
        let zero = STANDARD.encode([0u8; 32]);
        assert!(decode_public_key(&zero).is_err());
    }

    #[test]
    fn decode_public_key_rejects_non_base64() {
        assert!(decode_public_key("$$$ not base64 $$$").is_err());
    }

    #[test]
    fn label_validation() {
        validate_label("primary").unwrap();
        assert!(validate_label("").is_err());
        assert!(validate_label("   ").is_err());
        assert!(validate_label(&"x".repeat(LABEL_MAX_LEN + 1)).is_err());
    }
}
