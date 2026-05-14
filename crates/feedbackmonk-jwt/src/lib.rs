//! `feedbackmonk-jwt` -- EdDSA-only JWT verifier for feedbackmonk's submission API.
//!
//! This crate IS Contract C2 from the P0 plan. It exposes a single public
//! `verify()` function consumed by the submission handler (FR-FBR-03 +
//! FR-FBR-05). The end-user JWT is the ONLY identity feedbackmonk ever has for
//! a submitter in auth mode (DEC-FBR-04) -- there are no callbacks to the
//! customer's auth provider, no long-lived bearer tokens.
//!
//! ## Hard invariants (each enforced by a named test in
//! `tests/verify.rs` against the JWT fixture corpus)
//!
//! 1. `alg: "none"` tokens fail with `AlgorithmNotAllowed` regardless of
//!    key state.
//! 2. `alg: "HS256"` tokens (algorithm-confusion attack: HMAC signature
//!    computed with the Ed25519 public key as the HMAC secret) fail with
//!    `AlgorithmNotAllowed`.
//! 3. Wrong-audience tokens fail with `WrongAudience` even if the signature
//!    would have been valid against a key registered for a different project.
//! 4. Missing `sub`, `iat`, `exp`, or `aud` fails with
//!    `MissingRequiredClaim(name)`.
//! 5. `now_unix > exp` fails with `Expired`. The verifier is STRICT on `exp`
//!    -- no leeway. (The 5-minute sliding TTL mentioned in the plan is a
//!    customer-side minting convention, not verifier leeway.) `iat` is
//!    tolerant to clock skew via `DEFAULT_IAT_LEEWAY_SECONDS` (5s).
//! 6. `external_metadata` JSON > 4096 bytes fails with
//!    `ExternalMetadataTooLarge`.
//!
//! ## Algorithm allowlist
//!
//! `EdDSA` is the only accepted algorithm. The header is parsed BEFORE any
//! signature work, so alg-none and alg-HS256 attacks fail fast with
//! `AlgorithmNotAllowed`.
//!
//! ## Key rotation
//!
//! `active_keys` is consumed in `registered_at ASC` order (per
//! `SigningKeyRepo::list_active`); the first key whose signature verifies
//! wins. This supports zero-downtime key rotation.

#![deny(unsafe_code)]

use std::collections::HashMap;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use feedbackmonk_core::SigningKey;

/// Hard cap on `external_metadata` JSON size, per Contract C2 invariant 6.
pub const MAX_EXTERNAL_METADATA_BYTES: usize = 4096;

/// Clock-skew tolerance applied to `iat` only (not to `exp` -- exp is
/// strict per Contract C2 invariant 5).
pub const DEFAULT_IAT_LEEWAY_SECONDS: i64 = 5;

/// The single algorithm the verifier accepts. Header values other than this
/// (including `"none"` and `"HS256"`) fail fast with `AlgorithmNotAllowed`.
pub const ACCEPTED_ALG: &str = "EdDSA";

/// Verified end-user claims, returned on successful verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedClaims {
    pub sub: String,
    pub email: Option<String>,
    pub name: Option<String>,
    /// JSON object; size enforced <=4096 bytes by the verifier.
    pub external_metadata: Option<serde_json::Value>,
    pub iat: i64,
    pub exp: i64,
}

/// Verifier failure modes. Each variant maps 1:1 to a Contract C2 hard
/// invariant or to a malformed-input case.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum JwtError {
    #[error("bad signature")]
    BadSignature,
    #[error("token expired")]
    Expired,
    #[error("token not yet valid")]
    NotYetValid,
    #[error("wrong audience")]
    WrongAudience,
    #[error("algorithm not allowed")]
    AlgorithmNotAllowed,
    #[error("missing required claim: {0}")]
    MissingRequiredClaim(&'static str),
    #[error("external_metadata exceeds {MAX_EXTERNAL_METADATA_BYTES} bytes")]
    ExternalMetadataTooLarge,
    #[error("malformed token")]
    MalformedToken,
}

impl JwtError {
    /// Stable variant name for client-facing error bodies (e.g. JSON
    /// `{"error": "Expired"}`). The handler must NOT leak inner details.
    #[must_use]
    pub fn variant_name(&self) -> &'static str {
        match self {
            Self::BadSignature => "BadSignature",
            Self::Expired => "Expired",
            Self::NotYetValid => "NotYetValid",
            Self::WrongAudience => "WrongAudience",
            Self::AlgorithmNotAllowed => "AlgorithmNotAllowed",
            Self::MissingRequiredClaim(_) => "MissingRequiredClaim",
            Self::ExternalMetadataTooLarge => "ExternalMetadataTooLarge",
            Self::MalformedToken => "MalformedToken",
        }
    }
}

/// Verify a JWT against an expected project audience + active signing keys.
///
/// `now_unix` is injectable per Contract C2 (testability invariant).
/// Production callers pass the current epoch seconds; tests pass fixed
/// timestamps to drive expired / not-yet-valid paths deterministically.
///
/// Validation order (matters for error precedence):
///   1. Token shape (three base64url-encoded parts).
///   2. Header `alg` == `"EdDSA"` (rejects alg-none + alg-confusion fast).
///   3. Required claims present (`sub`, `iat`, `exp`, `aud`).
///   4. `exp` strict (`now_unix > exp` -> `Expired`).
///   5. `iat` with leeway.
///   6. `aud` matches `expected_aud_project_id`.
///   7. `external_metadata` <= 4096 bytes.
///   8. Signature verifies against at least one key in `active_keys`.
///
/// Returning Wrong-Audience BEFORE signature check is intentional: a token
/// signed by project A's key that is delivered to project B's endpoint
/// fails fast with WrongAudience (Contract C2 invariant 3), without
/// leaking whether a candidate key would have verified.
pub fn verify(
    token: &str,
    expected_aud_project_id: Uuid,
    active_keys: &[SigningKey],
    now_unix: i64,
) -> Result<VerifiedClaims, JwtError> {
    verify_with_leeway(
        token,
        expected_aud_project_id,
        active_keys,
        now_unix,
        DEFAULT_IAT_LEEWAY_SECONDS,
    )
}

/// Like `verify`, but with caller-controlled `iat` clock-skew leeway.
/// Used by tests; production code calls `verify` directly.
pub fn verify_with_leeway(
    token: &str,
    expected_aud_project_id: Uuid,
    active_keys: &[SigningKey],
    now_unix: i64,
    iat_leeway_seconds: i64,
) -> Result<VerifiedClaims, JwtError> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(JwtError::MalformedToken);
    }
    let header_b64 = parts[0];
    let payload_b64 = parts[1];
    let sig_b64 = parts[2];

    // ----- 1. Header / algorithm allowlist (Contract C2 invariants 1 + 2) -----
    let header_bytes = URL_SAFE_NO_PAD
        .decode(header_b64)
        .map_err(|_| JwtError::MalformedToken)?;
    let header: HashMap<String, serde_json::Value> =
        serde_json::from_slice(&header_bytes).map_err(|_| JwtError::MalformedToken)?;
    let alg = header
        .get("alg")
        .and_then(serde_json::Value::as_str)
        .ok_or(JwtError::AlgorithmNotAllowed)?;
    if alg != ACCEPTED_ALG {
        return Err(JwtError::AlgorithmNotAllowed);
    }

    // ----- 2. Payload / required claims (Contract C2 invariant 4) -----
    let payload_bytes = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .map_err(|_| JwtError::MalformedToken)?;
    let payload: HashMap<String, serde_json::Value> =
        serde_json::from_slice(&payload_bytes).map_err(|_| JwtError::MalformedToken)?;

    let sub = payload
        .get("sub")
        .and_then(serde_json::Value::as_str)
        .ok_or(JwtError::MissingRequiredClaim("sub"))?
        .to_string();
    let iat = payload
        .get("iat")
        .and_then(serde_json::Value::as_i64)
        .ok_or(JwtError::MissingRequiredClaim("iat"))?;
    let exp = payload
        .get("exp")
        .and_then(serde_json::Value::as_i64)
        .ok_or(JwtError::MissingRequiredClaim("exp"))?;
    let aud_str = payload
        .get("aud")
        .and_then(serde_json::Value::as_str)
        .ok_or(JwtError::MissingRequiredClaim("aud"))?;

    // ----- 3. Temporal checks (Contract C2 invariant 5) -----
    if now_unix > exp {
        return Err(JwtError::Expired);
    }
    if iat > now_unix + iat_leeway_seconds {
        return Err(JwtError::NotYetValid);
    }

    // ----- 4. Audience (Contract C2 invariant 3) -----
    let expected_aud = expected_aud_project_id.to_string();
    if aud_str != expected_aud {
        return Err(JwtError::WrongAudience);
    }

    // ----- 5. external_metadata cap (Contract C2 invariant 6) -----
    let external_metadata = payload.get("external_metadata").cloned();
    if let Some(ref meta) = external_metadata {
        let encoded = serde_json::to_vec(meta).map_err(|_| JwtError::MalformedToken)?;
        if encoded.len() > MAX_EXTERNAL_METADATA_BYTES {
            return Err(JwtError::ExternalMetadataTooLarge);
        }
    }

    // ----- 6. Signature (try each active key, first success wins) -----
    let signing_input = format!("{header_b64}.{payload_b64}");
    let sig_bytes = URL_SAFE_NO_PAD
        .decode(sig_b64)
        .map_err(|_| JwtError::MalformedToken)?;
    let sig_arr: [u8; 64] = sig_bytes
        .as_slice()
        .try_into()
        .map_err(|_| JwtError::MalformedToken)?;
    let signature = Signature::from_bytes(&sig_arr);

    for key in active_keys {
        let Ok(pk_arr) = <[u8; 32]>::try_from(key.public_key.as_slice()) else {
            continue;
        };
        let Ok(verifying_key) = VerifyingKey::from_bytes(&pk_arr) else {
            continue;
        };
        if verifying_key
            .verify_strict(signing_input.as_bytes(), &signature)
            .is_ok()
        {
            let email = payload
                .get("email")
                .and_then(serde_json::Value::as_str)
                .map(String::from);
            let name = payload
                .get("name")
                .and_then(serde_json::Value::as_str)
                .map(String::from);
            return Ok(VerifiedClaims {
                sub,
                email,
                name,
                external_metadata,
                iat,
                exp,
            });
        }
    }
    Err(JwtError::BadSignature)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_names_are_stable_strings() {
        let err = JwtError::MissingRequiredClaim("sub");
        assert_eq!(err.variant_name(), "MissingRequiredClaim");
        assert_eq!(JwtError::Expired.variant_name(), "Expired");
        assert_eq!(
            JwtError::AlgorithmNotAllowed.variant_name(),
            "AlgorithmNotAllowed"
        );
        assert_eq!(JwtError::WrongAudience.variant_name(), "WrongAudience");
        assert_eq!(
            JwtError::ExternalMetadataTooLarge.variant_name(),
            "ExternalMetadataTooLarge"
        );
    }

    #[test]
    fn accepted_alg_is_eddsa_only() {
        // Stable expectation: no other algorithm is ever accepted by the
        // verifier (Contract C2 invariants 1 + 2).
        assert_eq!(ACCEPTED_ALG, "EdDSA");
    }

    #[test]
    fn malformed_three_part_split_required() {
        let active_keys: Vec<SigningKey> = vec![];
        let result = verify("not-a-jwt", Uuid::nil(), &active_keys, 0);
        assert_eq!(result.unwrap_err(), JwtError::MalformedToken);
        let result2 = verify("aa.bb", Uuid::nil(), &active_keys, 0);
        assert_eq!(result2.unwrap_err(), JwtError::MalformedToken);
        let result3 = verify("aa.bb.cc.dd", Uuid::nil(), &active_keys, 0);
        assert_eq!(result3.unwrap_err(), JwtError::MalformedToken);
    }
}
